//! LLVM 20 textual IR generation.
//!
//! Design notes:
//!   - Opaque pointers only (`ptr`), matching LLVM 20.
//!   - No data layout is emitted; `llc` applies the default layout for
//!     `x86_64-unknown-linux-gnu`, which keeps the emitter independent of a
//!     specific LLVM build's layout string.
//!   - All locals and array temporaries are allocated in the entry block, so
//!     `alloca` never executes inside a loop.
//!   - Every operation has defined behavior: division by zero and signed
//!     division overflow call `__corex_fatal` (exit 101), and dynamic array
//!     indices are bounds-checked.
//!   - `syscall`, `alloc`, and `free` lower to inline-asm syscalls
//!     (`syscall` instruction) so the produced executable needs no libc.

use std::collections::HashMap;

use crate::ast::*;
use crate::diagnostics::{SourceFile, Span};
use crate::semantic::{Analysis, Callee, FnSig, GlobalDef};

struct DebugCtx {
    file: SourceFile,
    nodes: Vec<String>,
    next: u32,
    file_id: u32,
    unit_id: u32,
    scope: u32,
    locs: HashMap<(u32, u32, u32), u32>,
    type_cache: HashMap<Type, u32>,
}

impl DebugCtx {
    fn new(file: SourceFile) -> DebugCtx {
        let mut d = DebugCtx {
            file: file.clone(),
            nodes: Vec::new(),
            next: 0,
            file_id: 0,
            unit_id: 0,
            scope: 0,
            locs: HashMap::new(),
            type_cache: HashMap::new(),
        };
        let path = std::path::Path::new(&file.name);
        let name = path
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| file.name.clone());
        let dir = path
            .parent()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| ".".to_string());
        d.file_id = d.add(format!(
            "!DIFile(filename: {name:?}, directory: {dir:?})"
        ));
        d.unit_id = d.add(format!(
            "distinct !DICompileUnit(language: DW_LANG_C99, file: !{}, producer: \"corex\", isOptimized: false, runtimeVersion: 0, emissionKind: FullDebug)",
            d.file_id
        ));
        d
    }

    fn add(&mut self, text: String) -> u32 {
        let id = self.next;
        self.next += 1;
        self.nodes.push(format!("!{id} = {text}"));
        id
    }

    fn line_col(&self, span: Span) -> (u32, u32) {
        let (_, line, col) = self.file.source_location(span.start);
        (line as u32, col as u32)
    }

    fn loc(&mut self, span: Span, scope: u32) -> u32 {
        let (line, col) = self.line_col(span);
        let key = (line, col, scope);
        if let Some(id) = self.locs.get(&key) {
            return *id;
        }
        let id = self.add(format!(
            "!DILocation(line: {line}, column: {col}, scope: !{scope})"
        ));
        self.locs.insert(key, id);
        id
    }

    fn type_meta(&mut self, ty: &Type) -> u32 {
        if let Some(id) = self.type_cache.get(ty) {
            return *id;
        }
        let text = match ty {
            Type::Unit => "!DIBasicType(name: \"void\", size: 0, encoding: DW_ATE_unsigned)".to_string(),
            Type::Bool => "!DIBasicType(name: \"bool\", size: 8, encoding: DW_ATE_boolean)".to_string(),
            Type::Char => "!DIBasicType(name: \"char\", size: 8, encoding: DW_ATE_unsigned_char)".to_string(),
            Type::Float(f) => format!(
                "!DIBasicType(name: \"{}\", size: {}, encoding: DW_ATE_float)",
                f.name(),
                f.bits()
            ),
            Type::Int(t) => format!(
                "!DIBasicType(name: \"{}\", size: {}, encoding: {})",
                t.name(),
                t.bits(),
                if t.signed() { "DW_ATE_signed" } else { "DW_ATE_unsigned" }
            ),
            Type::Enum(name) => format!(
                "!DIBasicType(name: \"{name}\", size: 32, encoding: DW_ATE_signed)"
            ),
            Type::Ptr { pointee, .. } => {
                let base = self.type_meta(pointee);
                format!(
                    "!DIDerivedType(tag: DW_TAG_pointer_type, baseType: !{base}, size: 64)"
                )
            }
            Type::Fn(_) | Type::CStr => {
                "!DIDerivedType(tag: DW_TAG_pointer_type, size: 64)".to_string()
            }
            Type::Slice(_) | Type::Str => format!(
                "!DIBasicType(name: \"{}\", size: 128, encoding: DW_ATE_unsigned)",
                ty.display()
            ),
            Type::Vec(_) => format!(
                "!DIBasicType(name: \"{}\", size: 192, encoding: DW_ATE_unsigned)",
                ty.display()
            ),
            Type::Array { elem, len } => {
                let base = self.type_meta(elem);
                format!(
                    "!DICompositeType(tag: DW_TAG_array_type, baseType: !{base}, size: {})",
                    elem.size_bytes() * len * 8
                )
            }
            Type::Struct(info) => format!(
                "!DIBasicType(name: \"{}\", size: {}, encoding: DW_ATE_unsigned)",
                info.name,
                info.size.get() * 8
            ),
            Type::Named(name) => format!(
                "!DIBasicType(name: \"{name}\", size: 64, encoding: DW_ATE_unsigned)"
            ),
        };
        let id = self.add(text);
        self.type_cache.insert(ty.clone(), id);
        id
    }

    fn module_text(&mut self) -> String {
        let dwarf = self.add("!{i32 7, !\"Dwarf Version\", i32 5}".to_string());
        let version = self.add("!{i32 2, !\"Debug Info Version\", i32 3}".to_string());
        let mut out = format!(
            "!llvm.dbg.cu = !{{!{}}}\n!llvm.module.flags = !{{!{dwarf}, !{version}}}\n",
            self.unit_id
        );
        for node in &self.nodes {
            out.push_str(node);
            out.push('\n');
        }
        out
    }
}

pub fn generate(program: &Program, analysis: &Analysis, module_name: &str) -> String {
    generate_with_mode(program, analysis, module_name, None, None, false, false)
}

/// Generate a module with DWARF line tables, function, and local-variable
/// metadata derived from `file`'s source spans.
pub fn generate_debug(
    program: &Program,
    analysis: &Analysis,
    module_name: &str,
    file: &SourceFile,
) -> String {
    generate_with_mode(program, analysis, module_name, None, Some(file), false, false)
}

/// Generate a module with optional debug info and checked integer arithmetic.
pub fn generate_full(
    program: &Program,
    analysis: &Analysis,
    module_name: &str,
    file: Option<&SourceFile>,
    sanitize: bool,
    hosted: bool,
) -> String {
    generate_with_mode(program, analysis, module_name, None, file, sanitize, hosted)
}

/// Generate a module whose entry point runs `tests` and reports results.
pub fn generate_tests(
    program: &Program,
    analysis: &Analysis,
    module_name: &str,
    tests: &[String],
) -> String {
    generate_with_mode(program, analysis, module_name, Some(tests), None, false, false)
}

fn generate_with_mode(
    program: &Program,
    analysis: &Analysis,
    module_name: &str,
    tests: Option<&[String]>,
    debug_file: Option<&SourceFile>,
    sanitize: bool,
    hosted: bool,
) -> String {
    let mut cg = CodeGen {
        analysis,
        out: String::new(),
        entry: String::new(),
        fn_defs: Vec::new(),
        globals: Vec::new(),
        strings: Vec::new(),
        tmp: 0,
        lbl: 0,
        slot_ptrs: Vec::new(),
        terminated: false,
        current_label: "entry".to_string(),
        break_stack: Vec::new(),
        continue_stack: Vec::new(),
        label_map: HashMap::new(),
        current_fn: String::new(),
        debug: debug_file.map(|f| DebugCtx::new(f.clone())),
        sanitize,
        current_scope: 0,
        current_span: Span::new(0, 0),
        used_syscall: false,
        used_alloc: false,
        used_free: false,
        used_fatal: false,
        used_print_u64: false,
        used_print_char: false,
        used_print_f64: false,
        used_print_str: false,
        used_memcpy: false,
        used_strlen: false,
        used_file_io: false,
        used_panic: false,
        used_print_i64: false,
        used_print_bool: false,
        current_ret: Type::Unit,
    };

    cg.gen_globals(&analysis.globals);

    // C ABI declarations for `extern` functions.
    let mut declares = String::new();
    for f in &program.fns {
        if f.body.is_some() {
            continue;
        }
        if let Some(sig) = analysis.fn_sigs.get(&f.name) {
            declares.push_str(&format!(
                "declare {} @{}({})\n\n",
                llvm_ret(sig, sig.c_abi),
                f.name,
                llvm_params(sig, sig.c_abi)
            ));
        }
    }

    for f in &program.fns {
        cg.gen_fn(f);
    }

    // The synthesized entry point and runtime helpers can register additional
    // string globals, so they must be materialized before the module header,
    // globals, and function bodies are assembled.
    let mut start_def = String::new();
    let mut module_asm = String::new();
    if hosted && tests.is_none() {
        // The C runtime provides `_start` and calls `main`.
    } else if let Some(tests) = tests {
        cg.used_syscall = true;
        let mut body = String::new();
        body.push_str("define void @_start() #0 {\nentry:\n  %failures = alloca i32, align 4\n  store i32 0, ptr %failures, align 4\n");
        for (index, name) in tests.iter().enumerate() {
            let ok_msg = format!("test {name} ... ok\n");
            let fail_msg = format!("test {name} ... FAILED\n");
            let ok_global = cg.add_string(ok_msg.as_bytes());
            let fail_global = cg.add_string(fail_msg.as_bytes());
            body.push_str(&format!(
                "  %r{index} = call i8 @{name}()\n  %ok{index} = icmp ne i8 %r{index}, 0\n  br i1 %ok{index}, label %pass{index}, label %fail{index}\n"
            ));
            body.push_str(&format!("pass{index}:\n"));
            body.push_str(&format!(
                "  %pok{index} = ptrtoint ptr {ok_global} to i64\n  %pw{index} = call i64 @__corex_syscall(i64 1, i64 1, i64 %pok{index}, i64 {}, i64 0, i64 0, i64 0)\n  br label %next{index}\n",
                ok_msg.len()
            ));
            body.push_str(&format!("fail{index}:\n"));
            body.push_str(&format!(
                "  %fp{index} = ptrtoint ptr {fail_global} to i64\n  %fw{index} = call i64 @__corex_syscall(i64 1, i64 1, i64 %fp{index}, i64 {}, i64 0, i64 0, i64 0)\n  %fc{index} = load i32, ptr %failures, align 4\n  %fn{index} = add i32 %fc{index}, 1\n  store i32 %fn{index}, ptr %failures, align 4\n  br label %next{index}\n",
                fail_msg.len()
            ));
            body.push_str(&format!("next{index}:\n"));
        }
        body.push_str(
            "  %fc = load i32, ptr %failures, align 4\n  %bad = icmp ne i32 %fc, 0\n  %code = select i1 %bad, i32 1, i32 0\n  %ext = sext i32 %code to i64\n  %exit = call i64 @__corex_syscall(i64 60, i64 %ext, i64 0, i64 0, i64 0, i64 0, i64 0)\n  unreachable\n}\n\n",
        );
        start_def = body;
    } else if analysis.entry == "main" && !hosted {
        cg.used_syscall = true;
        if analysis.entry_params == 3 {
            for line in [
                ".globl _start",
                "_start:",
                "  movq %rsp, %rdi",
                "  andq $-16, %rsp",
                "  call __corex_entry",
                "  hlt",
            ] {
                module_asm.push_str(&format!("module asm \"{line}\"\n"));
            }
            start_def.push_str(
                "define void @__corex_entry(i64 %sp) #0 {\nentry:\n  %argc_p = inttoptr i64 %sp to ptr\n  %argc = load i32, ptr %argc_p, align 4\n  %argv_off = add i64 %sp, 8\n  %argv = inttoptr i64 %argv_off to ptr\n  %argc64 = sext i32 %argc to i64\n  %env_idx = add i64 %argc64, 2\n  %env_off = mul i64 %env_idx, 8\n  %env_addr = add i64 %sp, %env_off\n  %envp = inttoptr i64 %env_addr to ptr\n  %r = call i32 @main(i32 %argc, ptr %argv, ptr %envp)\n  %ext = sext i32 %r to i64\n  %e = call i64 @__corex_syscall(i64 60, i64 %ext, i64 0, i64 0, i64 0, i64 0, i64 0)\n  unreachable\n}\n\n",
            );
        } else {
            start_def.push_str(
                "define void @_start() #0 {\nentry:\n  %r = call i32 @main()\n  %ext = sext i32 %r to i64\n  %e = call i64 @__corex_syscall(i64 60, i64 %ext, i64 0, i64 0, i64 0, i64 0, i64 0)\n  unreachable\n}\n\n",
            );
        }
    }
    let helpers = cg.helpers();

    let mut module = String::new();
    module.push_str(&format!("; CoreX module: {module_name}\n"));
    module.push_str(&format!("source_filename = \"{module_name}\"\n"));
    module.push_str("target triple = \"x86_64-unknown-linux-gnu\"\n\n");
    module.push_str(&module_asm);
    if !module_asm.is_empty() {
        module.push('\n');
    }
    module.push_str(&declares);
    if sanitize {
        for bits in [8u32, 16, 32, 64] {
            for op in ["sadd", "ssub", "smul", "uadd", "usub", "umul"] {
                module.push_str(&format!(
                    "declare {{i{bits}, i1}} @llvm.{op}.with.overflow.i{bits}(i{bits}, i{bits})\n"
                ));
            }
        }
        module.push('\n');
    }

    for g in &cg.globals {
        module.push_str(g);
        module.push('\n');
    }
    if !cg.globals.is_empty() {
        module.push('\n');
    }

    for f in &cg.fn_defs {
        module.push_str(f);
        module.push('\n');
    }

    module.push_str(&start_def);
    module.push_str(&helpers);
    if let Some(debug) = &mut cg.debug {
        module.push_str(&debug.module_text());
    }
    module.push_str("\nattributes #0 = { alignstack=16 }\n");
    module
}

struct Val {
    ssa: String,
    ty: Type,
}

struct CodeGen<'a> {
    analysis: &'a Analysis,
    /// Instructions of the function currently being generated.
    out: String,
    /// Entry-block instructions (allocas and parameter spills).
    entry: String,
    fn_defs: Vec<String>,
    globals: Vec<String>,
    strings: Vec<(String, Vec<u8>)>,
    tmp: u32,
    lbl: u32,
    slot_ptrs: Vec<String>,
    terminated: bool,
    current_label: String,
    used_syscall: bool,
    used_alloc: bool,
    used_free: bool,
    used_fatal: bool,
    used_print_u64: bool,
    used_print_char: bool,
    used_print_f64: bool,
    used_print_str: bool,
    used_memcpy: bool,
    used_strlen: bool,
    used_file_io: bool,
    used_panic: bool,
    used_print_i64: bool,
    used_print_bool: bool,
    current_ret: Type,
    break_stack: Vec<String>,
    continue_stack: Vec<String>,
    label_map: HashMap<String, String>,
    current_fn: String,
    debug: Option<DebugCtx>,
    sanitize: bool,
    current_scope: u32,
    current_span: Span,
}

impl<'a> CodeGen<'a> {
    fn fresh_val(&mut self) -> String {
        self.tmp += 1;
        format!("%t{}", self.tmp)
    }

    fn fresh_label(&mut self) -> String {
        self.lbl += 1;
        format!("bb{}", self.lbl)
    }

    fn inst(&mut self, line: String) {
        debug_assert!(!self.terminated);
        let line = self.with_debug_location(line);
        self.out.push_str("  ");
        self.out.push_str(&line);
        self.out.push('\n');
    }

    fn terminate(&mut self, line: String) {
        debug_assert!(!self.terminated);
        let line = self.with_debug_location(line);
        self.out.push_str("  ");
        self.out.push_str(&line);
        self.out.push('\n');
        self.terminated = true;
    }

    fn with_debug_location(&mut self, line: String) -> String {
        if line.contains('\n') {
            return line;
        }
        let Some(debug) = &mut self.debug else {
            return line;
        };
        if self.current_span.start == 0 {
            return line;
        }
        let loc = debug.loc(self.current_span, self.current_scope);
        format!("{line}, !dbg !{loc}")
    }

    /// Begin a new basic block, closing the current one with a fallthrough edge.
    fn label(&mut self, name: &str) {
        if !self.terminated {
            self.out.push_str(&format!("  br label %{name}\n"));
        }
        self.out.push_str(&format!("{name}:\n"));
        self.current_label = name.to_string();
        self.terminated = false;
    }

    // ----- module-level helpers ---------------------------------------

    fn add_string(&mut self, bytes: &[u8]) -> String {
        if let Some((name, _)) = self.strings.iter().find(|(_, b)| b == bytes) {
            return name.clone();
        }
        let name = format!("@.str.{}", self.strings.len());
        self.strings.push((name.clone(), bytes.to_vec()));
        let escaped = escape_string(bytes);
        self.globals.push(format!(
            "{name} = private unnamed_addr constant [{} x i8] c\"{escaped}\"",
            bytes.len()
        ));
        name
    }

    fn add_cstr_string(&mut self, bytes: &[u8]) -> String {
        let mut terminated = bytes.to_vec();
        terminated.push(0);
        if let Some((name, _b)) = self.strings.iter().find(|(_, b)| b == &terminated) {
            return name.clone();
        }
        let name = format!("@.cstr.{}", self.strings.len());
        self.strings.push((name.clone(), terminated.clone()));
        let escaped = escape_string(&terminated);
        self.globals.push(format!(
            "{name} = private unnamed_addr constant [{} x i8] c\"{escaped}\"",
            terminated.len()
        ));
        name
    }

    fn fatal(&mut self, message: &str) {
        if self.terminated {
            return;
        }
        let g = self.add_string(message.as_bytes());
        self.used_fatal = true;
        self.terminate(format!(
            "call void @__corex_fatal(ptr {g}, i64 {})\n  unreachable",
            message.len()
        ));
    }

    fn helpers(&mut self) -> String {
        let mut h = String::new();
        // `alloc` aborts through `__corex_fatal`, so it implies both helpers.
        let need_fatal = self.used_fatal || self.used_alloc;
        let need_syscall = self.used_syscall
            || self.used_alloc
            || self.used_free
            || need_fatal
            || self.used_print_u64
            || self.used_print_i64
            || self.used_print_bool
            || self.used_print_char
            || self.used_print_f64
            || self.used_print_str
            || self.used_panic;
        if need_syscall {
            h.push_str(
                "define internal i64 @__corex_syscall(i64 %nr, i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5, i64 %a6) {\nentry:\n  %r = call i64 asm sideeffect \"syscall\", \"={rax},{rax},{rdi},{rsi},{rdx},{r10},{r8},{r9},~{rcx},~{r11},~{memory}\"(i64 %nr, i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5, i64 %a6)\n  ret i64 %r\n}\n\n",
            );
        }
        if need_fatal {
            h.push_str(
                "define internal void @__corex_fatal(ptr %msg, i64 %len) {\nentry:\n  %mp = ptrtoint ptr %msg to i64\n  %w = call i64 @__corex_syscall(i64 1, i64 2, i64 %mp, i64 %len, i64 0, i64 0, i64 0)\n  %x = call i64 @__corex_syscall(i64 60, i64 101, i64 0, i64 0, i64 0, i64 0, i64 0)\n  unreachable\n}\n\n",
            );
        }
        if self.used_alloc {
            let msg = self.add_string(b"corex: allocation failed\n");
            let len = "corex: allocation failed\n".len();
            h.push_str(&format!(
                "define internal ptr @__corex_alloc(i64 %n) {{\nentry:\n  %r = call i64 @__corex_syscall(i64 9, i64 0, i64 %n, i64 3, i64 34, i64 -1, i64 0)\n  %failed = icmp slt i64 %r, 0\n  br i1 %failed, label %fail, label %ok\nfail:\n  call void @__corex_fatal(ptr {msg}, i64 {len})\n  unreachable\nok:\n  %p = inttoptr i64 %r to ptr\n  ret ptr %p\n}}\n\n"
            ));
        }
        if self.used_print_u64 || self.used_print_i64 || self.used_print_f64 {
            h.push_str(
                "define internal void @__corex_put_u64(i64 %value) {\nentry:\n  %buf = alloca [20 x i8], align 1\n  %pos = alloca i64, align 8\n  %cur = alloca i64, align 8\n  store i64 20, ptr %pos, align 8\n  store i64 %value, ptr %cur, align 8\n  br label %loop\nloop:\n  %p0 = load i64, ptr %pos, align 8\n  %p1 = sub i64 %p0, 1\n  store i64 %p1, ptr %pos, align 8\n  %c0 = load i64, ptr %cur, align 8\n  %rem = urem i64 %c0, 10\n  %d0 = add i64 %rem, 48\n  %d1 = trunc i64 %d0 to i8\n  %slot = getelementptr inbounds [20 x i8], ptr %buf, i64 0, i64 %p1\n  store i8 %d1, ptr %slot, align 1\n  %next = udiv i64 %c0, 10\n  store i64 %next, ptr %cur, align 8\n  %done = icmp eq i64 %next, 0\n  br i1 %done, label %emit, label %loop\nemit:\n  %p2 = load i64, ptr %pos, align 8\n  %len = sub i64 20, %p2\n  %base = getelementptr inbounds [20 x i8], ptr %buf, i64 0, i64 %p2\n  %bp = ptrtoint ptr %base to i64\n  %w = call i64 @__corex_syscall(i64 1, i64 1, i64 %bp, i64 %len, i64 0, i64 0, i64 0)\n  ret void\n}\n\n",
            );
            h.push_str(
                "define internal void @__corex_print_u64(i64 %value) {\nentry:\n  call void @__corex_put_u64(i64 %value)\n  call void @__corex_newline()\n  ret void\n}\n\n",
            );
        }
        if self.used_print_f64 {
            let nan = self.add_string(b"nan\n");
            let inf = self.add_string(b"inf\n");
            let ninf = self.add_string(b"-inf\n");
            let minus = self.add_string(b"-");
            let dot = self.add_string(b".");
            let huge = self.add_string(b"corex: float too large to print\n");
            let huge_len = "corex: float too large to print\n".len();
            let mut body = r#"define internal void @__corex_put_pad6(i64 %value) {
entry:
  %buf = alloca [6 x i8], align 1
  %r1 = udiv i64 %value, 10
  %r2 = udiv i64 %r1, 10
  %r3 = udiv i64 %r2, 10
  %r4 = udiv i64 %r3, 10
  %r5 = udiv i64 %r4, 10
  %d0 = urem i64 %value, 10
  %d1 = urem i64 %r1, 10
  %d2 = urem i64 %r2, 10
  %d3 = urem i64 %r3, 10
  %d4 = urem i64 %r4, 10
  %d5 = urem i64 %r5, 10
  %c0 = add i64 %d0, 48
  %c1 = add i64 %d1, 48
  %c2 = add i64 %d2, 48
  %c3 = add i64 %d3, 48
  %c4 = add i64 %d4, 48
  %c5 = add i64 %d5, 48
  %b0 = trunc i64 %c0 to i8
  %b1 = trunc i64 %c1 to i8
  %b2 = trunc i64 %c2 to i8
  %b3 = trunc i64 %c3 to i8
  %b4 = trunc i64 %c4 to i8
  %b5 = trunc i64 %c5 to i8
  %p0 = getelementptr inbounds [6 x i8], ptr %buf, i64 0, i64 0
  %p1 = getelementptr inbounds [6 x i8], ptr %buf, i64 0, i64 1
  %p2 = getelementptr inbounds [6 x i8], ptr %buf, i64 0, i64 2
  %p3 = getelementptr inbounds [6 x i8], ptr %buf, i64 0, i64 3
  %p4 = getelementptr inbounds [6 x i8], ptr %buf, i64 0, i64 4
  %p5 = getelementptr inbounds [6 x i8], ptr %buf, i64 0, i64 5
  store i8 %b5, ptr %p0, align 1
  store i8 %b4, ptr %p1, align 1
  store i8 %b3, ptr %p2, align 1
  store i8 %b2, ptr %p3, align 1
  store i8 %b1, ptr %p4, align 1
  store i8 %b0, ptr %p5, align 1
  %bp = ptrtoint ptr %buf to i64
  %w = call i64 @__corex_syscall(i64 1, i64 1, i64 %bp, i64 6, i64 0, i64 0, i64 0)
  ret void
}

define internal void @__corex_print_f64(double %value) {
entry:
  %isnan = fcmp uno double %value, %value
  br i1 %isnan, label %nan, label %checkinf
nan:
  %np = ptrtoint ptr __NAN__ to i64
  %nw = call i64 @__corex_syscall(i64 1, i64 1, i64 %np, i64 4, i64 0, i64 0, i64 0)
  ret void
checkinf:
  %bits = bitcast double %value to i64
  %absbits = and i64 %bits, 9223372036854775807
  %isinf = icmp eq i64 %absbits, 9218868437227405312
  br i1 %isinf, label %inf, label %finite
inf:
  %neginf = icmp slt i64 %bits, 0
  %ip = select i1 %neginf, ptr __NINF__, ptr __INF__
  %in = select i1 %neginf, i64 5, i64 4
  %ipn = ptrtoint ptr %ip to i64
  %iw = call i64 @__corex_syscall(i64 1, i64 1, i64 %ipn, i64 %in, i64 0, i64 0, i64 0)
  ret void
finite:
  %neg = icmp slt i64 %bits, 0
  br i1 %neg, label %negative, label %positive
negative:
  %mp = ptrtoint ptr __MINUS__ to i64
  %mw = call i64 @__corex_syscall(i64 1, i64 1, i64 %mp, i64 1, i64 0, i64 0, i64 0)
  %magn = fneg double %value
  br label %emit
positive:
  br label %emit
emit:
  %x = phi double [ %magn, %negative ], [ %value, %positive ]
  %toolarge = fcmp oge double %x, 1.0e18
  br i1 %toolarge, label %fail, label %ok
fail:
  call void @__corex_fatal(ptr __HUGE__, i64 __HUGELEN__)
  unreachable
ok:
  %i = fptoui double %x to i64
  %fi = uitofp i64 %i to double
  %frac = fsub double %x, %fi
  %scaled = fmul double %frac, 1000000.0
  %f6 = fptoui double %scaled to i64
  call void @__corex_put_u64(i64 %i)
  %dp = ptrtoint ptr __DOT__ to i64
  %dw = call i64 @__corex_syscall(i64 1, i64 1, i64 %dp, i64 1, i64 0, i64 0, i64 0)
  call void @__corex_put_pad6(i64 %f6)
  call void @__corex_newline()
  ret void
}

"#
            .replace("__NAN__", &nan)
            .replace("__NINF__", &ninf)
            .replace("__INF__", &inf)
            .replace("__MINUS__", &minus)
            .replace("__DOT__", &dot)
            .replace("__HUGE__", &huge)
            .replace("__HUGELEN__", &huge_len.to_string());
            body = body.replace("__INF__", &inf);
            h.push_str(&body);
        }
        if self.used_print_u64
            || self.used_print_i64
            || self.used_print_bool
            || self.used_print_char
            || self.used_print_f64
            || self.used_print_str
        {
            let nl = self.add_string(b"\n");
            h.push_str(&format!(
                "define internal void @__corex_newline() {{\nentry:\n  %np = ptrtoint ptr {nl} to i64\n  %w = call i64 @__corex_syscall(i64 1, i64 1, i64 %np, i64 1, i64 0, i64 0, i64 0)\n  ret void\n}}\n\n"
            ));
        }
        if self.used_print_char {
            h.push_str(
                "define internal void @__corex_print_char(i8 %value) {\nentry:\n  %buf = alloca i8, align 1\n  store i8 %value, ptr %buf, align 1\n  %p = ptrtoint ptr %buf to i64\n  %w = call i64 @__corex_syscall(i64 1, i64 1, i64 %p, i64 1, i64 0, i64 0, i64 0)\n  call void @__corex_newline()\n  ret void\n}\n\n",
            );
        }
        if self.used_print_i64 {
            let minus = self.add_string(b"-");
            h.push_str(&format!(
                "define internal void @__corex_print_i64(i64 %value) {{\nentry:\n  %neg = icmp slt i64 %value, 0\n  br i1 %neg, label %minus, label %plain\nminus:\n  %mp = ptrtoint ptr {minus} to i64\n  %mw = call i64 @__corex_syscall(i64 1, i64 1, i64 %mp, i64 1, i64 0, i64 0, i64 0)\n  %mag = sub i64 0, %value\n  call void @__corex_print_u64(i64 %mag)\n  ret void\nplain:\n  call void @__corex_print_u64(i64 %value)\n  ret void\n}}\n\n"
            ));
        }
        if self.used_print_bool {
            let t = self.add_string(b"true");
            let f = self.add_string(b"false");
            h.push_str(&format!(
                "define internal void @__corex_print_bool(i8 %value) {{\nentry:\n  %t = icmp ne i8 %value, 0\n  %p = select i1 %t, ptr {t}, ptr {f}\n  %n = select i1 %t, i64 4, i64 5\n  %pi = ptrtoint ptr %p to i64\n  %w = call i64 @__corex_syscall(i64 1, i64 1, i64 %pi, i64 %n, i64 0, i64 0, i64 0)\n  call void @__corex_newline()\n  ret void\n}}\n\n"
            ));
        }
        // Weak freestanding implementations of the C memory functions that
        // LLVM's loop-idiom pass may synthesize as library calls. A user
        // definition (strong symbol) overrides these.
        h.push_str(
            "define weak void @memset(ptr %dst, i8 %value, i64 %n) {\nentry:\n  br label %loop\nloop:\n  %i = phi i64 [ 0, %entry ], [ %next, %body ]\n  %cond = icmp ult i64 %i, %n\n  br i1 %cond, label %body, label %done\nbody:\n  %p = getelementptr inbounds i8, ptr %dst, i64 %i\n  store i8 %value, ptr %p, align 1\n  %next = add i64 %i, 1\n  br label %loop\ndone:\n  ret void\n}\n\n",
        );
        h.push_str(
            "define weak ptr @memcpy(ptr %dst, ptr %src, i64 %n) {\nentry:\n  br label %loop\nloop:\n  %i = phi i64 [ 0, %entry ], [ %next, %body ]\n  %cond = icmp ult i64 %i, %n\n  br i1 %cond, label %body, label %done\nbody:\n  %s = getelementptr inbounds i8, ptr %src, i64 %i\n  %b = load i8, ptr %s, align 1\n  %d = getelementptr inbounds i8, ptr %dst, i64 %i\n  store i8 %b, ptr %d, align 1\n  %next = add i64 %i, 1\n  br label %loop\ndone:\n  ret ptr %dst\n}\n\n",
        );
        h.push_str(
            "define weak ptr @memmove(ptr %dst, ptr %src, i64 %n) {\nentry:\n  %di = ptrtoint ptr %dst to i64\n  %si = ptrtoint ptr %src to i64\n  %fwd = icmp ult i64 %di, %si\n  br i1 %fwd, label %forward, label %backward\nforward:\n  br label %floop\nfloop:\n  %fi = phi i64 [ 0, %forward ], [ %fnext, %fbody ]\n  %fc = icmp ult i64 %fi, %n\n  br i1 %fc, label %fbody, label %done\nfbody:\n  %fs = getelementptr inbounds i8, ptr %src, i64 %fi\n  %fb = load i8, ptr %fs, align 1\n  %fd = getelementptr inbounds i8, ptr %dst, i64 %fi\n  store i8 %fb, ptr %fd, align 1\n  %fnext = add i64 %fi, 1\n  br label %floop\nbackward:\n  br label %bloop\nbloop:\n  %bi = phi i64 [ %n, %backward ], [ %bnext, %bcont ]\n  %bc = icmp ugt i64 %bi, 0\n  br i1 %bc, label %bcont, label %done\nbcont:\n  %bnext = sub i64 %bi, 1\n  %bs = getelementptr inbounds i8, ptr %src, i64 %bnext\n  %bb = load i8, ptr %bs, align 1\n  %bd = getelementptr inbounds i8, ptr %dst, i64 %bnext\n  store i8 %bb, ptr %bd, align 1\n  br label %bloop\ndone:\n  ret ptr %dst\n}\n\n",
        );
        h.push_str(
            "define weak i32 @memcmp(ptr %a, ptr %b, i64 %n) {\nentry:\n  br label %loop\nloop:\n  %i = phi i64 [ 0, %entry ], [ %next, %more ]\n  %cond = icmp ult i64 %i, %n\n  br i1 %cond, label %body, label %equal\nbody:\n  %ap = getelementptr inbounds i8, ptr %a, i64 %i\n  %av = load i8, ptr %ap, align 1\n  %bp = getelementptr inbounds i8, ptr %b, i64 %i\n  %bv = load i8, ptr %bp, align 1\n  %same = icmp eq i8 %av, %bv\n  br i1 %same, label %more, label %diff\nmore:\n  %next = add i64 %i, 1\n  br label %loop\ndiff:\n  %ax = zext i8 %av to i32\n  %bx = zext i8 %bv to i32\n  %d = sub i32 %ax, %bx\n  ret i32 %d\nequal:\n  ret i32 0\n}\n\n",
        );
        if self.used_strlen {
            h.push_str(
                "define internal i64 @__corex_strlen(ptr %s) {\nentry:\n  br label %loop\nloop:\n  %i = phi i64 [ 0, %entry ], [ %next, %body ]\n  %p = getelementptr inbounds i8, ptr %s, i64 %i\n  %c = load i8, ptr %p, align 1\n  %stop = icmp eq i8 %c, 0\n  br i1 %stop, label %done, label %body\nbody:\n  %next = add i64 %i, 1\n  br label %loop\ndone:\n  ret i64 %i\n}\n\n",
            );
        }
        if self.used_memcpy {
            h.push_str(
                "define internal void @__corex_memcpy(ptr %dst, ptr %src, i64 %n) {\nentry:\n  br label %loop\nloop:\n  %i = phi i64 [ 0, %entry ], [ %next, %body ]\n  %cond = icmp ult i64 %i, %n\n  br i1 %cond, label %body, label %done\nbody:\n  %sp = getelementptr inbounds i8, ptr %src, i64 %i\n  %b = load i8, ptr %sp, align 1\n  %dp = getelementptr inbounds i8, ptr %dst, i64 %i\n  store i8 %b, ptr %dp, align 1\n  %next = add i64 %i, 1\n  br label %loop\ndone:\n  ret void\n}\n\n",
            );
        }
        if self.used_panic {
            let nl = self.add_string(b"\n");
            h.push_str(&format!(
                "define internal void @__corex_panic(ptr %msg, i64 %len) {{\nentry:\n  %mp = ptrtoint ptr %msg to i64\n  %w = call i64 @__corex_syscall(i64 1, i64 2, i64 %mp, i64 %len, i64 0, i64 0, i64 0)\n  %np = ptrtoint ptr {nl} to i64\n  %nw = call i64 @__corex_syscall(i64 1, i64 2, i64 %np, i64 1, i64 0, i64 0, i64 0)\n  %x = call i64 @__corex_syscall(i64 60, i64 101, i64 0, i64 0, i64 0, i64 0, i64 0)\n  unreachable\n}}\n\n"
            ));
        }
        if self.used_file_io {
            h.push_str(
                r#"define internal ptr @__corex_cstr(ptr %data, i64 %len) {
entry:
  %n = add i64 %len, 1
  %p = call ptr @__corex_alloc(i64 %n)
  call void @__corex_memcpy(ptr %p, ptr %data, i64 %len)
  %end = getelementptr inbounds i8, ptr %p, i64 %len
  store i8 0, ptr %end, align 1
  ret ptr %p
}

define internal i32 @__corex_write_all(i64 %fd, ptr %data, i64 %len) {
entry:
  br label %loop
loop:
  %off = phi i64 [ 0, %entry ], [ %next, %more ]
  %left = sub i64 %len, %off
  %done = icmp eq i64 %left, 0
  br i1 %done, label %finish, label %body
body:
  %p = getelementptr inbounds i8, ptr %data, i64 %off
  %pi = ptrtoint ptr %p to i64
  %w = call i64 @__corex_syscall(i64 1, i64 %fd, i64 %pi, i64 %left, i64 0, i64 0, i64 0)
  %bad = icmp sle i64 %w, 0
  br i1 %bad, label %fail, label %more
more:
  %next = add i64 %off, %w
  br label %loop
finish:
  ret i32 0
fail:
  ret i32 -1
}

define internal { ptr, i64, i64 } @__corex_read_file(ptr %path, i64 %plen) {
entry:
  %c = call ptr @__corex_cstr(ptr %path, i64 %plen)
  %ci = ptrtoint ptr %c to i64
  %fd = call i64 @__corex_syscall(i64 2, i64 %ci, i64 0, i64 0, i64 0, i64 0, i64 0)
  %bad = icmp slt i64 %fd, 0
  br i1 %bad, label %fail, label %ok
fail:
  %clen0 = add i64 %plen, 1
  call void @__corex_free(ptr %c, i64 %clen0)
  ret { ptr, i64, i64 } zeroinitializer
ok:
  %size = call i64 @__corex_syscall(i64 8, i64 %fd, i64 0, i64 2, i64 0, i64 0, i64 0)
  %back = call i64 @__corex_syscall(i64 8, i64 %fd, i64 0, i64 0, i64 0, i64 0, i64 0)
  %empty = icmp sle i64 %size, 0
  br i1 %empty, label %none, label %alloc
none:
  %cl0 = call i64 @__corex_syscall(i64 3, i64 %fd, i64 0, i64 0, i64 0, i64 0, i64 0)
  %clen1 = add i64 %plen, 1
  call void @__corex_free(ptr %c, i64 %clen1)
  ret { ptr, i64, i64 } zeroinitializer
alloc:
  %data = call ptr @__corex_alloc(i64 %size)
  %datapi = ptrtoint ptr %data to i64
  %r = call i64 @__corex_syscall(i64 0, i64 %fd, i64 %datapi, i64 %size, i64 0, i64 0, i64 0)
  %cl = call i64 @__corex_syscall(i64 3, i64 %fd, i64 0, i64 0, i64 0, i64 0, i64 0)
  %clen2 = add i64 %plen, 1
  call void @__corex_free(ptr %c, i64 %clen2)
  %neg = icmp slt i64 %r, 0
  %len = select i1 %neg, i64 0, i64 %r
  %v0 = insertvalue { ptr, i64, i64 } zeroinitializer, ptr %data, 0
  %v1 = insertvalue { ptr, i64, i64 } %v0, i64 %len, 1
  %v2 = insertvalue { ptr, i64, i64 } %v1, i64 %size, 2
  ret { ptr, i64, i64 } %v2
}

define internal i32 @__corex_write_file(ptr %path, i64 %plen, ptr %data, i64 %dlen) {
entry:
  %c = call ptr @__corex_cstr(ptr %path, i64 %plen)
  %ci = ptrtoint ptr %c to i64
  %fd = call i64 @__corex_syscall(i64 2, i64 %ci, i64 577, i64 420, i64 0, i64 0, i64 0)
  %bad = icmp slt i64 %fd, 0
  br i1 %bad, label %fail, label %ok
fail:
  %clen0 = add i64 %plen, 1
  call void @__corex_free(ptr %c, i64 %clen0)
  ret i32 -1
ok:
  %wr = call i32 @__corex_write_all(i64 %fd, ptr %data, i64 %dlen)
  %cl = call i64 @__corex_syscall(i64 3, i64 %fd, i64 0, i64 0, i64 0, i64 0, i64 0)
  %clen1 = add i64 %plen, 1
  call void @__corex_free(ptr %c, i64 %clen1)
  ret i32 %wr
}

"#,
            );
        }
        if self.used_free {
            h.push_str(
                "define internal void @__corex_free(ptr %p, i64 %n) {\nentry:\n  %pi = ptrtoint ptr %p to i64\n  %r = call i64 @__corex_syscall(i64 11, i64 %pi, i64 %n, i64 0, i64 0, i64 0, i64 0)\n  ret void\n}\n\n",
            );
        }
        h
    }

    // ----- functions ---------------------------------------------------

    fn gen_globals(&mut self, globals: &[GlobalDef]) {
        for g in globals {
            if self.analysis.external_globals.contains(&g.name) {
                self.globals.push(format!(
                    "@{} = external global {}, align {}",
                    g.name,
                    g.ty.llvm(),
                    g.ty.align_bytes()
                ));
                continue;
            }
            if let ConstValue::Str(bytes) = &g.value {
                if matches!(g.ty, Type::Str | Type::Slice(_)) {
                    let data = self.add_string(bytes);
                    let linkage = if g.mutable { "global" } else { "constant" };
                    self.globals.push(format!(
                        "@{} = {} {{ ptr, i64 }} {{ ptr {data}, i64 {} }}, align 8",
                        g.name,
                        linkage,
                        bytes.len()
                    ));
                    continue;
                }
            }
            let llvm_ty = g.ty.llvm();
            let value = match &g.value {
                ConstValue::Str(bytes) => {
                    format!("c\"{}\"", escape_string(bytes))
                }
                ConstValue::Bool(b) => if *b { "1" } else { "0" }.to_string(),
                ConstValue::Char(c) => (*c as u64).to_string(),
                ConstValue::GlobalAddr(target) => format!("@{target}"),
                ConstValue::Int(v) if *v == 0 && g.ty.is_pointer_like() => "null".to_string(),
                ConstValue::Int(v) => int_const(&g.ty, *v),
                ConstValue::Float(v) => float_const(&g.ty, *v),
            };
            let linkage = if g.mutable { "global" } else { "constant" };
            self.globals.push(format!(
                "@{} = {} {} {}, align {}",
                g.name,
                linkage,
                llvm_ty,
                value,
                g.ty.align_bytes()
            ));
        }
    }

    fn gen_fn(&mut self, f: &FnDecl) {
        let Some(body) = &f.body else {
            return;
        };
        self.out.clear();
        self.entry.clear();
        self.slot_ptrs.clear();
        self.tmp = 0;
        self.lbl = 0;
        self.terminated = false;
        self.current_label = "entry".to_string();
        self.current_fn = f.name.clone();
        self.current_span = f.span;
        self.current_scope = 0;
        if let Some(debug) = &mut self.debug {
            let (line, _) = debug.line_col(f.span);
            let null_types = debug.add("!{null}".to_string());
            let sub_type = debug.add(format!("!DISubroutineType(types: !{null_types})"));
            let file_id = debug.file_id;
            let unit = debug.unit_id;
            let scope = debug.add(format!(
                "distinct !DISubprogram(name: {:?}, scope: !{file_id}, file: !{file_id}, line: {line}, type: !{sub_type}, scopeLine: {line}, spFlags: DISPFlagDefinition, unit: !{unit})",
                f.name
            ));
            debug.scope = scope;
            self.current_scope = scope;
        }
        self.break_stack.clear();
        self.continue_stack.clear();
        self.label_map.clear();
        collect_goto_labels(body, &mut self.label_map);

        let sig = self
            .analysis
            .fn_sigs
            .get(&f.name)
            .cloned()
            .unwrap_or(FnSig {
                params: f.params.iter().map(|p| p.ty.clone()).collect(),
                ret: f.ret.clone(),
                variadic: f.variadic,
                c_abi: f.extern_abi.is_some(),
                external: f.body.is_none(),
            });
        self.current_ret = sig.ret.clone();

        let layout = self
            .analysis
            .layouts
            .get(&f.name)
            .cloned()
            .unwrap_or_default();

        for (i, ty) in layout.slot_types.iter().enumerate() {
            let (llvm_ty, align) = if *ty == Type::Unit {
                ("i8".to_string(), 1)
            } else {
                (ty.llvm(), ty.align_bytes())
            };
            self.entry.push_str(&format!(
                "  %slot{i} = alloca {llvm_ty}, align {align}\n"
            ));
            self.slot_ptrs.push(format!("%slot{i}"));
        }
        if self.debug.is_some() {
            for (i, ty) in layout.slot_types.iter().enumerate() {
                let name = layout
                    .slot_names
                    .get(i)
                    .cloned()
                    .flatten();
                let Some((name, span)) = name else {
                    continue;
                };
                let debug = self.debug.as_mut().unwrap();
                let (line, _) = debug.line_col(span);
                let ty_id = debug.type_meta(ty);
                let file_id = debug.file_id;
                let scope = self.current_scope;
                let var = debug.add(format!(
                    "!DILocalVariable(name: {name:?}, scope: !{scope}, file: !{file_id}, line: {line}, type: !{ty_id})"
                ));
                let expr = debug.add("!DIExpression()".to_string());
                let loc = debug.loc(span, scope);
                self.entry.push_str(&format!(
                    "  call void @llvm.dbg.declare(metadata ptr %slot{i}, metadata !{var}, metadata !{expr}), !dbg !{loc}\n"
                ));
            }
        }

        let mut params = Vec::new();
        if sig.ret.is_aggregate() && !sig.c_abi {
            params.push("ptr %sret".to_string());
        }
        for (i, ty) in sig.params.iter().enumerate() {
            if ty.is_aggregate() && sig.c_abi {
                params.push(format!("{} %p{i}", ty.llvm()));
                self.entry.push_str(&format!(
                    "  store {} %p{i}, ptr %slot{i}, align {}\n",
                    ty.llvm(),
                    ty.align_bytes()
                ));
            } else if ty.is_aggregate() {
                params.push(format!("ptr %p{i}"));
                let loaded = self.fresh_val();
                self.entry.push_str(&format!(
                    "  {loaded} = load {}, ptr %p{i}, align {}\n",
                    ty.llvm(),
                    ty.align_bytes()
                ));
                self.entry.push_str(&format!(
                    "  store {} {loaded}, ptr %slot{i}, align {}\n",
                    ty.llvm(),
                    ty.align_bytes()
                ));
            } else {
                params.push(format!("{} %p{i}", ty.llvm()));
                self.entry.push_str(&format!(
                    "  store {} %p{i}, ptr %slot{i}, align {}\n",
                    ty.llvm(),
                    ty.align_bytes()
                ));
            }
        }

        let body_terminated = self.gen_block(body);

        if !body_terminated {
            if f.name == "_start" {
                if self.analysis.diverging.contains(&f.name) {
                    // The body always terminates through an `exit` syscall, so
                    // there is no fallthrough to give an exit status to.
                    self.terminate("unreachable".to_string());
                } else {
                    self.used_syscall = true;
                    self.terminate(
                        "call i64 @__corex_syscall(i64 60, i64 0, i64 0, i64 0, i64 0, i64 0, i64 0)\n  unreachable"
                            .to_string(),
                    );
                }
            } else if sig.ret == Type::Unit {
                self.terminate("ret void".to_string());
            } else {
                self.terminate("unreachable".to_string());
            }
        }

        // The kernel enters `_start` with a 16-byte-aligned `%rsp`, while LLVM
        // assumes the SysV callee state (`%rsp % 16 == 8`). Without
        // `alignstack=16`, optimized code can emit misaligned SSE accesses.
        let attrs = if f.name == "_start" { " #0" } else { "" };
        let mut def = String::new();
        let ret_llvm = if sig.ret.is_aggregate() && !sig.c_abi {
            "void".to_string()
        } else {
            sig.ret.llvm()
        };
        let dbg_attr = if self.debug.is_some() {
            format!(" !dbg !{}", self.current_scope)
        } else {
            String::new()
        };
        def.push_str(&format!(
            "define {ret_llvm} @{}({}){attrs} nounwind{dbg_attr} {{\nentry:\n",
            f.name,
            params.join(", ")
        ));
        def.push_str(&self.entry);
        def.push_str(&self.out);
        def.push_str("}\n");
        self.fn_defs.push(def);
    }

    fn gen_block(&mut self, block: &Block) -> bool {
        for stmt in &block.stmts {
            if self.terminated && !matches!(stmt, Stmt::Label { .. }) {
                continue;
            }
            self.gen_stmt(stmt);
        }
        self.terminated
    }

    fn gen_stmt(&mut self, stmt: &Stmt) {
        self.current_span = stmt.span();
        match stmt {
            Stmt::Let { init, ty, .. } => {
                let val = self.gen_expr(init);
                let slot = self
                    .analysis
                    .let_slots
                    .get(&init.id)
                    .copied()
                    .unwrap_or(0);
                let ptr = self
                    .slot_ptrs
                    .get(slot)
                    .cloned()
                    .unwrap_or_else(|| "%slot0".to_string());
                self.store_value(&ptr, &val);
                let _ = ty;
            }
            Stmt::Assign { target, value, .. } => {
                if let Some(field) = self.bitfield_target(target) {
                    let ExprKind::Field { base, name } = &target.kind else {
                        unreachable!()
                    };
                    let (base_ptr, base_ty) = self.gen_place(base);
                    let (ptr, _) = self.gen_field_ptr(&base_ptr, &base_ty, name);
                    let val = self.gen_expr(value);
                    self.store_bitfield(&ptr, &field, &val);
                    return;
                }
                let (ptr, ty) = self.gen_place(target);
                let val = self.gen_expr(value);
                debug_assert_eq!(val.ty, ty);
                if self.analysis.volatile_places.contains(&target.id) {
                    self.store_volatile(&ptr, &val);
                } else {
                    self.store_value(&ptr, &val);
                }
            }
            Stmt::CompoundAssign {
                target,
                op,
                value,
                ..
            } => {
                if let Some(field) = self.bitfield_target(target) {
                    let ExprKind::Field { base, name } = &target.kind else {
                        unreachable!()
                    };
                    let (base_ptr, base_ty) = self.gen_place(base);
                    let (ptr, _) = self.gen_field_ptr(&base_ptr, &base_ty, name);
                    let cur = self.load(&ptr, &field.ty);
                    // Reading a bitfield applies the shift and mask.
                    let read = self.read_bitfield(cur, &field);
                    let rhs = self.gen_expr(value);
                    let result = self.emit_int_arith(*op, &read, &rhs, &field.ty);
                    self.store_bitfield(&ptr, &field, &result);
                    return;
                }
                let (ptr, ty) = self.gen_place(target);
                let cur = if self.analysis.volatile_places.contains(&target.id) {
                    self.load_volatile(&ptr, &ty)
                } else {
                    self.load(&ptr, &ty)
                };
                let rhs = self.gen_expr(value);
                let result = if ty.is_float() {
                    self.emit_float_arith(*op, &cur, &rhs, &ty)
                } else {
                    self.emit_int_arith(*op, &cur, &rhs, &ty)
                };
                if self.analysis.volatile_places.contains(&target.id) {
                    self.store_volatile(&ptr, &result);
                } else {
                    self.store_value(&ptr, &result);
                }
            }
            Stmt::Expr(e) => {
                self.gen_expr(e);
            }
            Stmt::If {
                cond,
                then_block,
                else_branch,
                ..
            } => self.gen_if(cond, then_block, else_branch.as_deref()),
            Stmt::While { cond, body, .. } => self.gen_while(cond, body),
            Stmt::Loop { body, .. } => self.gen_loop(body),
            Stmt::Break(_) => {
                let target = self
                    .break_stack
                    .last()
                    .cloned()
                    .unwrap_or_else(|| "bb0".to_string());
                self.terminate(format!("br label %{target}"));
            }
            Stmt::Continue(_) => {
                let target = self
                    .continue_stack
                    .last()
                    .cloned()
                    .unwrap_or_else(|| "bb0".to_string());
                self.terminate(format!("br label %{target}"));
            }
            Stmt::For {
                init,
                cond,
                step,
                body,
                ..
            } => self.gen_for(init.as_deref(), cond.as_ref(), step.as_deref(), body),
            Stmt::DoWhile { body, cond, .. } => self.gen_do_while(body, cond),
            Stmt::Switch { value, arms, .. } => self.gen_switch(value, arms),
            Stmt::Label { name, stmt, .. } => {
                let label = self
                    .label_map
                    .get(name)
                    .cloned()
                    .unwrap_or_else(|| format!("lbl_{name}"));
                self.label(&label);
                self.gen_stmt(stmt);
            }
            Stmt::Goto { label, .. } => {
                let target = self
                    .label_map
                    .get(label)
                    .cloned()
                    .unwrap_or_else(|| format!("lbl_{label}"));
                self.terminate(format!("br label %{target}"));
            }
            Stmt::Return { value, .. } => match value {
                Some(e) => {
                    let v = self.gen_expr(e);
                    let c_abi = self
                        .analysis
                        .fn_sigs
                        .get(&self.current_fn)
                        .map(|s| s.c_abi)
                        .unwrap_or(false);
                    if self.current_ret.is_aggregate() && c_abi {
                        let loaded = self.fresh_val();
                        self.inst(format!(
                            "{loaded} = load {}, ptr {}, align {}",
                            self.current_ret.llvm(),
                            v.ssa,
                            self.current_ret.align_bytes()
                        ));
                        self.terminate(format!(
                            "ret {} {loaded}",
                            self.current_ret.llvm()
                        ));
                    } else if self.current_ret.is_aggregate() {
                        self.store_value("%sret", &v);
                        self.terminate("ret void".to_string());
                    } else if v.ty == Type::Unit {
                        self.terminate("ret void".to_string());
                    } else {
                        self.terminate(format!("ret {} {}", v.ty.llvm(), v.ssa));
                    }
                }
                None => self.terminate("ret void".to_string()),
            },
            Stmt::Unsafe { body, .. } => {
                self.gen_block(body);
            }
            Stmt::Block(block) => {
                self.gen_block(block);
            }
            Stmt::Empty(_) => {}
        }
    }

    fn gen_if(&mut self, cond: &Expr, then_block: &Block, else_branch: Option<&Stmt>) {
        let c = self.gen_cond(cond);
        let then_l = self.fresh_label();
        let else_l = self.fresh_label();
        let end_l = self.fresh_label();

        self.terminate(format!("br i1 {c}, label %{then_l}, label %{else_l}"));

        self.label(&then_l);
        let then_terminated = self.gen_block(then_block);
        if !then_terminated {
            self.terminate(format!("br label %{end_l}"));
        }

        self.label(&else_l);
        let else_terminated = match else_branch {
            Some(s) => {
                self.gen_stmt(s);
                self.terminated
            }
            None => false,
        };
        if !else_terminated {
            self.terminate(format!("br label %{end_l}"));
        }

        self.label(&end_l);
    }

    fn gen_while(&mut self, cond: &Expr, body: &Block) {
        let cond_l = self.fresh_label();
        let body_l = self.fresh_label();
        let end_l = self.fresh_label();

        self.terminate(format!("br label %{cond_l}"));
        self.label(&cond_l);
        let c = self.gen_cond(cond);
        self.terminate(format!("br i1 {c}, label %{body_l}, label %{end_l}"));

        self.label(&body_l);
        self.break_stack.push(end_l.clone());
        self.continue_stack.push(cond_l.clone());
        let t = self.gen_block(body);
        self.continue_stack.pop();
        self.break_stack.pop();
        if !t {
            self.terminate(format!("br label %{cond_l}"));
        }

        self.label(&end_l);
    }

    fn gen_loop(&mut self, body: &Block) {
        let body_l = self.fresh_label();
        let end_l = self.fresh_label();

        self.terminate(format!("br label %{body_l}"));
        self.label(&body_l);
        self.break_stack.push(end_l.clone());
        self.continue_stack.push(body_l.clone());
        let t = self.gen_block(body);
        self.continue_stack.pop();
        self.break_stack.pop();
        if !t {
            self.terminate(format!("br label %{body_l}"));
        }

        self.label(&end_l);
    }

    fn gen_for(
        &mut self,
        init: Option<&Stmt>,
        cond: Option<&Expr>,
        step: Option<&Stmt>,
        body: &Block,
    ) {
        if let Some(init) = init {
            self.gen_stmt(init);
        }
        let cond_l = self.fresh_label();
        let body_l = self.fresh_label();
        let step_l = self.fresh_label();
        let end_l = self.fresh_label();

        self.terminate(format!("br label %{cond_l}"));
        self.label(&cond_l);
        match cond {
            Some(cond) => {
                let c = self.gen_cond(cond);
                self.terminate(format!("br i1 {c}, label %{body_l}, label %{end_l}"));
            }
            None => self.terminate(format!("br label %{body_l}")),
        }

        self.label(&body_l);
        self.break_stack.push(end_l.clone());
        self.continue_stack.push(step_l.clone());
        let terminated = self.gen_block(body);
        self.continue_stack.pop();
        self.break_stack.pop();
        if !terminated {
            self.terminate(format!("br label %{step_l}"));
        }

        self.label(&step_l);
        if let Some(step) = step {
            self.gen_stmt(step);
        }
        self.terminate(format!("br label %{cond_l}"));

        self.label(&end_l);
    }

    fn gen_do_while(&mut self, body: &Block, cond: &Expr) {
        let body_l = self.fresh_label();
        let cond_l = self.fresh_label();
        let end_l = self.fresh_label();

        self.terminate(format!("br label %{body_l}"));
        self.label(&body_l);
        self.break_stack.push(end_l.clone());
        self.continue_stack.push(cond_l.clone());
        let terminated = self.gen_block(body);
        self.continue_stack.pop();
        self.break_stack.pop();
        if !terminated {
            self.terminate(format!("br label %{cond_l}"));
        }

        self.label(&cond_l);
        let c = self.gen_cond(cond);
        self.terminate(format!("br i1 {c}, label %{body_l}, label %{end_l}"));
        self.label(&end_l);
    }

    fn gen_switch(&mut self, value: &Expr, arms: &[SwitchArm]) {
        let v = self.gen_expr(value);
        let end_l = self.fresh_label();
        let labels: Vec<String> = arms.iter().map(|_| self.fresh_label()).collect();
        let mut default_l = end_l.clone();

        for (index, arm) in arms.iter().enumerate() {
            match &arm.value {
                Some(case_expr) => {
                    let case_v = self.gen_expr(case_expr);
                    let cmp = self.fresh_val();
                    self.inst(format!(
                        "{cmp} = icmp eq {} {}, {}",
                        v.ty.llvm(),
                        v.ssa,
                        case_v.ssa
                    ));
                    let next = self.fresh_label();
                    self.terminate(format!(
                        "br i1 {cmp}, label %{}, label %{next}",
                        labels[index]
                    ));
                    self.label(&next);
                }
                None => default_l = labels[index].clone(),
            }
        }
        self.terminate(format!("br label %{default_l}"));

        for (index, arm) in arms.iter().enumerate() {
            self.label(&labels[index]);
            self.break_stack.push(end_l.clone());
            self.gen_block(&arm.body);
            self.break_stack.pop();
            // C fallthrough: the next arm's block follows directly.
        }
        self.label(&end_l);
    }

    fn gen_cond(&mut self, cond: &Expr) -> String {
        let v = self.gen_expr(cond);
        let t = self.fresh_val();
        self.inst(format!("{t} = icmp ne i8 {}, 0", v.ssa));
        t
    }

    // ----- values ------------------------------------------------------

    fn load(&mut self, ptr: &str, ty: &Type) -> Val {
        if *ty == Type::Unit {
            return Val {
                ssa: String::new(),
                ty: ty.clone(),
            };
        }
        let t = self.fresh_val();
        self.inst(format!(
            "{t} = load {}, ptr {ptr}, align {}",
            ty.llvm(),
            ty.align_bytes()
        ));
        Val {
            ssa: t,
            ty: ty.clone(),
        }
    }

    fn load_volatile(&mut self, ptr: &str, ty: &Type) -> Val {
        let t = self.fresh_val();
        self.inst(format!(
            "{t} = load volatile {}, ptr {ptr}, align {}",
            ty.llvm(),
            ty.align_bytes()
        ));
        Val {
            ssa: t,
            ty: ty.clone(),
        }
    }

    fn store_volatile(&mut self, dest: &str, val: &Val) {
        self.inst(format!(
            "store volatile {} {}, ptr {dest}, align {}",
            val.ty.llvm(),
            val.ssa,
            val.ty.align_bytes()
        ));
    }

    fn store_value(&mut self, dest: &str, val: &Val) {
        match val.ty {
            Type::Unit => {}
            Type::Array { .. }
            | Type::Struct(_)
            | Type::Slice(_)
            | Type::Str
            | Type::Vec(_) => {
                // Aggregate values are represented by a pointer to storage;
                // materialize the value before copying it into `dest`.
                let loaded = self.fresh_val();
                self.inst(format!(
                    "{loaded} = load {}, ptr {}, align {}",
                    val.ty.llvm(),
                    val.ssa,
                    val.ty.align_bytes()
                ));
                self.inst(format!(
                    "store {} {loaded}, ptr {dest}, align {}",
                    val.ty.llvm(),
                    val.ty.align_bytes()
                ));
            }
            _ => self.inst(format!(
                "store {} {}, ptr {dest}, align {}",
                val.ty.llvm(),
                val.ssa,
                val.ty.align_bytes()
            )),
        }
    }

    fn gen_expr(&mut self, e: &Expr) -> Val {
        self.current_span = e.span;
        let ty = self
            .analysis
            .expr_types
            .get(&e.id)
            .cloned()
            .unwrap_or(Type::Int(IntTy::I32));

        match &e.kind {
            ExprKind::Int(v) => Val {
                ssa: int_const(&ty, *v),
                ty,
            },
            ExprKind::Float(v) => Val {
                ssa: float_const(&ty, *v),
                ty,
            },
            ExprKind::Char(c) => Val {
                ssa: int_const(&ty, *c as u64),
                ty,
            },
            ExprKind::EnumVariant { enum_name, variant } => {
                let value = self
                    .analysis
                    .enums
                    .get(enum_name)
                    .and_then(|info| info.variants.iter().find(|(n, _)| n == variant))
                    .map(|(_, v)| *v as i64 as u64)
                    .unwrap_or(0);
                Val {
                    ssa: int_const(&ty, value),
                    ty,
                }
            }
            ExprKind::StructLit { fields, .. } => {
                let ptr = self.fresh_val();
                self.entry.push_str(&format!(
                    "  {ptr} = alloca {}, align {}\n",
                    ty.llvm(),
                    ty.align_bytes()
                ));
                self.inst(format!(
                    "store {} zeroinitializer, ptr {ptr}, align {}",
                    ty.llvm(),
                    ty.align_bytes()
                ));
                for (fname, value) in fields {
                    let v = self.gen_expr(value);
                    self.store_field(&ptr, &ty, fname, &v);
                }
                Val { ssa: ptr, ty }
            }
            ExprKind::Field { base, name } => {
                let base_is_place = matches!(
                    base.kind,
                    ExprKind::Ident(_)
                        | ExprKind::Field { .. }
                        | ExprKind::Index { .. }
                        | ExprKind::Unary { op: UnOp::Deref, .. }
                );
                let (base_ptr, base_ty) = if base_is_place {
                    self.gen_place(base)
                } else {
                    let base_val = self.gen_expr(base);
                    (base_val.ssa, base_val.ty)
                };
                let field = self.field_info(&base_ty, name);
                let (ptr, field_ty) = self.gen_field_ptr(&base_ptr, &base_ty, name);
                if let Some(f) = field {
                    if f.bit_width > 0 {
                        let raw = self.load(&ptr, &f.ty);
                        return self.read_bitfield(raw, &f);
                    }
                }
                if field_ty.is_aggregate() {
                    Val {
                        ssa: ptr,
                        ty: field_ty,
                    }
                } else {
                    self.load(&ptr, &field_ty)
                }
            }
            ExprKind::Bool(b) => Val {
                ssa: if *b { "1" } else { "0" }.to_string(),
                ty: Type::Bool,
            },
            ExprKind::Str(bytes) => {
                if matches!(ty, Type::Array { .. }) {
                    let g = self.add_string(bytes);
                    Val { ssa: g, ty }
                } else {
                    let data = self.add_string(bytes);
                    let ptr = self.fresh_val();
                    self.entry.push_str(&format!(
                        "  {ptr} = alloca {{ ptr, i64 }}, align 8\n"
                    ));
                    let p0 = self.fresh_val();
                    let p1 = self.fresh_val();
                    self.inst(format!(
                        "{p0} = getelementptr inbounds {{ ptr, i64 }}, ptr {ptr}, i64 0, i32 0"
                    ));
                    self.inst(format!("store ptr {data}, ptr {p0}, align 8"));
                    self.inst(format!(
                        "{p1} = getelementptr inbounds {{ ptr, i64 }}, ptr {ptr}, i64 0, i32 1"
                    ));
                    self.inst(format!(
                        "store i64 {}, ptr {p1}, align 8",
                        bytes.len()
                    ));
                    Val { ssa: ptr, ty }
                }
            }
            ExprKind::Path(segments) => {
                let full = segments.join("$");
                if let Some(sig) = self.analysis.fn_sigs.get(&full).cloned() {
                    return Val {
                        ssa: format!("@{full}"),
                        ty: Type::Fn(FnType {
                            params: sig.params,
                            ret: Box::new(sig.ret),
                            variadic: sig.variadic,
                        }),
                    };
                }
                if let Some((cty, cv)) = self.analysis.consts.get(&full).cloned() {
                    return self.const_val(&cty, &cv);
                }
                let n = segments.len();
                let enum_name = segments[..n - 1].join("$");
                if let Some(info) = self.analysis.enums.get(&enum_name) {
                    let value = info
                        .variants
                        .iter()
                        .find(|(v, _)| *v == segments[n - 1])
                        .map(|(_, v)| *v as i64 as u64)
                        .unwrap_or(0);
                    return Val {
                        ssa: int_const(&ty, value),
                        ty,
                    };
                }
                let (ptr, t) = self.gen_place(e);
                if t.is_aggregate() {
                    Val { ssa: ptr, ty: t }
                } else if self.analysis.volatile_places.contains(&e.id) {
                    self.load_volatile(&ptr, &t)
                } else {
                    self.load(&ptr, &t)
                }
            }
            ExprKind::Ident(name) => {
                if ty.is_fn() && self.analysis.idents.get(&e.id).is_none() {
                    return Val {
                        ssa: format!("@{name}"),
                        ty,
                    };
                }
                if self.analysis.idents.get(&e.id).is_none() {
                    if let Some((cty, cv)) = self.analysis.consts.get(name).cloned() {
                        return self.const_val(&cty, &cv);
                    }
                }
                let (ptr, ty) = self.gen_place(e);
                if ty.is_aggregate() {
                    Val { ssa: ptr, ty }
                } else if self.analysis.volatile_places.contains(&e.id) {
                    self.load_volatile(&ptr, &ty)
                } else {
                    self.load(&ptr, &ty)
                }
            }
            ExprKind::Unary { op, expr } => match op {
                UnOp::Neg => {
                    if let ExprKind::Int(v) = expr.kind {
                        return Val {
                            ssa: int_const(&ty, v.wrapping_neg()),
                            ty,
                        };
                    }
                    let v = self.gen_expr(expr);
                    let t = self.fresh_val();
                    if v.ty.is_float() {
                        self.inst(format!("{t} = fneg {} {}", v.ty.llvm(), v.ssa));
                    } else {
                        self.inst(format!(
                            "{t} = sub {} 0, {}",
                            v.ty.llvm(),
                            v.ssa
                        ));
                    }
                    Val {
                        ssa: t,
                        ty: v.ty,
                    }
                }
                UnOp::Not => {
                    let v = self.gen_expr(expr);
                    let t = self.fresh_val();
                    if v.ty == Type::Bool {
                        self.inst(format!("{t} = xor i8 {}, 1", v.ssa));
                    } else {
                        self.inst(format!(
                            "{t} = xor {} {}, -1",
                            v.ty.llvm(),
                            v.ssa
                        ));
                    }
                    Val {
                        ssa: t,
                        ty: v.ty,
                    }
                }
                UnOp::AddrOf | UnOp::AddrOfMut => {
                    if let ExprKind::Ident(name) = &expr.kind {
                        if self
                            .analysis
                            .expr_types
                            .get(&expr.id)
                            .map(|t| t.is_fn())
                            .unwrap_or(false)
                        {
                            return Val {
                                ssa: format!("@{name}"),
                                ty,
                            };
                        }
                    }
                    let (ptr, _) = self.gen_place(expr);
                    if let Some(len) = self.analysis.slice_from_array.get(&e.id).copied() {
                        let tmp = self.fresh_val();
                        self.entry.push_str(&format!(
                            "  {tmp} = alloca {{ ptr, i64 }}, align 8\n"
                        ));
                        let p0 = self.fresh_val();
                        let p1 = self.fresh_val();
                        self.inst(format!(
                            "{p0} = getelementptr inbounds {{ ptr, i64 }}, ptr {tmp}, i64 0, i32 0"
                        ));
                        self.inst(format!("store ptr {ptr}, ptr {p0}, align 8"));
                        self.inst(format!(
                            "{p1} = getelementptr inbounds {{ ptr, i64 }}, ptr {tmp}, i64 0, i32 1"
                        ));
                        self.inst(format!("store i64 {len}, ptr {p1}, align 8"));
                        return Val { ssa: tmp, ty };
                    }
                    Val { ssa: ptr, ty }
                }
                UnOp::Deref => {
                    let v = self.gen_expr(expr);
                    let pointee = ty.clone();
                    if pointee.is_aggregate() {
                        Val {
                            ssa: v.ssa,
                            ty: pointee,
                        }
                    } else {
                        self.load(&v.ssa, &pointee)
                    }
                }
            },
            ExprKind::Binary { op, lhs, rhs } => self.gen_binary(e, *op, lhs, rhs, &ty),
            ExprKind::Call { callee, args } => self.gen_call(e, callee, args, &ty),
            ExprKind::Index { .. } => {
                let (ptr, ty) = self.gen_place(e);
                if ty.is_aggregate() {
                    Val { ssa: ptr, ty }
                } else if self.analysis.volatile_places.contains(&e.id) {
                    self.load_volatile(&ptr, &ty)
                } else {
                    self.load(&ptr, &ty)
                }
            }
            ExprKind::SliceRange { base, start, end } => {
                let (data, len, elem) = if let ExprKind::Str(bytes) = &base.kind {
                    (self.add_string(bytes), bytes.len().to_string(), Type::Int(IntTy::U8))
                } else {
                    let const_base = matches!(
                        &base.kind,
                        ExprKind::Ident(name) if self.analysis.consts.contains_key(name)
                    );
                    let base_is_place = !const_base
                        && matches!(
                            base.kind,
                            ExprKind::Ident(_)
                                | ExprKind::Field { .. }
                                | ExprKind::Index { .. }
                                | ExprKind::Unary { op: UnOp::Deref, .. }
                        );
                    let (base_ptr, base_ty) = if base_is_place {
                        self.gen_place(base)
                    } else {
                        let v = self.gen_expr(base);
                        (v.ssa, v.ty)
                    };
                    match &base_ty {
                        Type::Array { elem, len } => {
                            (base_ptr, len.to_string(), (**elem).clone())
                        }
                        Type::Slice(elem) => {
                            let (d, l) = self.load_slice_parts(&base_ptr);
                            (d, l, (**elem).clone())
                        }
                        Type::Str => {
                            let (d, l) = self.load_slice_parts(&base_ptr);
                            (d, l, Type::Int(IntTy::U8))
                        }
                        other => {
                            let _ = other;
                            (base_ptr, "0".to_string(), Type::Int(IntTy::U8))
                        }
                    }
                };

                let start_v = match start {
                    Some(e) => self.gen_expr(e).ssa,
                    None => "0".to_string(),
                };
                let end_v = match end {
                    Some(e) => self.gen_expr(e).ssa,
                    None => len.clone(),
                };
                let ok1 = self.fresh_val();
                let ok2 = self.fresh_val();
                let ok = self.fresh_val();
                self.inst(format!("{ok1} = icmp ule i64 {start_v}, {end_v}"));
                self.inst(format!("{ok2} = icmp ule i64 {end_v}, {len}"));
                self.inst(format!("{ok} = and i1 {ok1}, {ok2}"));
                let in_l = self.fresh_label();
                let bad_l = self.fresh_label();
                self.terminate(format!("br i1 {ok}, label %{in_l}, label %{bad_l}"));
                self.label(&bad_l);
                self.fatal("corex: slice range out of bounds\n");
                self.label(&in_l);

                let new_len = self.fresh_val();
                let new_data = self.fresh_val();
                self.inst(format!("{new_len} = sub i64 {end_v}, {start_v}"));
                self.inst(format!(
                    "{new_data} = getelementptr inbounds {}, ptr {data}, i64 {start_v}",
                    elem.llvm()
                ));
                let tmp = self.fresh_val();
                self.entry
                    .push_str(&format!("  {tmp} = alloca {{ ptr, i64 }}, align 8\n"));
                let p0 = self.fresh_val();
                let p1 = self.fresh_val();
                self.inst(format!(
                    "{p0} = getelementptr inbounds {{ ptr, i64 }}, ptr {tmp}, i64 0, i32 0"
                ));
                self.inst(format!("store ptr {new_data}, ptr {p0}, align 8"));
                self.inst(format!(
                    "{p1} = getelementptr inbounds {{ ptr, i64 }}, ptr {tmp}, i64 0, i32 1"
                ));
                self.inst(format!("store i64 {new_len}, ptr {p1}, align 8"));
                Val { ssa: tmp, ty }
            }
            ExprKind::Cast { expr, .. } => {
                let v = self.gen_expr(expr);
                let to = self
                    .analysis
                    .cast_types
                    .get(&e.id)
                    .cloned()
                    .unwrap_or_else(|| v.ty.clone());
                self.gen_cast(v, &to)
            }
            ExprKind::ArrayRepeat { value, count } => {
                let ptr = self.fresh_val();
                self.entry.push_str(&format!(
                    "  {ptr} = alloca {}, align {}\n",
                    ty.llvm(),
                    ty.align_bytes()
                ));
                for i in 0..*count {
                    let v = self.gen_expr(value);
                    let ep = self.fresh_val();
                    self.inst(format!(
                        "{ep} = getelementptr inbounds {}, ptr {ptr}, i64 0, i64 {i}",
                        ty.llvm()
                    ));
                    self.store_value(&ep, &v);
                }
                Val { ssa: ptr, ty }
            }
            ExprKind::CStr(bytes) => {
                let g = self.add_cstr_string(bytes);
                Val { ssa: g, ty }
            }
            ExprKind::Null => Val {
                ssa: "null".to_string(),
                ty,
            },
            ExprKind::SizeOf(_) | ExprKind::AlignOf(_) | ExprKind::OffsetOf(_, _) => Val {
                ssa: self
                    .analysis
                    .const_sizes
                    .get(&e.id)
                    .copied()
                    .unwrap_or(0)
                    .to_string(),
                ty,
            },
            ExprKind::CompoundLit { fields, .. } => {
                let resolved = self
                    .analysis
                    .type_operands
                    .get(&e.id)
                    .cloned()
                    .unwrap_or_else(|| ty.clone());
                let ptr = self.fresh_val();
                self.entry.push_str(&format!(
                    "  {ptr} = alloca {}, align {}\n",
                    resolved.llvm(),
                    resolved.align_bytes()
                ));
                self.inst(format!(
                    "store {} zeroinitializer, ptr {ptr}, align {}",
                    resolved.llvm(),
                    resolved.align_bytes()
                ));
                match &resolved {
                    Type::Struct(_) => {
                        for (fname, value) in fields {
                            let v = self.gen_expr(value);
                            self.store_field(&ptr, &resolved, fname, &v);
                        }
                    }
                    Type::Array { elem, .. } => {
                        for (key, value) in fields {
                            let Ok(index) = key.parse::<u64>() else {
                                continue;
                            };
                            let v = self.gen_expr(value);
                            let ep = self.fresh_val();
                            self.inst(format!(
                                "{ep} = getelementptr inbounds {}, ptr {ptr}, i64 0, i64 {index}",
                                resolved.llvm()
                            ));
                            let _ = elem;
                            self.store_value(&ep, &v);
                        }
                    }
                    _ => {}
                }
                Val { ssa: ptr, ty: resolved }
            }
            ExprKind::Array(elems) => {
                let ptr = self.fresh_val();
                self.entry.push_str(&format!(
                    "  {ptr} = alloca {}, align {}\n",
                    ty.llvm(),
                    ty.align_bytes()
                ));
                for (i, el) in elems.iter().enumerate() {
                    let v = self.gen_expr(el);
                    let ep = self.fresh_val();
                    self.inst(format!(
                        "{ep} = getelementptr inbounds {}, ptr {ptr}, i64 0, i64 {i}",
                        ty.llvm()
                    ));
                    self.store_value(&ep, &v);
                }
                Val { ssa: ptr, ty }
            }
        }
    }

    fn emit_float_arith(&mut self, op: BinOp, l: &Val, r: &Val, ty: &Type) -> Val {
        let t = self.fresh_val();
        let instr = match op {
            BinOp::Add => "fadd",
            BinOp::Sub => "fsub",
            BinOp::Mul => "fmul",
            BinOp::Div => "fdiv",
            BinOp::Rem => "frem",
            _ => unreachable!("non-float compound assignment operator"),
        };
        self.inst(format!("{t} = {instr} {} {}, {}", ty.llvm(), l.ssa, r.ssa));
        Val {
            ssa: t,
            ty: ty.clone(),
        }
    }

    /// Integer arithmetic shared by binary expressions and compound assignments.
    fn emit_int_arith(&mut self, op: BinOp, l: &Val, r: &Val, ty: &Type) -> Val {
        if self.sanitize
            && signed_checked_int(ty)
            && matches!(op, BinOp::Add | BinOp::Sub | BinOp::Mul)
        {
            return self.emit_checked_arith(op, l, r, ty);
        }
        let t = self.fresh_val();
        let signed = ty.is_integer() && ty.int_ty().map(|i| i.signed()).unwrap_or(false);
        match op {
            BinOp::Add => self.inst(format!("{t} = add {} {}, {}", ty.llvm(), l.ssa, r.ssa)),
            BinOp::Sub => self.inst(format!("{t} = sub {} {}, {}", ty.llvm(), l.ssa, r.ssa)),
            BinOp::Mul => self.inst(format!("{t} = mul {} {}, {}", ty.llvm(), l.ssa, r.ssa)),
            BinOp::BitAnd => self.inst(format!("{t} = and {} {}, {}", ty.llvm(), l.ssa, r.ssa)),
            BinOp::BitOr => self.inst(format!("{t} = or {} {}, {}", ty.llvm(), l.ssa, r.ssa)),
            BinOp::BitXor => self.inst(format!("{t} = xor {} {}, {}", ty.llvm(), l.ssa, r.ssa)),
            BinOp::Div | BinOp::Rem => {
                self.guard_division(l, r);
                let instr = match (op, signed) {
                    (BinOp::Div, true) => "sdiv",
                    (BinOp::Div, false) => "udiv",
                    (BinOp::Rem, true) => "srem",
                    (BinOp::Rem, false) => "urem",
                    _ => unreachable!(),
                };
                self.inst(format!("{t} = {instr} {} {}, {}", ty.llvm(), l.ssa, r.ssa));
            }
            BinOp::Shl | BinOp::Shr => {
                let bits = ty.int_ty().map(|i| i.bits()).unwrap_or(8);
                let mask = self.fresh_val();
                self.inst(format!("{mask} = and {} {}, {}", ty.llvm(), r.ssa, bits - 1));
                let instr = if op == BinOp::Shl {
                    "shl"
                } else if signed {
                    "ashr"
                } else {
                    "lshr"
                };
                self.inst(format!("{t} = {instr} {} {}, {mask}", ty.llvm(), l.ssa));
            }
            _ => unreachable!("non-arithmetic compound assignment operator"),
        }
        Val {
            ssa: t,
            ty: ty.clone(),
        }
    }

    fn emit_checked_arith(&mut self, op: BinOp, l: &Val, r: &Val, ty: &Type) -> Val {
        let bits = ty.int_ty().map(|t| t.bits()).unwrap_or(64);
        let signed = ty.int_ty().map(|t| t.signed()).unwrap_or(false);
        let intrinsic = match (op, signed) {
            (BinOp::Add, true) => "sadd",
            (BinOp::Sub, true) => "ssub",
            (BinOp::Mul, true) => "smul",
            (BinOp::Add, false) => "uadd",
            (BinOp::Sub, false) => "usub",
            (BinOp::Mul, false) => "umul",
            _ => unreachable!(),
        };
        let pair_ty = format!("{{i{bits}, i1}}");
        let pair = self.fresh_val();
        self.inst(format!(
            "{pair} = call {pair_ty} @llvm.{intrinsic}.with.overflow.i{bits}(i{bits} {}, i{bits} {})",
            l.ssa, r.ssa
        ));
        let result = self.fresh_val();
        self.inst(format!(
            "{result} = extractvalue {pair_ty} {pair}, 0"
        ));
        let overflow = self.fresh_val();
        self.inst(format!(
            "{overflow} = extractvalue {pair_ty} {pair}, 1"
        ));
        let fail_l = self.fresh_label();
        let ok_l = self.fresh_label();
        self.terminate(format!(
            "br i1 {overflow}, label %{fail_l}, label %{ok_l}"
        ));
        self.label(&fail_l);
        self.fatal("corex: integer overflow\n");
        self.label(&ok_l);
        Val {
            ssa: result,
            ty: ty.clone(),
        }
    }

    fn gen_binary(
        &mut self,
        _e: &Expr,
        op: BinOp,
        lhs: &Expr,
        rhs: &Expr,
        ty: &Type,
    ) -> Val {
        match op {
            BinOp::LogicalAnd | BinOp::LogicalOr => {
                return self.gen_logical(op, lhs, rhs);
            }
            _ => {}
        }

        let l = self.gen_expr(lhs);
        let r = self.gen_expr(rhs);
        let t = self.fresh_val();
        // Comparison expressions have type `bool`; use the operand type for
        // the instruction itself.
        let operand_ty = if ty == &Type::Bool && op.is_comparison() {
            l.ty.clone()
        } else {
            ty.clone()
        };
        let ty = &operand_ty;

        if self.sanitize
            && signed_checked_int(ty)
            && matches!(op, BinOp::Add | BinOp::Sub | BinOp::Mul)
        {
            return self.emit_checked_arith(op, &l, &r, ty);
        }

        if ty.is_float() {
            if let BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge = op {
                let pred = match op {
                    BinOp::Eq => "oeq",
                    BinOp::Ne => "une",
                    BinOp::Lt => "olt",
                    BinOp::Le => "ole",
                    BinOp::Gt => "ogt",
                    BinOp::Ge => "oge",
                    _ => unreachable!(),
                };
                let cmp = self.fresh_val();
                self.inst(format!(
                    "{cmp} = fcmp {pred} {} {}, {}",
                    ty.llvm(),
                    l.ssa,
                    r.ssa
                ));
                self.inst(format!("{t} = zext i1 {cmp} to i8"));
                return Val {
                    ssa: t,
                    ty: Type::Bool,
                };
            }
            let instr = match op {
                BinOp::Add => "fadd",
                BinOp::Sub => "fsub",
                BinOp::Mul => "fmul",
                BinOp::Div => "fdiv",
                BinOp::Rem => "frem",
                _ => unreachable!("non-float operator"),
            };
            self.inst(format!("{t} = {instr} {} {}, {}", ty.llvm(), l.ssa, r.ssa));
            return Val {
                ssa: t,
                ty: ty.clone(),
            };
        }

        match op {
            BinOp::Add if ty.is_pointer() => {
                self.inst(format!(
                    "{t} = getelementptr inbounds {}, ptr {}, i64 {}",
                    ty.pointee().map(|p| p.llvm()).unwrap_or_else(|| "i8".to_string()),
                    l.ssa,
                    r.ssa
                ));
            }
            BinOp::Sub if ty.is_pointer() => {
                let neg = self.fresh_val();
                self.inst(format!("{neg} = sub i64 0, {}", r.ssa));
                self.inst(format!(
                    "{t} = getelementptr inbounds {}, ptr {}, i64 {neg}",
                    ty.pointee().map(|p| p.llvm()).unwrap_or_else(|| "i8".to_string()),
                    l.ssa
                ));
            }
            BinOp::Add => self.inst(format!("{t} = add {} {}, {}", ty.llvm(), l.ssa, r.ssa)),
            BinOp::Sub => self.inst(format!("{t} = sub {} {}, {}", ty.llvm(), l.ssa, r.ssa)),
            BinOp::Mul => self.inst(format!("{t} = mul {} {}, {}", ty.llvm(), l.ssa, r.ssa)),
            BinOp::BitAnd => self.inst(format!("{t} = and {} {}, {}", ty.llvm(), l.ssa, r.ssa)),
            BinOp::BitOr => self.inst(format!("{t} = or {} {}, {}", ty.llvm(), l.ssa, r.ssa)),
            BinOp::BitXor => self.inst(format!("{t} = xor {} {}, {}", ty.llvm(), l.ssa, r.ssa)),
            BinOp::Div | BinOp::Rem => {
                self.guard_division(&l, &r);
                let instr = match (op, ty.is_integer() && ty.int_ty().map(|i| i.signed()).unwrap_or(false)) {
                    (BinOp::Div, true) => "sdiv",
                    (BinOp::Div, false) => "udiv",
                    (BinOp::Rem, true) => "srem",
                    (BinOp::Rem, false) => "urem",
                    _ => unreachable!(),
                };
                self.inst(format!("{t} = {instr} {} {}, {}", ty.llvm(), l.ssa, r.ssa));
            }
            BinOp::Shl | BinOp::Shr => {
                let bits = ty.int_ty().map(|i| i.bits()).unwrap_or(64);
                let mask = self.fresh_val();
                self.inst(format!("{mask} = and {} {}, {}", ty.llvm(), r.ssa, bits - 1));
                let instr = if op == BinOp::Shl {
                    "shl"
                } else if ty.int_ty().map(|i| i.signed()).unwrap_or(false) {
                    "ashr"
                } else {
                    "lshr"
                };
                self.inst(format!("{t} = {instr} {} {}, {mask}", ty.llvm(), l.ssa));
            }
            BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
                let signed_cmp = ty.is_enum()
                    || (ty.is_integer() && ty.int_ty().map(|t| t.signed()).unwrap_or(false));
                let pred = match op {
                    BinOp::Eq => "eq",
                    BinOp::Ne => "ne",
                    BinOp::Lt if signed_cmp => "slt",
                    BinOp::Lt => "ult",
                    BinOp::Le if signed_cmp => "sle",
                    BinOp::Le => "ule",
                    BinOp::Gt if signed_cmp => "sgt",
                    BinOp::Gt => "ugt",
                    BinOp::Ge if signed_cmp => "sge",
                    BinOp::Ge => "uge",
                    _ => unreachable!(),
                };
                let c = self.fresh_val();
                self.inst(format!(
                    "{c} = icmp {pred} {} {}, {}",
                    l.ty.llvm(),
                    l.ssa,
                    r.ssa
                ));
                self.inst(format!("{t} = zext i1 {c} to i8"));
                return Val {
                    ssa: t,
                    ty: Type::Bool,
                };
            }
            BinOp::LogicalAnd | BinOp::LogicalOr => unreachable!(),
        }

        Val {
            ssa: t,
            ty: ty.clone(),
        }
    }

    fn gen_logical(&mut self, op: BinOp, lhs: &Expr, rhs: &Expr) -> Val {
        let l = self.gen_expr(lhs);
        let cb = self.fresh_val();
        self.inst(format!("{cb} = icmp ne i8 {}, 0", l.ssa));
        let lhs_block = self.current_label.clone();
        let rhs_l = self.fresh_label();
        let end_l = self.fresh_label();

        if op == BinOp::LogicalAnd {
            self.terminate(format!("br i1 {cb}, label %{rhs_l}, label %{end_l}"));
        } else {
            self.terminate(format!("br i1 {cb}, label %{end_l}, label %{rhs_l}"));
        }

        self.label(&rhs_l);
        let r = self.gen_expr(rhs);
        let rb = self.fresh_val();
        self.inst(format!("{rb} = icmp ne i8 {}, 0", r.ssa));
        let rhs_block = self.current_label.clone();
        self.terminate(format!("br label %{end_l}"));

        self.label(&end_l);
        let p = self.fresh_val();
        let (short, other) = if op == BinOp::LogicalAnd {
            ("false", rb.as_str())
        } else {
            ("true", rb.as_str())
        };
        self.inst(format!(
            "{p} = phi i1 [{short}, %{lhs_block}], [{other}, %{rhs_block}]"
        ));
        let out = self.fresh_val();
        self.inst(format!("{out} = zext i1 {p} to i8"));
        Val {
            ssa: out,
            ty: Type::Bool,
        }
    }

    /// Division by zero and `MIN / -1` are both defined to abort with exit 101.
    fn guard_division(&mut self, l: &Val, r: &Val) {
        let is_signed = l
            .ty
            .int_ty()
            .map(|i| i.signed())
            .unwrap_or(false);
        let ty = l.ty.llvm();
        let zero = self.fresh_val();
        self.inst(format!("{zero} = icmp eq {ty} {}, 0", r.ssa));

        let bad = if is_signed {
            let bits = l.ty.int_ty().unwrap().bits();
            let min = if bits == 64 {
                i64::MIN
            } else {
                -(1i64 << (bits - 1))
            };
            let n1 = self.fresh_val();
            self.inst(format!(
                "{n1} = icmp eq {ty} {}, -1",
                r.ssa
            ));
            let is_min = self.fresh_val();
            self.inst(format!("{is_min} = icmp eq {ty} {}, {min}", l.ssa));
            let overflow = self.fresh_val();
            self.inst(format!("{overflow} = and i1 {is_min}, {n1}"));
            let both = self.fresh_val();
            self.inst(format!("{both} = or i1 {zero}, {overflow}"));
            both
        } else {
            zero
        };

        let fail_l = self.fresh_label();
        let ok_l = self.fresh_label();
        self.terminate(format!("br i1 {bad}, label %{fail_l}, label %{ok_l}"));
        self.label(&fail_l);
        self.fatal("corex: division by zero or signed overflow\n");
        self.label(&ok_l);
    }

    fn gen_call(&mut self, _e: &Expr, callee: &Expr, args: &[Expr], ty: &Type) -> Val {
        let name = match &callee.kind {
            ExprKind::Ident(n) => n.clone(),
            _ => String::new(),
        };
        let kind = self
            .analysis
            .calls
            .get(&_e.id)
            .cloned()
            .unwrap_or(Callee::User(name.clone()));

        match kind {
            Callee::Syscall => {
                self.used_syscall = true;
                let mut vals = Vec::new();
                for a in args {
                    let v = self.gen_expr(a);
                    vals.push(v.ssa);
                }
                while vals.len() < 7 {
                    vals.push("0".to_string());
                }
                let t = self.fresh_val();
                self.inst(format!(
                    "{t} = call i64 @__corex_syscall(i64 {}, i64 {}, i64 {}, i64 {}, i64 {}, i64 {}, i64 {})",
                    vals[0], vals[1], vals[2], vals[3], vals[4], vals[5], vals[6]
                ));
                Val {
                    ssa: t,
                    ty: ty.clone(),
                }
            }
            Callee::Alloc => {
                self.used_alloc = true;
                let n = self.gen_expr(&args[0]);
                let t = self.fresh_val();
                self.inst(format!("{t} = call ptr @__corex_alloc(i64 {})", n.ssa));
                Val {
                    ssa: t,
                    ty: ty.clone(),
                }
            }
            Callee::Print => {
                let v = self.gen_expr(&args[0]);
                match &v.ty {
                    Type::Bool => {
                        self.used_print_bool = true;
                        self.inst(format!("call void @__corex_print_bool(i8 {})", v.ssa));
                    }
                    Type::Int(t) if t.signed() => {
                        self.used_print_i64 = true;
                        self.used_print_u64 = true;
                        let wide = if t.bits() < 64 {
                            let w = self.fresh_val();
                            self.inst(format!(
                                "{w} = sext i{} {} to i64",
                                t.bits(),
                                v.ssa
                            ));
                            w
                        } else {
                            v.ssa.clone()
                        };
                        self.inst(format!("call void @__corex_print_i64(i64 {wide})"));
                    }
                    Type::CStr => {
                        self.used_print_str = true;
                        self.used_strlen = true;
                        let len = self.fresh_val();
                        self.inst(format!(
                            "{len} = call i64 @__corex_strlen(ptr {})",
                            v.ssa
                        ));
                        let ip = self.fresh_val();
                        self.inst(format!("{ip} = ptrtoint ptr {} to i64", v.ssa));
                        self.inst(format!(
                            "call i64 @__corex_syscall(i64 1, i64 1, i64 {ip}, i64 {len}, i64 0, i64 0, i64 0)"
                        ));
                        self.inst("call void @__corex_newline()".to_string());
                    }
                    Type::Str | Type::Slice(_) => {
                        self.used_print_str = true;
                        let p0 = self.fresh_val();
                        let p1 = self.fresh_val();
                        self.inst(format!(
                            "{p0} = getelementptr inbounds {{ ptr, i64 }}, ptr {}, i64 0, i32 0",
                            v.ssa
                        ));
                        let data = self.fresh_val();
                        self.inst(format!("{data} = load ptr, ptr {p0}, align 8"));
                        self.inst(format!(
                            "{p1} = getelementptr inbounds {{ ptr, i64 }}, ptr {}, i64 0, i32 1",
                            v.ssa
                        ));
                        let len = self.fresh_val();
                        self.inst(format!("{len} = load i64, ptr {p1}, align 8"));
                        let ip = self.fresh_val();
                        self.inst(format!("{ip} = ptrtoint ptr {data} to i64"));
                        self.inst(format!(
                            "call i64 @__corex_syscall(i64 1, i64 1, i64 {ip}, i64 {len}, i64 0, i64 0, i64 0)"
                        ));
                        self.inst("call void @__corex_newline()".to_string());
                    }
                    Type::Char => {
                        self.used_print_char = true;
                        self.inst(format!("call void @__corex_print_char(i8 {})", v.ssa));
                    }
                    Type::Float(t) => {
                        self.used_print_f64 = true;
                        self.used_fatal = true;
                        let wide = if *t == FloatTy::F32 {
                            let w = self.fresh_val();
                            self.inst(format!("{w} = fpext float {} to double", v.ssa));
                            w
                        } else {
                            v.ssa.clone()
                        };
                        self.inst(format!("call void @__corex_print_f64(double {wide})"));
                    }
                    Type::Enum(_) => {
                        self.used_print_i64 = true;
                        self.used_print_u64 = true;
                        let w = self.fresh_val();
                        self.inst(format!("{w} = sext i32 {} to i64", v.ssa));
                        self.inst(format!("call void @__corex_print_i64(i64 {w})"));
                    }
                    Type::Int(t) => {
                        self.used_print_u64 = true;
                        let wide = if t.bits() < 64 {
                            let w = self.fresh_val();
                            self.inst(format!(
                                "{w} = zext i{} {} to i64",
                                t.bits(),
                                v.ssa
                            ));
                            w
                        } else {
                            v.ssa.clone()
                        };
                        self.inst(format!("call void @__corex_print_u64(i64 {wide})"));
                    }
                    other => {
                        let _ = other;
                        // Rejected by semantic analysis; keep IR valid.
                    }
                }
                Val {
                    ssa: String::new(),
                    ty: Type::Unit,
                }
            }
            Callee::Asm => {
                let (template, constraints) = self
                    .analysis
                    .asm_specs
                    .get(&_e.id)
                    .cloned()
                    .unwrap_or_default();
                let skip = if matches!(
                    args.get(1).map(|a| &a.kind),
                    Some(ExprKind::Str(_)) | Some(ExprKind::CStr(_))
                ) {
                    2
                } else {
                    1
                };
                let mut arg_strs = Vec::new();
                for arg in args.iter().skip(skip) {
                    let v = self.gen_expr(arg);
                    arg_strs.push(format!("{} {}", v.ty.llvm(), v.ssa));
                }
                let escaped_template = escape_string(template.as_bytes());
                let escaped_constraints = escape_string(constraints.as_bytes());
                if *ty == Type::Unit {
                    self.inst(format!(
                        "call void asm sideeffect \"{escaped_template}\", \"{escaped_constraints}\"({})",
                        arg_strs.join(", ")
                    ));
                    Val {
                        ssa: String::new(),
                        ty: Type::Unit,
                    }
                } else {
                    let t = self.fresh_val();
                    self.inst(format!(
                        "{t} = call {} asm sideeffect \"{escaped_template}\", \"{escaped_constraints}\"({})",
                        ty.llvm(),
                        arg_strs.join(", ")
                    ));
                    Val {
                        ssa: t,
                        ty: ty.clone(),
                    }
                }
            }
            Callee::AtomicLoad
            | Callee::AtomicLoadRelaxed
            | Callee::VolatileLoad => {
                let v = self.gen_expr(&args[0]);
                let pointee = self
                    .analysis
                    .expr_types
                    .get(&args[0].id)
                    .and_then(|t| t.pointee().cloned())
                    .unwrap_or(Type::Int(IntTy::I32));
                let t = self.fresh_val();
                if matches!(kind, Callee::VolatileLoad) {
                    self.inst(format!(
                        "{t} = load volatile {}, ptr {}, align {}",
                        pointee.llvm(),
                        v.ssa,
                        pointee.align_bytes()
                    ));
                } else {
                    let order = if matches!(kind, Callee::AtomicLoadRelaxed) {
                        "monotonic"
                    } else {
                        "seq_cst"
                    };
                    self.inst(format!(
                        "{t} = load atomic {}, ptr {} {order}, align {}",
                        pointee.llvm(),
                        v.ssa,
                        pointee.align_bytes()
                    ));
                }
                Val { ssa: t, ty: pointee }
            }
            Callee::AtomicStore
            | Callee::AtomicStoreRelaxed
            | Callee::VolatileStore => {
                let p = self.gen_expr(&args[0]);
                let value = self.gen_expr(&args[1]);
                let pointee = value.ty.clone();
                if matches!(kind, Callee::VolatileStore) {
                    self.inst(format!(
                        "store volatile {} {}, ptr {}, align {}",
                        pointee.llvm(),
                        value.ssa,
                        p.ssa,
                        pointee.align_bytes()
                    ));
                } else {
                    let order = if matches!(kind, Callee::AtomicStoreRelaxed) {
                        "monotonic"
                    } else {
                        "seq_cst"
                    };
                    self.inst(format!(
                        "store atomic {} {}, ptr {} {order}, align {}",
                        pointee.llvm(),
                        value.ssa,
                        p.ssa,
                        pointee.align_bytes()
                    ));
                }
                Val {
                    ssa: String::new(),
                    ty: Type::Unit,
                }
            }
            Callee::AtomicFetchAdd
            | Callee::AtomicFetchSub
            | Callee::AtomicFetchAnd
            | Callee::AtomicFetchOr
            | Callee::AtomicFetchXor
            | Callee::AtomicXchg => {
                let p = self.gen_expr(&args[0]);
                let value = self.gen_expr(&args[1]);
                let pointee = value.ty.clone();
                let op = match kind {
                    Callee::AtomicFetchAdd => "add",
                    Callee::AtomicFetchSub => "sub",
                    Callee::AtomicFetchAnd => "and",
                    Callee::AtomicFetchOr => "or",
                    Callee::AtomicFetchXor => "xor",
                    Callee::AtomicXchg => "xchg",
                    _ => unreachable!(),
                };
                let t = self.fresh_val();
                self.inst(format!(
                    "{t} = atomicrmw {op} ptr {}, {} {} seq_cst",
                    p.ssa,
                    pointee.llvm(),
                    value.ssa
                ));
                Val { ssa: t, ty: pointee }
            }
            Callee::AtomicCas => {
                let p = self.gen_expr(&args[0]);
                let expected = self.gen_expr(&args[1]);
                let new = self.gen_expr(&args[2]);
                let pointee = expected.ty.clone();
                let pair_ty = format!("{{ {}, i1 }}", pointee.llvm());
                let pair = self.fresh_val();
                self.inst(format!(
                    "{pair} = cmpxchg ptr {}, {} {}, {} {} seq_cst seq_cst",
                    p.ssa,
                    pointee.llvm(),
                    expected.ssa,
                    pointee.llvm(),
                    new.ssa
                ));
                let success = self.fresh_val();
                self.inst(format!("{success} = extractvalue {pair_ty} {pair}, 1"));
                let out = self.fresh_val();
                self.inst(format!("{out} = zext i1 {success} to i8"));
                Val {
                    ssa: out,
                    ty: Type::Bool,
                }
            }
            Callee::AtomicFence => {
                self.inst("fence seq_cst".to_string());
                Val {
                    ssa: String::new(),
                    ty: Type::Unit,
                }
            }
            Callee::DataPtr => {
                let v = self.gen_expr(&args[0]);
                let (data, _len) = self.load_slice_parts(&v.ssa);
                Val {
                    ssa: data,
                    ty: ty.clone(),
                }
            }
            Callee::StrFromRaw => {
                let data = self.gen_expr(&args[0]);
                let len = self.gen_expr(&args[1]);
                let tmp = self.fresh_val();
                self.entry
                    .push_str(&format!("  {tmp} = alloca {{ ptr, i64 }}, align 8\n"));
                let p0 = self.fresh_val();
                let p1 = self.fresh_val();
                self.inst(format!(
                    "{p0} = getelementptr inbounds {{ ptr, i64 }}, ptr {tmp}, i64 0, i32 0"
                ));
                self.inst(format!("store ptr {}, ptr {p0}, align 8", data.ssa));
                self.inst(format!(
                    "{p1} = getelementptr inbounds {{ ptr, i64 }}, ptr {tmp}, i64 0, i32 1"
                ));
                self.inst(format!("store i64 {}, ptr {p1}, align 8", len.ssa));
                Val {
                    ssa: tmp,
                    ty: Type::Str,
                }
            }
            Callee::CStrLen => {
                self.used_strlen = true;
                let v = self.gen_expr(&args[0]);
                let t = self.fresh_val();
                self.inst(format!("{t} = call i64 @__corex_strlen(ptr {})", v.ssa));
                Val {
                    ssa: t,
                    ty: Type::Int(IntTy::Usize),
                }
            }
            Callee::CStrToStr => {
                self.used_strlen = true;
                let v = self.gen_expr(&args[0]);
                let len = self.fresh_val();
                self.inst(format!("{len} = call i64 @__corex_strlen(ptr {})", v.ssa));
                let tmp = self.fresh_val();
                self.entry
                    .push_str(&format!("  {tmp} = alloca {{ ptr, i64 }}, align 8\n"));
                let p0 = self.fresh_val();
                let p1 = self.fresh_val();
                self.inst(format!(
                    "{p0} = getelementptr inbounds {{ ptr, i64 }}, ptr {tmp}, i64 0, i32 0"
                ));
                self.inst(format!("store ptr {}, ptr {p0}, align 8", v.ssa));
                self.inst(format!(
                    "{p1} = getelementptr inbounds {{ ptr, i64 }}, ptr {tmp}, i64 0, i32 1"
                ));
                self.inst(format!("store i64 {len}, ptr {p1}, align 8"));
                Val {
                    ssa: tmp,
                    ty: Type::Str,
                }
            }
            Callee::StrToCStr => {
                self.used_memcpy = true;
                let buf = self.gen_expr(&args[0]);
                let cap = self.gen_expr(&args[1]);
                let s = self.gen_expr(&args[2]);
                let (data, len) = self.load_slice_parts(&s.ssa);
                let need = self.fresh_val();
                let ok = self.fresh_val();
                self.inst(format!("{need} = add i64 {len}, 1"));
                self.inst(format!("{ok} = icmp ule i64 {need}, {}", cap.ssa));
                let good_l = self.fresh_label();
                let bad_l = self.fresh_label();
                self.terminate(format!("br i1 {ok}, label %{good_l}, label %{bad_l}"));
                self.label(&bad_l);
                self.fatal("corex: cstr buffer too small\n");
                self.label(&good_l);
                self.inst(format!(
                    "call void @__corex_memcpy(ptr {}, ptr {data}, i64 {len})",
                    buf.ssa
                ));
                let end = self.fresh_val();
                self.inst(format!(
                    "{end} = getelementptr inbounds i8, ptr {}, i64 {len}",
                    buf.ssa
                ));
                self.inst(format!("store i8 0, ptr {end}, align 1"));
                Val {
                    ssa: buf.ssa,
                    ty: Type::CStr,
                }
            }
            Callee::Len => {
                let v = self.gen_expr(&args[0]);
                match &v.ty {
                    Type::Array { len, .. } => Val {
                        ssa: len.to_string(),
                        ty: Type::Int(IntTy::Usize),
                    },
                    Type::Slice(_) | Type::Str => {
                        let p = self.fresh_val();
                        self.inst(format!(
                            "{p} = getelementptr inbounds {{ ptr, i64 }}, ptr {}, i64 0, i32 1",
                            v.ssa
                        ));
                        let n = self.fresh_val();
                        self.inst(format!("{n} = load i64, ptr {p}, align 8"));
                        Val {
                            ssa: n,
                            ty: Type::Int(IntTy::Usize),
                        }
                    }
                    Type::Vec(_) => {
                        let p = self.vec_field(&v.ssa, 1);
                        let n = self.fresh_val();
                        self.inst(format!("{n} = load i64, ptr {p}, align 8"));
                        Val {
                            ssa: n,
                            ty: Type::Int(IntTy::Usize),
                        }
                    }
                    _ => Val {
                        ssa: "0".to_string(),
                        ty: Type::Int(IntTy::Usize),
                    },
                }
            }
            Callee::Panic => {
                let msg = self.gen_expr(&args[0]);
                let (data, len) = self.load_slice_parts(&msg.ssa);
                self.used_panic = true;
                self.terminate(format!(
                    "call void @__corex_panic(ptr {data}, i64 {len})\n  unreachable"
                ));
                Val {
                    ssa: String::new(),
                    ty: Type::Unit,
                }
            }
            Callee::Assert => {
                let cond = self.gen_expr(&args[0]);
                let msg = self.gen_expr(&args[1]);
                let (data, len) = self.load_slice_parts(&msg.ssa);
                let ok = self.fresh_val();
                self.inst(format!("{ok} = icmp ne i8 {}, 0", cond.ssa));
                let good_l = self.fresh_label();
                let bad_l = self.fresh_label();
                self.terminate(format!(
                    "br i1 {ok}, label %{good_l}, label %{bad_l}"
                ));
                self.label(&bad_l);
                self.used_panic = true;
                self.terminate(format!(
                    "call void @__corex_panic(ptr {data}, i64 {len})\n  unreachable"
                ));
                self.label(&good_l);
                Val {
                    ssa: String::new(),
                    ty: Type::Unit,
                }
            }
            Callee::ReadFile => {
                let path = self.gen_expr(&args[0]);
                let (pdata, plen) = self.load_slice_parts(&path.ssa);
                self.used_file_io = true;
                self.used_alloc = true;
                self.used_free = true;
                self.used_memcpy = true;
                self.used_syscall = true;
                let res = self.fresh_val();
                self.inst(format!(
                    "{res} = call {{ ptr, i64, i64 }} @__corex_read_file(ptr {pdata}, i64 {plen})"
                ));
                let tmp = self.fresh_val();
                self.entry.push_str(&format!(
                    "  {tmp} = alloca {{ ptr, i64, i64 }}, align 8\n"
                ));
                self.inst(format!(
                    "store {{ ptr, i64, i64 }} {res}, ptr {tmp}, align 8"
                ));
                Val {
                    ssa: tmp,
                    ty: ty.clone(),
                }
            }
            Callee::WriteFile => {
                let path = self.gen_expr(&args[0]);
                let data = self.gen_expr(&args[1]);
                let (pdata, plen) = self.load_slice_parts(&path.ssa);
                let (ddata, dlen) = self.load_slice_parts(&data.ssa);
                self.used_file_io = true;
                self.used_alloc = true;
                self.used_free = true;
                self.used_memcpy = true;
                self.used_syscall = true;
                let res = self.fresh_val();
                self.inst(format!(
                    "{res} = call i32 @__corex_write_file(ptr {pdata}, i64 {plen}, ptr {ddata}, i64 {dlen})"
                ));
                Val {
                    ssa: res,
                    ty: Type::Int(IntTy::I32),
                }
            }
            Callee::VecNew => {
                let elem = ty.vec_elem().unwrap_or(Type::Int(IntTy::U8));
                let esize = elem.size_bytes();
                let cap = self.gen_expr(&args[0]);
                let vecptr = self.fresh_val();
                self.entry.push_str(&format!(
                    "  {vecptr} = alloca {{ ptr, i64, i64 }}, align 8\n"
                ));
                let is_zero = self.fresh_val();
                self.inst(format!("{is_zero} = icmp eq i64 {}, 0", cap.ssa));
                let alloc_l = self.fresh_label();
                let empty_l = self.fresh_label();
                let join_l = self.fresh_label();
                self.terminate(format!(
                    "br i1 {is_zero}, label %{empty_l}, label %{alloc_l}"
                ));
                self.label(&alloc_l);
                self.used_alloc = true;
                let bytes = self.fresh_val();
                self.inst(format!("{bytes} = mul i64 {}, {esize}", cap.ssa));
                let data = self.fresh_val();
                self.inst(format!("{data} = call ptr @__corex_alloc(i64 {bytes})"));
                self.terminate(format!("br label %{join_l}"));
                self.label(&empty_l);
                self.terminate(format!("br label %{join_l}"));
                self.label(&join_l);
                let data_phi = self.fresh_val();
                self.inst(format!(
                    "{data_phi} = phi ptr [ {data}, %{alloc_l} ], [ null, %{empty_l} ]"
                ));
                let f0 = self.vec_field(&vecptr, 0);
                self.inst(format!("store ptr {data_phi}, ptr {f0}, align 8"));
                let f1 = self.vec_field(&vecptr, 1);
                self.inst(format!("store i64 0, ptr {f1}, align 8"));
                let f2 = self.vec_field(&vecptr, 2);
                self.inst(format!("store i64 {}, ptr {f2}, align 8", cap.ssa));
                Val {
                    ssa: vecptr,
                    ty: ty.clone(),
                }
            }
            Callee::VecPush => {
                let vecptr = self.gen_expr(&args[0]).ssa;
                let value = self.gen_expr(&args[1]);
                let elem = value.ty.clone();
                let esize = elem.size_bytes();
                let f0 = self.vec_field(&vecptr, 0);
                let f1 = self.vec_field(&vecptr, 1);
                let f2 = self.vec_field(&vecptr, 2);
                let data = self.fresh_val();
                let len = self.fresh_val();
                let cap = self.fresh_val();
                self.inst(format!("{data} = load ptr, ptr {f0}, align 8"));
                self.inst(format!("{len} = load i64, ptr {f1}, align 8"));
                self.inst(format!("{cap} = load i64, ptr {f2}, align 8"));
                let full = self.fresh_val();
                self.inst(format!("{full} = icmp eq i64 {len}, {cap}"));
                let grow_l = self.fresh_label();
                let nogrow_l = self.fresh_label();
                let join_l = self.fresh_label();
                self.terminate(format!(
                    "br i1 {full}, label %{grow_l}, label %{nogrow_l}"
                ));
                self.label(&grow_l);
                self.used_alloc = true;
                self.used_free = true;
                self.used_memcpy = true;
                let dbl = self.fresh_val();
                let isz = self.fresh_val();
                let ncap = self.fresh_val();
                self.inst(format!("{dbl} = mul i64 {cap}, 2"));
                self.inst(format!("{isz} = icmp eq i64 {cap}, 0"));
                self.inst(format!("{ncap} = select i1 {isz}, i64 4, i64 {dbl}"));
                let nbytes = self.fresh_val();
                self.inst(format!("{nbytes} = mul i64 {ncap}, {esize}"));
                let ndata = self.fresh_val();
                self.inst(format!("{ndata} = call ptr @__corex_alloc(i64 {nbytes})"));
                let oldbytes = self.fresh_val();
                self.inst(format!("{oldbytes} = mul i64 {len}, {esize}"));
                self.inst(format!(
                    "call void @__corex_memcpy(ptr {ndata}, ptr {data}, i64 {oldbytes})"
                ));
                let cbytes = self.fresh_val();
                self.inst(format!("{cbytes} = mul i64 {cap}, {esize}"));
                self.inst(format!(
                    "call void @__corex_free(ptr {data}, i64 {cbytes})"
                ));
                self.inst(format!("store ptr {ndata}, ptr {f0}, align 8"));
                self.inst(format!("store i64 {ncap}, ptr {f2}, align 8"));
                self.terminate(format!("br label %{join_l}"));
                self.label(&nogrow_l);
                self.terminate(format!("br label %{join_l}"));
                self.label(&join_l);
                let data2 = self.fresh_val();
                self.inst(format!(
                    "{data2} = phi ptr [ {ndata}, %{grow_l} ], [ {data}, %{nogrow_l} ]"
                ));
                let slot = self.fresh_val();
                self.inst(format!(
                    "{slot} = getelementptr inbounds {}, ptr {data2}, i64 {len}",
                    elem.llvm()
                ));
                self.store_value(&slot, &value);
                let nlen = self.fresh_val();
                self.inst(format!("{nlen} = add i64 {len}, 1"));
                self.inst(format!("store i64 {nlen}, ptr {f1}, align 8"));
                Val {
                    ssa: String::new(),
                    ty: Type::Unit,
                }
            }
            Callee::VecFree => {
                let vecptr = self.gen_expr(&args[0]).ssa;
                let elem = self
                    .analysis
                    .expr_types
                    .get(&args[0].id)
                    .and_then(|t| t.pointee().cloned())
                    .and_then(|t| t.vec_elem())
                    .unwrap_or(Type::Int(IntTy::U8));
                let esize = elem.size_bytes();
                let f0 = self.vec_field(&vecptr, 0);
                let f1 = self.vec_field(&vecptr, 1);
                let f2 = self.vec_field(&vecptr, 2);
                let data = self.fresh_val();
                let cap = self.fresh_val();
                self.inst(format!("{data} = load ptr, ptr {f0}, align 8"));
                self.inst(format!("{cap} = load i64, ptr {f2}, align 8"));
                self.used_free = true;
                // `munmap(null, 0)` is harmless, so no branch is needed.
                let cbytes = self.fresh_val();
                self.inst(format!("{cbytes} = mul i64 {cap}, {esize}"));
                self.inst(format!(
                    "call void @__corex_free(ptr {data}, i64 {cbytes})"
                ));
                self.inst(format!("store ptr null, ptr {f0}, align 8"));
                self.inst(format!("store i64 0, ptr {f1}, align 8"));
                self.inst(format!("store i64 0, ptr {f2}, align 8"));
                Val {
                    ssa: String::new(),
                    ty: Type::Unit,
                }
            }
            Callee::VecSlice => {
                let vecptr = self.gen_expr(&args[0]).ssa;
                let f0 = self.vec_field(&vecptr, 0);
                let f1 = self.vec_field(&vecptr, 1);
                let data = self.fresh_val();
                let len = self.fresh_val();
                self.inst(format!("{data} = load ptr, ptr {f0}, align 8"));
                self.inst(format!("{len} = load i64, ptr {f1}, align 8"));
                let tmp = self.fresh_val();
                self.entry
                    .push_str(&format!("  {tmp} = alloca {{ ptr, i64 }}, align 8\n"));
                let p0 = self.fresh_val();
                let p1 = self.fresh_val();
                self.inst(format!(
                    "{p0} = getelementptr inbounds {{ ptr, i64 }}, ptr {tmp}, i64 0, i32 0"
                ));
                self.inst(format!("store ptr {data}, ptr {p0}, align 8"));
                self.inst(format!(
                    "{p1} = getelementptr inbounds {{ ptr, i64 }}, ptr {tmp}, i64 0, i32 1"
                ));
                self.inst(format!("store i64 {len}, ptr {p1}, align 8"));
                Val {
                    ssa: tmp,
                    ty: ty.clone(),
                }
            }
            Callee::Free => {
                self.used_free = true;
                let p = self.gen_expr(&args[0]);
                let n = self.gen_expr(&args[1]);
                self.inst(format!(
                    "call void @__corex_free(ptr {}, i64 {})",
                    p.ssa, n.ssa
                ));
                Val {
                    ssa: String::new(),
                    ty: Type::Unit,
                }
            }
            Callee::User(fname) => {
                let sig = self.analysis.fn_sigs.get(&fname).cloned();
                let c_abi = sig.as_ref().map(|s| s.c_abi).unwrap_or(false);
                let variadic = sig.as_ref().map(|s| s.variadic).unwrap_or(false);
                let fixed = sig.as_ref().map(|s| s.params.len()).unwrap_or(args.len());
                let ret = sig
                    .as_ref()
                    .map(|s| s.ret.clone())
                    .unwrap_or_else(|| ty.clone());

                let mut arg_strs = Vec::new();
                let mut sret_ptr = None;
                if ret.is_aggregate() && !c_abi {
                    let ptr = self.fresh_val();
                    self.entry.push_str(&format!(
                        "  {ptr} = alloca {}, align {}\n",
                        ret.llvm(),
                        ret.align_bytes()
                    ));
                    arg_strs.push(format!("ptr {ptr}"));
                    sret_ptr = Some(ptr);
                }
                for (index, a) in args.iter().enumerate() {
                    let v = self.gen_expr(a);
                    let v = if index >= fixed && variadic {
                        self.promote_variadic(v)
                    } else {
                        v
                    };
                    if v.ty.is_aggregate() {
                        if c_abi {
                            let loaded = self.fresh_val();
                            self.inst(format!(
                                "{loaded} = load {}, ptr {}, align {}",
                                v.ty.llvm(),
                                v.ssa,
                                v.ty.align_bytes()
                            ));
                            arg_strs.push(format!("{} {loaded}", v.ty.llvm()));
                        } else {
                            arg_strs.push(format!("ptr {}", v.ssa));
                        }
                    } else {
                        arg_strs.push(format!("{} {}", v.ty.llvm(), v.ssa));
                    }
                }
                let call_head = if variadic {
                    let params = sig
                        .as_ref()
                        .map(|s| {
                            let mut parts: Vec<String> =
                                s.params.iter().map(|t| t.llvm()).collect();
                            parts.push("...".to_string());
                            parts.join(", ")
                        })
                        .unwrap_or_default();
                    format!("call {} ({})", ret.llvm(), params)
                } else {
                    format!("call {}", ret.llvm())
                };
                if ret.is_aggregate() && c_abi {
                    let t = self.fresh_val();
                    self.inst(format!(
                        "{t} = {call_head} @{fname}({})",
                        arg_strs.join(", ")
                    ));
                    let ptr = self.fresh_val();
                    self.entry.push_str(&format!(
                        "  {ptr} = alloca {}, align {}\n",
                        ret.llvm(),
                        ret.align_bytes()
                    ));
                    self.inst(format!(
                        "store {} {t}, ptr {ptr}, align {}",
                        ret.llvm(),
                        ret.align_bytes()
                    ));
                    Val { ssa: ptr, ty: ret }
                } else if ret.is_aggregate() {
                    self.inst(format!(
                        "call void @{fname}({})",
                        arg_strs.join(", ")
                    ));
                    Val {
                        ssa: sret_ptr.unwrap_or_default(),
                        ty: ret,
                    }
                } else if ret == Type::Unit {
                    let head = if variadic {
                        call_head.replace("call void", "call void")
                    } else {
                        "call void".to_string()
                    };
                    self.inst(format!(
                        "{head} @{fname}({})",
                        arg_strs.join(", ")
                    ));
                    Val {
                        ssa: String::new(),
                        ty: Type::Unit,
                    }
                } else {
                    let t = self.fresh_val();
                    self.inst(format!(
                        "{t} = {call_head} @{fname}({})",
                        arg_strs.join(", ")
                    ));
                    Val { ssa: t, ty: ret }
                }
            }
            Callee::Indirect => {
                let callee_val = self.gen_expr(callee);
                let sig = self
                    .analysis
                    .expr_types
                    .get(&callee.id)
                    .and_then(|t| t.fn_sig().cloned())
                    .unwrap_or(FnType {
                        params: args
                            .iter()
                            .map(|a| {
                                self.analysis
                                    .expr_types
                                    .get(&a.id)
                                    .cloned()
                                    .unwrap_or(Type::Int(IntTy::I32))
                            })
                            .collect(),
                        ret: Box::new(ty.clone()),
                        variadic: false,
                    });
                let fixed = sig.params.len();
                let mut arg_strs = Vec::new();
                let mut sret_ptr = None;
                if sig.ret.is_aggregate() {
                    let ptr = self.fresh_val();
                    self.entry.push_str(&format!(
                        "  {ptr} = alloca {}, align {}\n",
                        sig.ret.llvm(),
                        sig.ret.align_bytes()
                    ));
                    arg_strs.push(format!("ptr {ptr}"));
                    sret_ptr = Some(ptr);
                }
                for (index, a) in args.iter().enumerate() {
                    let v = self.gen_expr(a);
                    let v = if index >= fixed && sig.variadic {
                        self.promote_variadic(v)
                    } else {
                        v
                    };
                    if v.ty.is_aggregate() {
                        arg_strs.push(format!("ptr {}", v.ssa));
                    } else {
                        arg_strs.push(format!("{} {}", v.ty.llvm(), v.ssa));
                    }
                }
                let mut type_parts: Vec<String> =
                    sig.params.iter().map(|t| t.llvm()).collect();
                if sig.variadic {
                    type_parts.push("...".to_string());
                }
                let ret_ty = if sig.ret.is_aggregate() {
                    "void".to_string()
                } else {
                    sig.ret.llvm()
                };
                let call_head = format!("call {ret_ty} ({})", type_parts.join(", "));
                if sig.ret.is_aggregate() {
                    self.inst(format!(
                        "{call_head} {}({})",
                        callee_val.ssa,
                        arg_strs.join(", ")
                    ));
                    Val {
                        ssa: sret_ptr.unwrap_or_default(),
                        ty: (*sig.ret).clone(),
                    }
                } else if *sig.ret == Type::Unit {
                    self.inst(format!(
                        "{call_head} {}({})",
                        callee_val.ssa,
                        arg_strs.join(", ")
                    ));
                    Val {
                        ssa: String::new(),
                        ty: Type::Unit,
                    }
                } else {
                    let t = self.fresh_val();
                    self.inst(format!(
                        "{t} = {call_head} {}({})",
                        callee_val.ssa,
                        arg_strs.join(", ")
                    ));
                    Val {
                        ssa: t,
                        ty: (*sig.ret).clone(),
                    }
                }
            }
        }
    }

    /// C default argument promotions for values passed through `...`.
    fn promote_variadic(&mut self, v: Val) -> Val {
        match &v.ty {
            Type::Float(FloatTy::F32) => {
                let t = self.fresh_val();
                self.inst(format!("{t} = fpext float {} to double", v.ssa));
                Val {
                    ssa: t,
                    ty: Type::Float(FloatTy::F64),
                }
            }
            Type::Bool => {
                let t = self.fresh_val();
                self.inst(format!("{t} = zext i8 {} to i32", v.ssa));
                Val {
                    ssa: t,
                    ty: Type::Int(IntTy::I32),
                }
            }
            Type::Char => {
                let t = self.fresh_val();
                self.inst(format!("{t} = zext i8 {} to i32", v.ssa));
                Val {
                    ssa: t,
                    ty: Type::Int(IntTy::I32),
                }
            }
            Type::Int(t) if t.bits() < 32 => {
                let bits = t.bits();
                let signed = t.signed();
                let out = self.fresh_val();
                let op = if signed { "sext" } else { "zext" };
                self.inst(format!("{out} = {op} i{bits} {} to i32", v.ssa));
                Val {
                    ssa: out,
                    ty: Type::Int(IntTy::I32),
                }
            }
            _ => v,
        }
    }

    fn gen_cast(&mut self, v: Val, to: &Type) -> Val {
        let from = v.ty.clone();
        if from == Type::Char && *to == Type::Char {
            return Val {
                ssa: v.ssa,
                ty: to.clone(),
            };
        }
        if from == Type::Char {
            if let Some(t) = to.int_ty() {
                if t.bits() == 8 {
                    return Val {
                        ssa: v.ssa,
                        ty: to.clone(),
                    };
                }
                let tmp = self.fresh_val();
                self.inst(format!("{tmp} = zext i8 {} to i{}", v.ssa, t.bits()));
                return Val {
                    ssa: tmp,
                    ty: to.clone(),
                };
            }
        }
        if *to == Type::Char {
            if let Some(t) = from.int_ty() {
                if t.bits() == 8 {
                    return Val {
                        ssa: v.ssa,
                        ty: to.clone(),
                    };
                }
                let tmp = self.fresh_val();
                self.inst(format!("{tmp} = trunc i{} {} to i8", t.bits(), v.ssa));
                return Val {
                    ssa: tmp,
                    ty: to.clone(),
                };
            }
        }

        if from == *to {
            return Val {
                ssa: v.ssa,
                ty: to.clone(),
            };
        }

        // Pointer/fn/C-string representations are all `ptr`.
        if from.is_pointer_like() && to.is_pointer_like() {
            return Val {
                ssa: v.ssa,
                ty: to.clone(),
            };
        }

        if from.is_cstr() && to.is_str() {
            self.used_strlen = true;
            let len = self.fresh_val();
            self.inst(format!("{len} = call i64 @__corex_strlen(ptr {})", v.ssa));
            let tmp = self.fresh_val();
            self.entry
                .push_str(&format!("  {tmp} = alloca {{ ptr, i64 }}, align 8\n"));
            let p0 = self.fresh_val();
            let p1 = self.fresh_val();
            self.inst(format!(
                "{p0} = getelementptr inbounds {{ ptr, i64 }}, ptr {tmp}, i64 0, i32 0"
            ));
            self.inst(format!("store ptr {}, ptr {p0}, align 8", v.ssa));
            self.inst(format!(
                "{p1} = getelementptr inbounds {{ ptr, i64 }}, ptr {tmp}, i64 0, i32 1"
            ));
            self.inst(format!("store i64 {len}, ptr {p1}, align 8"));
            return Val {
                ssa: tmp,
                ty: Type::Str,
            };
        }

        if from.is_float() && to.is_float() {
            let fb = from.float_ty().unwrap().bits();
            let tb = to.float_ty().unwrap().bits();
            if fb == tb {
                return Val {
                    ssa: v.ssa,
                    ty: to.clone(),
                };
            }
            let t = self.fresh_val();
            if tb > fb {
                self.inst(format!("{t} = fpext {} {} to {}", from.llvm(), v.ssa, to.llvm()));
            } else {
                self.inst(format!("{t} = fptrunc {} {} to {}", from.llvm(), v.ssa, to.llvm()));
            }
            return Val {
                ssa: t,
                ty: to.clone(),
            };
        }

        if from.is_integer() && to.is_float() {
            let fb = from.int_ty().unwrap().bits();
            let t = self.fresh_val();
            let op = if from.int_ty().unwrap().signed() {
                "sitofp"
            } else {
                "uitofp"
            };
            self.inst(format!("{t} = {op} i{fb} {} to {}", v.ssa, to.llvm()));
            return Val {
                ssa: t,
                ty: to.clone(),
            };
        }

        if from.is_float() && to.is_integer() {
            let tb = to.int_ty().unwrap().bits();
            let t = self.fresh_val();
            let op = if to.int_ty().unwrap().signed() {
                "fptosi"
            } else {
                "fptoui"
            };
            self.inst(format!("{t} = {op} {} {} to i{tb}", from.llvm(), v.ssa));
            return Val {
                ssa: t,
                ty: to.clone(),
            };
        }

        if from.is_enum() && to.is_integer() {
            let bits = to.int_ty().unwrap().bits();
            if bits == 32 {
                return Val {
                    ssa: v.ssa,
                    ty: to.clone(),
                };
            }
            let t = self.fresh_val();
            if bits < 32 {
                self.inst(format!("{t} = trunc i32 {} to i{bits}", v.ssa));
            } else {
                self.inst(format!("{t} = sext i32 {} to i{bits}", v.ssa));
            }
            return Val {
                ssa: t,
                ty: to.clone(),
            };
        }

        if from.is_pointer_like() && to.is_integer() {
            let i = self.fresh_val();
            self.inst(format!("{i} = ptrtoint ptr {} to i64", v.ssa));
            let target_bits = to.int_ty().unwrap().bits();
            if target_bits == 64 {
                return Val {
                    ssa: i,
                    ty: to.clone(),
                };
            }
            let t = self.fresh_val();
            self.inst(format!("{t} = trunc i64 {i} to i{target_bits}"));
            return Val {
                ssa: t,
                ty: to.clone(),
            };
        }

        if from.is_integer() && to.is_pointer_like() {
            let from_bits = from.int_ty().unwrap().bits();
            let wide = if from_bits < 64 {
                let t = self.fresh_val();
                let op = if from.int_ty().unwrap().signed() {
                    "sext"
                } else {
                    "zext"
                };
                self.inst(format!("{t} = {op} i{from_bits} {} to i64", v.ssa));
                t
            } else {
                v.ssa.clone()
            };
            let t = self.fresh_val();
            self.inst(format!("{t} = inttoptr i64 {wide} to ptr"));
            return Val {
                ssa: t,
                ty: to.clone(),
            };
        }

        if from.is_integer() && to.is_integer() {
            let fb = from.int_ty().unwrap().bits();
            let tb = to.int_ty().unwrap().bits();
            if fb == tb {
                return Val {
                    ssa: v.ssa,
                    ty: to.clone(),
                };
            }
            let t = self.fresh_val();
            if tb < fb {
                self.inst(format!("{t} = trunc i{fb} {} to i{tb}", v.ssa));
            } else {
                let op = if from.int_ty().unwrap().signed() {
                    "sext"
                } else {
                    "zext"
                };
                self.inst(format!("{t} = {op} i{fb} {} to i{tb}", v.ssa));
            }
            return Val {
                ssa: t,
                ty: to.clone(),
            };
        }

        // Unreachable for semantically valid programs; keep the value as-is.
        Val {
            ssa: v.ssa,
            ty: to.clone(),
        }
    }

    fn vec_field(&mut self, base_ptr: &str, index: u32) -> String {
        let p = self.fresh_val();
        self.inst(format!(
            "{p} = getelementptr inbounds {{ ptr, i64, i64 }}, ptr {base_ptr}, i64 0, i32 {index}"
        ));
        p
    }

    fn load_slice_parts(&mut self, base_ptr: &str) -> (String, String) {
        let daddr = self.fresh_val();
        let laddr = self.fresh_val();
        let d = self.fresh_val();
        let l = self.fresh_val();
        self.inst(format!(
            "{daddr} = getelementptr inbounds {{ ptr, i64 }}, ptr {base_ptr}, i64 0, i32 0"
        ));
        self.inst(format!("{d} = load ptr, ptr {daddr}, align 8"));
        self.inst(format!(
            "{laddr} = getelementptr inbounds {{ ptr, i64 }}, ptr {base_ptr}, i64 0, i32 1"
        ));
        self.inst(format!("{l} = load i64, ptr {laddr}, align 8"));
        (d, l)
    }

    fn field_info(&self, base_ty: &Type, name: &str) -> Option<StructField> {
        let info = base_ty.struct_info()?;
        let fields = info.fields.borrow();
        fields.iter().find(|f| f.name == name).cloned()
    }

    fn bitfield_target(&self, e: &Expr) -> Option<StructField> {
        let ExprKind::Field { base, name } = &e.kind else {
            return None;
        };
        let base_ty = self.analysis.expr_types.get(&base.id)?;
        let field = self.field_info(base_ty, name)?;
        if field.bit_width > 0 {
            Some(field)
        } else {
            None
        }
    }

    fn bitfield_bits(ty: &Type) -> u32 {
        match ty {
            Type::Int(t) => t.bits(),
            Type::Enum(_) => 32,
            Type::Bool | Type::Char => 8,
            _ => 64,
        }
    }

    fn read_bitfield(&mut self, raw: Val, field: &StructField) -> Val {
        let ty = field.ty.llvm();
        let mut value = raw.ssa;
        if field.bit_offset > 0 {
            let shifted = self.fresh_val();
            self.inst(format!(
                "{shifted} = lshr {ty} {value}, {}",
                field.bit_offset
            ));
            value = shifted;
        }
        let bits = Self::bitfield_bits(&field.ty);
        if field.bit_width < bits {
            let mask = (1u64 << field.bit_width) - 1;
            let masked = self.fresh_val();
            self.inst(format!(
                "{masked} = and {ty} {value}, {}",
                int_const(&field.ty, mask)
            ));
            value = masked;
        }
        Val {
            ssa: value,
            ty: field.ty.clone(),
        }
    }

    fn store_bitfield(&mut self, ptr: &str, field: &StructField, val: &Val) {
        let ty = field.ty.llvm();
        let bits = Self::bitfield_bits(&field.ty);
        let width = field.bit_width.min(bits);
        let low = if width >= 64 {
            u64::MAX
        } else {
            (1u64 << width) - 1
        };
        let shifted_mask = if field.bit_offset >= 64 {
            0
        } else {
            low.checked_shl(field.bit_offset).unwrap_or(0)
        };
        let raw = self.load(ptr, &field.ty);
        let keep = self.fresh_val();
        self.inst(format!(
            "{keep} = and {ty} {}, {}",
            raw.ssa,
            int_const(&field.ty, !shifted_mask)
        ));
        let masked = self.fresh_val();
        self.inst(format!(
            "{masked} = and {ty} {}, {}",
            val.ssa,
            int_const(&field.ty, low)
        ));
        let placed = if field.bit_offset > 0 {
            let shifted = self.fresh_val();
            self.inst(format!(
                "{shifted} = shl {ty} {masked}, {}",
                field.bit_offset
            ));
            shifted
        } else {
            masked
        };
        let result = self.fresh_val();
        self.inst(format!("{result} = or {ty} {keep}, {placed}"));
        self.store_value(
            ptr,
            &Val {
                ssa: result,
                ty: field.ty.clone(),
            },
        );
    }

    fn store_field(&mut self, base: &str, base_ty: &Type, name: &str, val: &Val) {
        let Some(field) = self.field_info(base_ty, name) else {
            return;
        };
        let (ptr, _) = self.gen_field_ptr(base, base_ty, name);
        if field.bit_width > 0 {
            self.store_bitfield(&ptr, &field, val);
        } else {
            self.store_value(&ptr, val);
        }
    }

    fn gen_field_ptr(&mut self, base_ptr: &str, base_ty: &Type, name: &str) -> (String, Type) {
        let Some(field) = self.field_info(base_ty, name) else {
            return (base_ptr.to_string(), Type::Int(IntTy::I32));
        };
        let ptr = self.fresh_val();
        self.inst(format!(
            "{ptr} = getelementptr inbounds i8, ptr {base_ptr}, i64 {}",
            field.offset
        ));
        (ptr, field.ty)
    }

    fn gen_place(&mut self, e: &Expr) -> (String, Type) {
        let ty = self
            .analysis
            .expr_types
            .get(&e.id)
            .cloned()
            .unwrap_or(Type::Int(IntTy::I32));

        match &e.kind {
            ExprKind::Field { base, name } => {
                let (base_ptr, base_ty) = self.gen_place(base);
                self.gen_field_ptr(&base_ptr, &base_ty, name)
            }
            ExprKind::Path(segments) => (format!("@{}", segments.join("$")), ty),
            ExprKind::Ident(name) => {
                if let Some(local) = self.analysis.idents.get(&e.id) {
                    let ptr = self
                        .slot_ptrs
                        .get(local.slot)
                        .cloned()
                        .unwrap_or_else(|| "%slot0".to_string());
                    (ptr, ty)
                } else {
                    (format!("@{name}"), ty)
                }
            }
            ExprKind::Str(bytes) => {
                let g = self.add_string(bytes);
                (
                    g,
                    Type::array_of(Type::Int(IntTy::U8), bytes.len() as u64),
                )
            }
            ExprKind::Unary {
                op: UnOp::Deref,
                expr,
            } => {
                let v = self.gen_expr(expr);
                (v.ssa, ty)
            }
            ExprKind::Index { base, index } => {
                let (base_ptr, base_ty) = match &base.kind {
                    ExprKind::Ident(name) if self.analysis.consts.contains_key(name) => {
                        let v = self.gen_expr(base);
                        (v.ssa, v.ty)
                    }
                    ExprKind::Ident(_)
                    | ExprKind::Index { .. }
                    | ExprKind::Str(_)
                    | ExprKind::Unary {
                        op: UnOp::Deref, ..
                    } => self.gen_place(base),
                    _ => {
                        let v = self.gen_expr(base);
                        (v.ssa, v.ty)
                    }
                };

                // Arrays keep their base pointer; slices load the data pointer
                // and runtime length from the two-word aggregate.
                let (elem, data_ptr, len_value, is_slice) = match &base_ty {
                    Type::Array { elem, len } => {
                        ((**elem).clone(), base_ptr.clone(), len.to_string(), false)
                    }
                    Type::Slice(elem) => {
                        let data = self.fresh_val();
                        let l = self.fresh_val();
                        let daddr = self.fresh_val();
                        let laddr = self.fresh_val();
                        self.inst(format!(
                            "{daddr} = getelementptr inbounds {{ ptr, i64 }}, ptr {base_ptr}, i64 0, i32 0"
                        ));
                        self.inst(format!("{data} = load ptr, ptr {daddr}, align 8"));
                        self.inst(format!(
                            "{laddr} = getelementptr inbounds {{ ptr, i64 }}, ptr {base_ptr}, i64 0, i32 1"
                        ));
                        self.inst(format!("{l} = load i64, ptr {laddr}, align 8"));
                        ((**elem).clone(), data, l, true)
                    }
                    Type::Vec(elem) => {
                        let data = self.fresh_val();
                        let l = self.fresh_val();
                        let daddr = self.vec_field(&base_ptr, 0);
                        let laddr = self.vec_field(&base_ptr, 1);
                        self.inst(format!("{data} = load ptr, ptr {daddr}, align 8"));
                        self.inst(format!("{l} = load i64, ptr {laddr}, align 8"));
                        ((**elem).clone(), data, l, true)
                    }
                    Type::Str => {
                        // Only reachable through a value (a literal goes through
                        // `gen_place`, which reports its array type).
                        let data = self.fresh_val();
                        let l = self.fresh_val();
                        let daddr = self.fresh_val();
                        let laddr = self.fresh_val();
                        self.inst(format!(
                            "{daddr} = getelementptr inbounds {{ ptr, i64 }}, ptr {base_ptr}, i64 0, i32 0"
                        ));
                        self.inst(format!("{data} = load ptr, ptr {daddr}, align 8"));
                        self.inst(format!(
                            "{laddr} = getelementptr inbounds {{ ptr, i64 }}, ptr {base_ptr}, i64 0, i32 1"
                        ));
                        self.inst(format!("{l} = load i64, ptr {laddr}, align 8"));
                        (Type::Int(IntTy::U8), data, l, true)
                    }
                    _ => (Type::Int(IntTy::U8), base_ptr.clone(), "1".to_string(), false),
                };

                let index_v = self.gen_expr(index);
                let idx = match index.kind {
                    ExprKind::Int(v) if !is_slice && len_value.parse::<u64>().map(|l| v < l).unwrap_or(false) => {
                        v.to_string()
                    }
                    _ => {
                        let ok = self.fresh_val();
                        self.inst(format!(
                            "{ok} = icmp ult i64 {}, {}",
                            index_v.ssa,
                            if is_slice { len_value.clone() } else { len_value.clone() }
                        ));
                        let in_l = self.fresh_label();
                        let bad_l = self.fresh_label();
                        self.terminate(format!(
                            "br i1 {ok}, label %{in_l}, label %{bad_l}"
                        ));
                        self.label(&bad_l);
                        self.fatal("corex: index out of bounds\n");
                        self.label(&in_l);
                        index_v.ssa.clone()
                    }
                };

                let ep = self.fresh_val();
                if is_slice {
                    self.inst(format!(
                        "{ep} = getelementptr inbounds {}, ptr {data_ptr}, i64 {idx}",
                        elem.llvm()
                    ));
                } else {
                    self.inst(format!(
                        "{ep} = getelementptr inbounds {}, ptr {base_ptr}, i64 0, i64 {idx}",
                        base_ty.llvm()
                    ));
                }
                (ep, elem)
            }
            _ => {
                let v = self.gen_expr(e);
                (v.ssa, v.ty)
            }
        }
    }
}

impl<'a> CodeGen<'a> {
    fn const_val(&mut self, ty: &Type, value: &ConstValue) -> Val {
        match value {
            ConstValue::Int(v) => Val {
                ssa: int_const(ty, *v),
                ty: ty.clone(),
            },
            ConstValue::Bool(b) => Val {
                ssa: if *b { "1" } else { "0" }.to_string(),
                ty: Type::Bool,
            },
            ConstValue::Char(c) => Val {
                ssa: int_const(ty, *c as u64),
                ty: Type::Char,
            },
            ConstValue::Float(v) => Val {
                ssa: float_const(ty, *v),
                ty: ty.clone(),
            },
            ConstValue::GlobalAddr(name) => Val {
                ssa: format!("@{name}"),
                ty: ty.clone(),
            },
            ConstValue::Str(bytes) => {
                let data = self.add_string(bytes);
                let ptr = self.fresh_val();
                self.entry
                    .push_str(&format!("  {ptr} = alloca {{ ptr, i64 }}, align 8\n"));
                let p0 = self.fresh_val();
                let p1 = self.fresh_val();
                self.inst(format!(
                    "{p0} = getelementptr inbounds {{ ptr, i64 }}, ptr {ptr}, i64 0, i32 0"
                ));
                self.inst(format!("store ptr {data}, ptr {p0}, align 8"));
                self.inst(format!(
                    "{p1} = getelementptr inbounds {{ ptr, i64 }}, ptr {ptr}, i64 0, i32 1"
                ));
                self.inst(format!("store i64 {}, ptr {p1}, align 8", bytes.len()));
                Val {
                    ssa: ptr,
                    ty: Type::Str,
                }
            }
        }
    }
}

fn collect_goto_labels(block: &Block, map: &mut HashMap<String, String>) {
    for stmt in &block.stmts {
        collect_goto_labels_stmt(stmt, map);
    }
}

fn collect_goto_labels_stmt(stmt: &Stmt, map: &mut HashMap<String, String>) {
    match stmt {
        Stmt::Label { name, stmt, .. } => {
            map.entry(name.clone()).or_insert_with(|| format!("lbl_{name}"));
            collect_goto_labels_stmt(stmt, map);
        }
        Stmt::If {
            then_block,
            else_branch,
            ..
        } => {
            collect_goto_labels(then_block, map);
            if let Some(branch) = else_branch {
                collect_goto_labels_stmt(branch, map);
            }
        }
        Stmt::While { body, .. }
        | Stmt::Loop { body, .. }
        | Stmt::For { body, .. }
        | Stmt::DoWhile { body, .. }
        | Stmt::Unsafe { body, .. }
        | Stmt::Block(body) => collect_goto_labels(body, map),
        Stmt::Switch { arms, .. } => {
            for arm in arms {
                collect_goto_labels(&arm.body, map);
            }
        }
        _ => {}
    }
}

/// Overflow checks apply to the signed integer types used for arithmetic;
/// unsigned and pointer-sized types keep their defined wrapping behavior.
fn signed_checked_int(ty: &Type) -> bool {
    matches!(
        ty,
        Type::Int(IntTy::I8 | IntTy::I16 | IntTy::I32 | IntTy::I64)
    )
}

fn llvm_params(sig: &FnSig, c_abi: bool) -> String {
    let mut parts: Vec<String> = sig
        .params
        .iter()
        .map(|t| {
            if t.is_aggregate() && !c_abi {
                "ptr".to_string()
            } else {
                t.llvm()
            }
        })
        .collect();
    if sig.variadic {
        parts.push("...".to_string());
    }
    parts.join(", ")
}

fn llvm_ret(sig: &FnSig, c_abi: bool) -> String {
    if sig.ret.is_aggregate() && !c_abi {
        "void".to_string()
    } else {
        sig.ret.llvm()
    }
}

/// Format a floating-point value as an LLVM hexadecimal FP constant.
fn float_const(ty: &Type, v: f64) -> String {
    // LLVM hexadecimal FP literals are always 64-bit double bit patterns,
    // even for `float` values (they are checked to be representable).
    let value = match ty {
        Type::Float(FloatTy::F32) => v as f32 as f64,
        _ => v,
    };
    format!("0x{:016X}", value.to_bits())
}

/// Format an unsigned bit pattern as a valid LLVM integer constant of `ty`.
fn int_const(ty: &Type, v: u64) -> String {
    let bits = match ty {
        Type::Char => 8,
        Type::Enum(_) => 32,
        _ => ty.int_ty().map(|t| t.bits()).unwrap_or(64),
    };
    // Mask to the target width first: `-7` for `i32` arrives here as
    // `u64::MAX - 6`, and LLVM's parser silently truncates out-of-range
    // constants, which would produce the wrong value instead of an error.
    let masked = if bits < 64 {
        v & ((1u64 << bits) - 1)
    } else {
        v
    };
    if bits >= 64 {
        (masked as i64).to_string()
    } else {
        let half = 1u64 << (bits - 1);
        if masked >= half {
            ((masked as i128) - (1i128 << bits)).to_string()
        } else {
            masked.to_string()
        }
    }
}

/// LLVM string-literal escaping: printable ASCII except `"` and `\` stay
/// literal, everything else becomes a two-digit hex escape.
fn escape_string(bytes: &[u8]) -> String {
    let mut out = String::new();
    for &b in bytes {
        match b {
            b'"' => out.push_str("\\22"),
            b'\\' => out.push_str("\\5C"),
            0x20..=0x7e => out.push(b as char),
            other => out.push_str(&format!("\\{other:02X}")),
        }
    }
    out
}
