//! Invalid-program rejection tests. Every case must fail semantic analysis with
//! a specific diagnostic code, before any IR is generated.

use corex::diagnostics::SourceFile;
use corex::driver;

fn codes(src: &str) -> Vec<&'static str> {
    let file = SourceFile::new("case.cx", src);
    match driver::check(&file) {
        Ok(_) => panic!("expected rejection, but analysis succeeded:\n{src}"),
        Err(diags) => diags.iter().map(|d| d.code).collect(),
    }
}

fn assert_rejected(src: &str, code: &str) {
    let found = codes(src);
    assert!(
        found.contains(&code),
        "expected {code}, got {found:?} for:\n{src}"
    );
}

#[test]
fn rejects_syntax_errors() {
    assert_rejected("fn main() -> i32 { return 0 }", "E0100");
}

#[test]
fn rejects_unknown_names() {
    assert_rejected("fn main() -> i32 { return missing; }", "E0201");
}

#[test]
fn rejects_duplicate_definitions() {
    assert_rejected("fn f() {} fn f() {} fn main() -> i32 { return 0; }", "E0202");
    assert_rejected(
        "fn main() -> i32 { let x: i32 = 1; let x: i32 = 2; return x; }",
        "E0202",
    );
}

#[test]
fn rejects_type_mismatches_without_implicit_conversion() {
    assert_rejected(
        "fn main() -> i32 { let a: i32 = 1; let b: u8 = 2; return a + b; }",
        "E0203",
    );
    assert_rejected("fn main() -> i32 { let x: bool = 1; return 0; }", "E0203");
    assert_rejected(
        "fn main() -> i32 { let a: [u8; 2] = [1, 2, 3]; return 0; }",
        "E0203",
    );
}

#[test]
fn rejects_wrong_argument_count() {
    assert_rejected(
        "fn f(a: i32) -> i32 { return a; } fn main() -> i32 { return f(1, 2); }",
        "E0204",
    );
}

#[test]
fn rejects_immutable_assignment() {
    assert_rejected(
        "fn main() -> i32 { let x: i32 = 1; x = 2; return x; }",
        "E0205",
    );
}

#[test]
fn rejects_missing_return_and_return_of_wrong_type() {
    assert_rejected(
        "fn f(b: bool) -> i32 { if b { return 1; } } fn main() -> i32 { return 0; }",
        "E0206",
    );
    assert_rejected(
        "fn f() -> i32 { return true; } fn main() -> i32 { return 0; }",
        "E0203",
    );
    assert_rejected(
        "fn f() -> i32 { return; } fn main() -> i32 { return 0; }",
        "E0206",
    );
}

#[test]
fn rejects_break_and_continue_outside_loops() {
    assert_rejected("fn main() -> i32 { break; }", "E0207");
    assert_rejected("fn main() -> i32 { continue; }", "E0208");
}

#[test]
fn rejects_unsafe_operations_outside_unsafe() {
    assert_rejected(
        "fn f(p: *mut u8) -> u8 { return *p; } fn main() -> i32 { return 0; }",
        "E0209",
    );
    assert_rejected(
        "fn f(p: *mut u8) -> *mut u8 { return p + 1; } fn main() -> i32 { return 0; }",
        "E0209",
    );
    assert_rejected(
        "fn main() -> i32 { syscall(60, 0, 0); return 0; }",
        "E0209",
    );
    assert_rejected(
        "fn main() -> i32 { let p: *mut u8 = alloc(8); return 0; }",
        "E0209",
    );
    assert_rejected(
        "fn main() -> i32 { let p: *mut u8 = 0 as *mut u8; return 0; }",
        "E0209",
    );
}

#[test]
fn rejects_invalid_casts() {
    assert_rejected("fn main() -> i32 { let x: bool = true; return x as i32; }", "E0210");
}

#[test]
fn rejects_address_of_non_place() {
    assert_rejected("fn main() -> i32 { let p: *i32 = &(1 + 2); return 0; }", "E0211");
}

#[test]
fn rejects_missing_entry_point_and_bad_signature() {
    assert_rejected("fn f() {}", "E0212");
    assert_rejected("fn main() { }", "E0217");
    assert_rejected("fn main(x: i32) -> i32 { return x; }", "E0217");
    assert_rejected("fn _start() -> i32 { return 0; }", "E0217");
}

#[test]
fn rejects_bad_array_indexing() {
    assert_rejected(
        "fn main() -> i32 { let a: [u8; 2] = [1, 2]; let i: u32 = 1; return a[i] as i32; }",
        "E0213",
    );
    assert_rejected(
        "fn main() -> i32 { let a: [u8; 2] = [1, 2]; return a[2] as i32; }",
        "E0222",
    );
    assert_rejected(
        "fn f(p: *mut u8) -> u8 { return p[0]; } fn main() -> i32 { return 0; }",
        "E0213",
    );
}

#[test]
fn rejects_non_bool_conditions() {
    assert_rejected("fn main() -> i32 { if 1 { return 1; } return 0; }", "E0214");
    assert_rejected(
        "fn main() -> i32 { while 1 { } return 0; }",
        "E0214",
    );
}

#[test]
fn rejects_out_of_range_literals() {
    assert_rejected("fn main() -> i32 { let x: u8 = 256; return 0; }", "E0215");
    assert_rejected("fn main() -> i32 { let x: i8 = 128; return 0; }", "E0215");
    assert_rejected("fn main() -> i32 { let x: i8 = -129; return 0; }", "E0215");
}

#[test]
fn accepts_minimum_signed_literal() {
    let file = SourceFile::new(
        "ok.cx",
        "fn main() -> i32 { let x: i8 = -128; let y: i64 = -9223372036854775808; return x as i32 + y as i32; }",
    );
    assert!(driver::check(&file).is_ok());
}

#[test]
fn rejects_arrays_in_parameters_and_returns() {
    assert_rejected("fn f(a: [u8; 4]) {} fn main() -> i32 { return 0; }", "E0216");
    assert_rejected(
        "fn f() -> [u8; 4] { return [0; 4]; } fn main() -> i32 { return 0; }",
        "E0216",
    );
}

#[test]
fn rejects_function_used_as_value() {
    assert_rejected(
        "fn g() {} fn main() -> i32 { let x: i32 = g; return 0; }",
        "E0218",
    );
}

#[test]
fn rejects_return_value_in_void_function() {
    assert_rejected("fn f() { return 1; } fn main() -> i32 { return 0; }", "E0219");
}

#[test]
fn rejects_amp_mut_of_immutable_place() {
    assert_rejected(
        "fn main() -> i32 { let a: [u8; 2] = [1, 2]; let p: *mut u8 = &mut a[0]; return 0; }",
        "E0220",
    );
}

#[test]
fn rejects_zero_length_array_type() {
    assert_rejected(
        "fn main() -> i32 { let a: [u8; 0] = [1]; return 0; }",
        "E0110",
    );
}

#[test]
fn rejects_oversized_array_repeat() {
    assert_rejected(
        "fn main() -> i32 { let a: [u8; 2000] = [0; 2000]; return 0; }",
        "E0221",
    );
}

#[test]
fn reports_every_error_with_a_source_location() {
    let src = "fn main() -> i32 { let x: u8 = 300; return x; }";
    let file = SourceFile::new("multi.cx", src);
    let diags = match driver::check(&file) {
        Ok(_) => panic!("expected rejection"),
        Err(d) => d,
    };
    let rendered = file.render_all(&diags);
    assert!(rendered.contains("multi.cx:1:"));
    assert!(rendered.contains('^'));
}

#[test]
fn diagnostics_report_file_line_and_column() {
    let src = "fn main() -> i32 { return nope; }";
    let file = SourceFile::new("span.cx", src);
    let diags = match driver::check(&file) {
        Ok(_) => panic!("expected rejection"),
        Err(d) => d,
    };
    let rendered = file.render_all(&diags);
    assert!(rendered.contains("span.cx:1:27"), "rendered:\n{rendered}");
    assert!(rendered.contains("^^^^"), "rendered:\n{rendered}");
}

#[test]
fn rejects_recursive_structs_and_bad_fields() {
    assert_rejected(
        "struct Bad { b: Bad } fn main() -> i32 { return 0; }",
        "E0224",
    );
    assert_rejected(
        "struct P { x: i32 } fn main() -> i32 { let p = P { y: 1 }; return 0; }",
        "E0227",
    );
    assert_rejected(
        "struct P { x: i32, y: i32 } fn main() -> i32 { let p = P { x: 1 }; return 0; }",
        "E0229",
    );
    assert_rejected(
        "struct P { x: i32 } fn main() -> i32 { let p: i32 = 1; print(p.x); return 0; }",
        "E0227",
    );
}

#[test]
fn rejects_non_constant_global_initializers() {
    assert_rejected(
        "static mut M: i32 = 1; static C: i32 = M; fn main() -> i32 { return 0; }",
        "E0226",
    );
    assert_rejected(
        "fn f() -> i32 { return 1; } static C: i32 = f(); fn main() -> i32 { return 0; }",
        "E0226",
    );
}

#[test]
fn rejects_enum_and_character_type_errors() {
    assert_rejected(
        "enum E { A, A } fn main() -> i32 { return 0; }",
        "E0228",
    );
    assert_rejected(
        "enum E { A } fn main() -> i32 { let x: E = E::A; let y: i32 = x; return 0; }",
        "E0203",
    );
    assert_rejected(
        "fn main() -> i32 { let c: char = 'a'; return c + 1; }",
        "E0203",
    );
}

#[test]
fn rejects_invalid_slice_ranges_and_printables() {
    assert_rejected(
        "fn main() -> i32 { let s: str = \"abc\"; print(s[2..1]); return 0; }",
        "E0222",
    );
    assert_rejected(
        "fn main() -> i32 { print(\"abc\"[4..]); return 0; }",
        "E0222",
    );
    assert_rejected(
        "fn main() -> i32 { let a: [i32; 2] = [1, 2]; let s: [i32] = &a; print(s); return 0; }",
        "E0203",
    );
}
