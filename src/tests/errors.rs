use super::run;
use crate::*;

#[test]
fn duplicate_struct_key_diagnostic_points_at_duplicate_key() {
    let source = "(let value {\n  x: 1\n  x: 2\n})";
    let error = match run(source) {
        Err(error) => error,
        Ok(_) => panic!("expected duplicate key error"),
    };
    let span = LAST_ERROR_SPAN.with(|span| *span.borrow());
    let rendered = diagnostic(&error, source, "sample.lisp", span);
    assert!(rendered.starts_with("sample.lisp:3:3:"));
    assert!(rendered.contains("  x: 2"));
}

#[test]
fn diagnostics_include_source_location_and_excerpt() {
    let source = "(let x 1)\n(div x 0)";
    let error = match run(source) {
        Err(error) => error,
        Ok(_) => panic!("expected runtime error"),
    };
    let span = LAST_ERROR_SPAN.with(|span| *span.borrow());
    let rendered = diagnostic(&error, source, "sample.lisp", span);
    assert!(rendered.starts_with("sample.lisp:2:"));
    assert!(rendered.contains("(div x 0)"));
    assert!(rendered.contains("^"));
}

#[test]
fn diagnostics_use_byte_offsets_after_unicode_source() {
    let source = "; section — variadic functions\n(let value 1)\n(div value 0)";
    let error = match run(source) {
        Err(error) => error,
        Ok(_) => panic!("expected division by zero"),
    };
    let span = LAST_ERROR_SPAN.with(|span| *span.borrow());
    let rendered = diagnostic(&error, source, "unicode.lisp", span);
    assert!(rendered.starts_with("unicode.lisp:3:"));
    assert!(rendered.contains("(div value 0)"));
    assert!(!rendered.contains("; section"));
}

#[test]
fn nested_user_function_errors_include_call_trace() {
    let error = match run("(let inner (fn () (div 1 0))) (let outer (fn () (inner))) (outer)") {
        Err(error) => error,
        Ok(_) => panic!("expected runtime error"),
    };
    let span = LAST_ERROR_SPAN.with(|span| *span.borrow());
    let rendered = diagnostic(
        &error,
        "(let inner (fn () (div 1 0))) (let outer (fn () (inner))) (outer)",
        "x",
        span,
    );
    assert!(rendered.contains("call trace:"));
    assert!(rendered.contains("outer"));
    assert!(rendered.contains("inner"));
    assert!(rendered.contains("at x:1:"));
}

#[test]
fn parse_diagnostics_point_at_the_failing_token() {
    let source = "(let x 1";
    let error = Parser {
        ts: lex(source).unwrap(),
        i: 0,
    }
    .program()
    .unwrap_err();
    let span = PARSE_ERROR_SPAN.with(|span| *span.borrow());
    let rendered = diagnostic(&error, source, "sample.lisp", span);
    assert!(rendered.starts_with("sample.lisp:1:"));
    assert!(rendered.contains("^"));
}
