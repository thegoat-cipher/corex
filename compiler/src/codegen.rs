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
use crate::semantic::{Analysis, Callee};

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
        used_print_i64: false,
        used_print_bool: false,
    };

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
    used_print_i64: bool,
    used_print_bool: bool,
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
            || self.used_print_bool;
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
        if self.used_print_u64 {
            let nl = self.add_string(b"\n");
            h.push_str(&format!(
                "define internal void @__corex_print_u64(i64 %value) {{
entry:
  %buf = alloca [20 x i8], align 1
  %pos = alloca i64, align 8
  %cur = alloca i64, align 8
  store i64 20, ptr %pos, align 8
  store i64 %value, ptr %cur, align 8
  br label %loop
loop:
  %p0 = load i64, ptr %pos, align 8
  %p1 = sub i64 %p0, 1
  store i64 %p1, ptr %pos, align 8
  %c0 = load i64, ptr %cur, align 8
  %rem = urem i64 %c0, 10
  %d0 = add i64 %rem, 48
  %d1 = trunc i64 %d0 to i8
  %slot = getelementptr inbounds [20 x i8], ptr %buf, i64 0, i64 %p1
  store i8 %d1, ptr %slot, align 1
  %next = udiv i64 %c0, 10
  store i64 %next, ptr %cur, align 8
  %done = icmp eq i64 %next, 0
  br i1 %done, label %emit, label %loop
emit:
  %p2 = load i64, ptr %pos, align 8
  %len = sub i64 20, %p2
  %base = getelementptr inbounds [20 x i8], ptr %buf, i64 0, i64 %p2
  %bp = ptrtoint ptr %base to i64
  %w = call i64 @__corex_syscall(i64 1, i64 1, i64 %bp, i64 %len, i64 0, i64 0, i64 0)
  call void @__corex_newline()
  ret void
}}

define internal void @__corex_newline() {{
entry:
  %np = ptrtoint ptr {nl} to i64
  %w = call i64 @__corex_syscall(i64 1, i64 1, i64 %np, i64 1, i64 0, i64 0, i64 0)
  ret void
}}

"
            ));
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
        if self.used_free {
            h.push_str(
                "define internal void @__corex_free(ptr %p, i64 %n) {\nentry:\n  %pi = ptrtoint ptr %p to i64\n  %r = call i64 @__corex_syscall(i64 11, i64 %pi, i64 %n, i64 0, i64 0, i64 0, i64 0)\n  ret void\n}\n\n",
            );
        }
        h
    }

    // ----- functions ---------------------------------------------------

    fn gen_fn(&mut self, f: &FnDecl) {
        self.out.clear();
        self.entry.clear();
        self.slot_ptrs.clear();
        self.tmp = 0;
        self.lbl = 0;
        self.terminated = false;
        self.current_label = "entry".to_string();
        self.loop_stack.clear();

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
        for (i, p) in f.params.iter().enumerate() {
            params.push(format!("{} %p{i}", p.ty.llvm()));
            self.entry.push_str(&format!(
                "  store {} %p{i}, ptr %slot{i}, align {}\n",
                p.ty.llvm(),
                p.ty.align_bytes()
            ));
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
            } else if f.ret == Type::Unit {
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
        def.push_str(&format!(
            "define {} @{}({}){attrs} {{\nentry:\n",
            f.ret.llvm(),
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
                debug_assert_eq!(&val.ty, ty, "let type mismatch between AST and codegen");
            }
            Stmt::Assign { target, value, .. } => {
                let (ptr, ty) = self.gen_place(target);
                let val = self.gen_expr(value);
                debug_assert_eq!(val.ty, ty);
                self.store_value(&ptr, &val);
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
                    if v.ty == Type::Unit {
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
            Type::Array { .. } => {
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

    fn to_arg(&mut self, val: &Val) -> String {
        val.ssa.clone()
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
            ExprKind::Bool(b) => Val {
                ssa: if *b { "1" } else { "0" }.to_string(),
                ty: Type::Bool,
            },
            ExprKind::Str(bytes) => {
                let g = self.add_string(bytes);
                Val {
                    ssa: g,
                    ty: Type::array_of(Type::Int(IntTy::U8), bytes.len() as u64),
                }
            }
            ExprKind::Ident(_) => {
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
                    self.inst(format!(
                        "{t} = sub {} 0, {}",
                        v.ty.llvm(),
                        v.ssa
                    ));
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
            ExprKind::Cast { expr, ty: to } => {
                let v = self.gen_expr(expr);
                self.gen_cast(v, to)
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
                let pred = match op {
                    BinOp::Eq => "eq",
                    BinOp::Ne => "ne",
                    BinOp::Lt => {
                        if ty.is_integer() && ty.int_ty().unwrap().signed() {
                            "slt"
                        } else {
                            "ult"
                        }
                    }
                    BinOp::Le => {
                        if ty.is_integer() && ty.int_ty().unwrap().signed() {
                            "sle"
                        } else {
                            "ule"
                        }
                    }
                    BinOp::Gt => {
                        if ty.is_integer() && ty.int_ty().unwrap().signed() {
                            "sgt"
                        } else {
                            "ugt"
                        }
                    }
                    BinOp::Ge => {
                        if ty.is_integer() && ty.int_ty().unwrap().signed() {
                            "sge"
                        } else {
                            "uge"
                        }
                    }
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
                let mut arg_strs = Vec::new();
                for a in args {
                    let v = self.gen_expr(a);
                    arg_strs.push(format!("{} {}", v.ty.llvm(), self.to_arg(&v)));
                }
                if *ty == Type::Unit {
                    self.inst(format!("call void @{fname}({})", arg_strs.join(", ")));
                    Val {
                        ssa: String::new(),
                        ty: Type::Unit,
                    }
                } else {
                    let t = self.fresh_val();
                    self.inst(format!(
                        "{t} = call {} @{fname}({})",
                        ty.llvm(),
                        arg_strs.join(", ")
                    ));
                    Val {
                        ssa: t,
                        ty: ty.clone(),
                    }
                }
            }
        }
    }

    fn gen_cast(&mut self, v: Val, to: &Type) -> Val {
        let from = v.ty.clone();
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

    fn gen_place(&mut self, e: &Expr) -> (String, Type) {
        let ty = self
            .analysis
            .expr_types
            .get(&e.id)
            .cloned()
            .unwrap_or(Type::Int(IntTy::I32));

        match &e.kind {
            ExprKind::Ident(_) => {
                let slot = self
                    .analysis
                    .idents
                    .get(&e.id)
                    .map(|l| l.slot)
                    .unwrap_or(0);
                let ptr = self
                    .slot_ptrs
                    .get(slot)
                    .cloned()
                    .unwrap_or_else(|| "%slot0".to_string());
                (ptr, ty)
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

                let (elem, len) = match &base_ty {
                    Type::Array { elem, len } => ((**elem).clone(), *len),
                    _ => (Type::Int(IntTy::U8), 1),
                };

                let index_v = self.gen_expr(index);
                let idx = match index.kind {
                    ExprKind::Int(v) => {
                        if v >= len {
                            self.fatal("corex: index out of bounds\n");
                        }
                        v.to_string()
                    }
                    _ => {
                        let ok = self.fresh_val();
                        self.inst(format!("{ok} = icmp ult i64 {}, {len}", index_v.ssa));
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
                self.inst(format!(
                    "{ep} = getelementptr inbounds {}, ptr {base_ptr}, i64 0, i64 {idx}",
                    base_ty.llvm()
                ));
                (ep, elem)
            }
            _ => {
                let v = self.gen_expr(e);
                (v.ssa, v.ty)
            }
        }
    }
}

/// Format an unsigned bit pattern as a valid LLVM integer constant of `ty`.
fn int_const(ty: &Type, v: u64) -> String {
    let bits = ty.int_ty().map(|t| t.bits()).unwrap_or(64);
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
