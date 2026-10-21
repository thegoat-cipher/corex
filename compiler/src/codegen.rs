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

use crate::ast::*;
use crate::semantic::{Analysis, Callee, FnSig, GlobalDef};

pub fn generate(program: &Program, analysis: &Analysis, module_name: &str) -> String {
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
        loop_stack: Vec::new(),
        used_syscall: false,
        used_alloc: false,
        used_free: false,
        used_fatal: false,
        used_print_u64: false,
        used_print_char: false,
        used_print_f64: false,
        used_print_str: false,
        used_memcpy: false,
        used_file_io: false,
        used_panic: false,
        used_print_i64: false,
        used_print_bool: false,
        current_ret: Type::Unit,
    };

    cg.gen_globals(&analysis.globals);

    for f in &program.fns {
        cg.gen_fn(f);
    }

    // The synthesized entry point and runtime helpers can register additional
    // string globals, so they must be materialized before the module header,
    // globals, and function bodies are assembled.
    let mut start_def = String::new();
    if analysis.entry == "main" {
        cg.used_syscall = true;
        start_def.push_str(
            "define void @_start() #0 {\nentry:\n  %r = call i32 @main()\n  %ext = sext i32 %r to i64\n  %e = call i64 @__corex_syscall(i64 60, i64 %ext, i64 0, i64 0, i64 0, i64 0, i64 0)\n  unreachable\n}\n\n",
        );
    }
    let helpers = cg.helpers();

    let mut module = String::new();
    module.push_str(&format!("; CoreX module: {module_name}\n"));
    module.push_str(&format!("source_filename = \"{module_name}\"\n"));
    module.push_str("target triple = \"x86_64-unknown-linux-gnu\"\n\n");

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
    loop_stack: Vec<(String, String)>,
    used_syscall: bool,
    used_alloc: bool,
    used_free: bool,
    used_fatal: bool,
    used_print_u64: bool,
    used_print_char: bool,
    used_print_f64: bool,
    used_print_str: bool,
    used_memcpy: bool,
    used_file_io: bool,
    used_panic: bool,
    used_print_i64: bool,
    used_print_bool: bool,
    current_ret: Type,
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
        self.out.push_str("  ");
        self.out.push_str(&line);
        self.out.push('\n');
    }

    fn terminate(&mut self, line: String) {
        debug_assert!(!self.terminated);
        self.out.push_str("  ");
        self.out.push_str(&line);
        self.out.push('\n');
        self.terminated = true;
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
        self.out.clear();
        self.entry.clear();
        self.slot_ptrs.clear();
        self.tmp = 0;
        self.lbl = 0;
        self.terminated = false;
        self.current_label = "entry".to_string();
        self.loop_stack.clear();

        let sig = self
            .analysis
            .fn_sigs
            .get(&f.name)
            .cloned()
            .unwrap_or(FnSig {
                params: f.params.iter().map(|p| p.ty.clone()).collect(),
                ret: f.ret.clone(),
            });
        self.current_ret = sig.ret.clone();

        let layout = self
            .analysis
            .layouts
            .get(&f.name)
            .cloned()
            .unwrap_or_default();

        for (i, ty) in layout.slot_types.iter().enumerate() {
            self.entry.push_str(&format!(
                "  %slot{i} = alloca {}, align {}\n",
                ty.llvm(),
                ty.align_bytes()
            ));
            self.slot_ptrs.push(format!("%slot{i}"));
        }

        let mut params = Vec::new();
        if sig.ret.is_aggregate() {
            params.push("ptr %sret".to_string());
        }
        for (i, ty) in sig.params.iter().enumerate() {
            if ty.is_aggregate() {
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

        let body_terminated = self.gen_block(&f.body);

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
        let ret_llvm = if sig.ret.is_aggregate() {
            "void".to_string()
        } else {
            sig.ret.llvm()
        };
        def.push_str(&format!(
            "define {ret_llvm} @{}({}){attrs} {{\nentry:\n",
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
            if self.terminated {
                break;
            }
            self.gen_stmt(stmt);
        }
        self.terminated
    }

    fn gen_stmt(&mut self, stmt: &Stmt) {
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
                let (ptr, ty) = self.gen_place(target);
                let val = self.gen_expr(value);
                debug_assert_eq!(val.ty, ty);
                self.store_value(&ptr, &val);
            }
            Stmt::CompoundAssign {
                target,
                op,
                value,
                ..
            } => {
                let (ptr, ty) = self.gen_place(target);
                let cur = self.load(&ptr, &ty);
                let rhs = self.gen_expr(value);
                let result = if ty.is_float() {
                    self.emit_float_arith(*op, &cur, &rhs, &ty)
                } else {
                    self.emit_int_arith(*op, &cur, &rhs, &ty)
                };
                self.store_value(&ptr, &result);
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
                    .loop_stack
                    .last()
                    .map(|(b, _)| b.clone())
                    .unwrap_or_else(|| "bb0".to_string());
                self.terminate(format!("br label %{target}"));
            }
            Stmt::Continue(_) => {
                let target = self
                    .loop_stack
                    .last()
                    .map(|(_, c)| c.clone())
                    .unwrap_or_else(|| "bb0".to_string());
                self.terminate(format!("br label %{target}"));
            }
            Stmt::Return { value, .. } => match value {
                Some(e) => {
                    let v = self.gen_expr(e);
                    if self.current_ret.is_aggregate() {
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
        self.loop_stack.push((end_l.clone(), cond_l.clone()));
        let t = self.gen_block(body);
        self.loop_stack.pop();
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
        self.loop_stack.push((end_l.clone(), body_l.clone()));
        let t = self.gen_block(body);
        self.loop_stack.pop();
        if !t {
            self.terminate(format!("br label %{body_l}"));
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
                for (fname, value) in fields {
                    let (field_ptr, field_ty) = self.gen_field_ptr(&ptr, &ty, fname);
                    let v = self.gen_expr(value);
                    self.store_value(&field_ptr, &v);
                    let _ = field_ty;
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
                let (ptr, field_ty) = if base_is_place {
                    self.gen_place(e)
                } else {
                    let base_val = self.gen_expr(base);
                    let (ptr, field_ty) =
                        self.gen_field_ptr(&base_val.ssa, &base_val.ty, name);
                    (ptr, field_ty)
                };
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
            ExprKind::Ident(name) => {
                if self.analysis.idents.get(&e.id).is_none() {
                    if let Some((cty, cv)) = self.analysis.consts.get(name).cloned() {
                        return self.const_val(&cty, &cv);
                    }
                }
                let (ptr, ty) = self.gen_place(e);
                if ty.is_aggregate() {
                    Val { ssa: ptr, ty }
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
                let ret = sig.as_ref().map(|s| s.ret.clone()).unwrap_or_else(|| ty.clone());
                let mut arg_strs = Vec::new();
                let mut sret_ptr = None;
                if ret.is_aggregate() {
                    let ptr = self.fresh_val();
                    self.entry.push_str(&format!(
                        "  {ptr} = alloca {}, align {}\n",
                        ret.llvm(),
                        ret.align_bytes()
                    ));
                    arg_strs.push(format!("ptr {ptr}"));
                    sret_ptr = Some(ptr);
                }
                for a in args {
                    let v = self.gen_expr(a);
                    if v.ty.is_aggregate() {
                        arg_strs.push(format!("ptr {}", v.ssa));
                    } else {
                        arg_strs.push(format!("{} {}", v.ty.llvm(), v.ssa));
                    }
                }
                if ret.is_aggregate() {
                    self.inst(format!("call void @{fname}({})", arg_strs.join(", ")));
                    Val {
                        ssa: sret_ptr.unwrap_or_default(),
                        ty: ret,
                    }
                } else if ret == Type::Unit {
                    self.inst(format!("call void @{fname}({})", arg_strs.join(", ")));
                    Val {
                        ssa: String::new(),
                        ty: Type::Unit,
                    }
                } else {
                    let t = self.fresh_val();
                    self.inst(format!(
                        "{t} = call {} @{fname}({})",
                        ret.llvm(),
                        arg_strs.join(", ")
                    ));
                    Val {
                        ssa: t,
                        ty: ret,
                    }
                }
            }
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

        // Pointer <-> pointer: representation is identical.
        if from.is_pointer() && to.is_pointer() {
            return Val {
                ssa: v.ssa,
                ty: to.clone(),
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

        if from.is_pointer() && to.is_integer() {
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

        if from.is_integer() && to.is_pointer() {
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

    fn gen_field_ptr(&mut self, base_ptr: &str, base_ty: &Type, name: &str) -> (String, Type) {
        let Some(info) = base_ty.struct_info().cloned() else {
            return (base_ptr.to_string(), Type::Int(IntTy::I32));
        };
        let fields = info.fields.borrow();
        let Some((index, field)) = fields
            .iter()
            .enumerate()
            .find(|(_, f)| f.name == name)
        else {
            drop(fields);
            return (base_ptr.to_string(), Type::Int(IntTy::I32));
        };
        let field_ty = field.ty.clone();
        drop(fields);
        let ptr = self.fresh_val();
        self.inst(format!(
            "{ptr} = getelementptr inbounds {}, ptr {base_ptr}, i64 0, i32 {index}",
            base_ty.llvm()
        ));
        (ptr, field_ty)
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
