use super::run;
use crate::*;

#[test]
fn duplicate_struct_key_diagnostic_points_at_duplicate_key() {
    let source = "(let value {\n  x: 1\n  x: 2\n})";
    let error = match run(source) {
        Err(error) => error,
        Ok(_) => panic!("expected duplicate key error"),
    };
    let span = LAST_ERROR_SPAN.with(|span| span.get());
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
    let span = LAST_ERROR_SPAN.with(|span| span.get());
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
    let span = LAST_ERROR_SPAN.with(|span| span.get());
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
    let span = LAST_ERROR_SPAN.with(|span| span.get());
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

#[test]
fn lexer_errors_point_at_their_own_token_and_never_at_a_stale_span() {
    // Only the parser used to record a span, so a lexer error was reported
    // against whatever an earlier parse left behind: in a REPL session, typing
    // "abc after a longer line put the caret past the end of the line. The
    // lexer records its own position now, and clears the span first, so each
    // error points at the start of the token it was reading.
    for (source, column) in [
        ("\"abc", 1),
        ("  \"abc", 3),
        ("(let a 1) \"abc", 11),
        ("\"\\q\"", 1),
        ("(add 1 1) \"\\u{110000}\"", 11),
    ] {
        // Leave a real span behind, at an offset well past the end of `source`.
        Parser {
            ts: lex("(let warm 1) (add 1 2)").unwrap(),
            i: 0,
        }
        .program()
        .unwrap();
        let error = match lex(source) {
            Err(error) => error,
            Ok(_) => panic!("expected a lexer error for {source:?}"),
        };
        let span = PARSE_ERROR_SPAN.with(|span| *span.borrow());
        let rendered = diagnostic(&error, source, "sample.lisp", span);
        assert!(
            rendered.starts_with(&format!("sample.lisp:1:{column}:")),
            "{source:?} belongs at column {column}, got:\n{rendered}"
        );
    }
}

#[test]
fn lexer_error_columns_count_characters_not_bytes() {
    // Spans are byte offsets, because that is what the diagnostic slices with,
    // but a column is a character count. A non-ASCII prefix must not push the
    // caret to the right.
    let source = "\"éé\" \"abc";
    let error = match lex(source) {
        Err(error) => error,
        Ok(_) => panic!("expected a lexer error"),
    };
    let span = PARSE_ERROR_SPAN.with(|span| *span.borrow());
    let rendered = diagnostic(&error, source, "sample.lisp", span);
    // The second quote is character 6, byte 7.
    assert!(
        rendered.starts_with("sample.lisp:1:6:"),
        "the caret belongs under the second quote, got:\n{rendered}"
    );
}

#[test]
fn builtin_errors_point_at_the_call_not_the_argument() {
    // DivisionByZero inside `div` used to be attributed to the last evaluated
    // argument (`0`, col 8); it must point at the `(div 5 0)` call (col 1).
    let source = "(div 5 0)";
    let error = match run(source) {
        Err(error) => error,
        Ok(_) => panic!("expected division by zero"),
    };
    let span = LAST_ERROR_SPAN.with(|span| span.get());
    let rendered = diagnostic(&error, source, "sample.lisp", span);
    assert!(rendered.starts_with("sample.lisp:1:1:"), "{rendered}");
    assert!(rendered.contains("(div 5 0)"));
}

#[test]
fn native_module_errors_point_at_the_call_not_the_argument() {
    // An io error used to be attributed to the last evaluated argument (the
    // path string, col 21); it must point at the `(io.open …)` call (col 12).
    let source = r#"(use "io") (io.open "file:///nonexistent?mode=r")"#;
    let error = match run(source) {
        Err(error) => error,
        Ok(_) => panic!("expected IO error"),
    };
    let span = LAST_ERROR_SPAN.with(|span| span.get());
    let rendered = diagnostic(&error, source, "sample.lisp", span);
    assert!(rendered.starts_with("sample.lisp:1:12:"), "{rendered}");
    assert!(rendered.contains("(io.open"));
}

#[test]
fn format_errors_point_at_the_dollar_call() {
    // A format-string error used to be attributed to the last evaluated
    // argument (the format literal); it must point at the `$` call (col 1).
    let source = r#"($ "%q")"#;
    let error = match run(source) {
        Err(error) => error,
        Ok(_) => panic!("expected format arity error"),
    };
    let span = LAST_ERROR_SPAN.with(|span| span.get());
    let rendered = diagnostic(&error, source, "sample.lisp", span);
    assert!(rendered.starts_with("sample.lisp:1:1:"), "{rendered}");
    assert!(rendered.contains("($ \"%q\")"));
}
