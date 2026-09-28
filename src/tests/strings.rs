use super::{check, run};
use crate::*;

#[test]
fn strings_support_grapheme_indexing_and_string_slices() {
    let value = run(r#"(let s "😀abc")
               (expect s[1] "😀")
               (expect s[2] "a")
               (expect s[-1] "c")
               (expect s[1..2] "😀a")
               (expect s[..2] "😀a")
               (expect s[3..] "bc")
               (expect s[-2..-1] "bc")
               (expect s[[1 3]] ["😀" "b"])
               (expect "é👩‍🚀"[1] "é")
               (expect ""[..] "")"#)
    .unwrap();
    assert!(matches!(value, Value::Bool(true)));
}

#[test]
fn string_index_errors_match_collection_rules() {
    assert!(matches!(
        run(r#""abc"[0]"#),
        Err(Error::Type(message)) if message.contains("1-based")
    ));
    assert!(matches!(
        run(r#""abc"[4]"#),
        Err(Error::Name(message)) if message == "string index 4 out of bounds"
    ));
    assert!(matches!(
        run(r#""abc"[1.0]"#),
        Err(Error::Type(message)) if message.contains("integer")
    ));
    assert!(matches!(
        run(r#""éx"[3]"#),
        Err(Error::Name(message)) if message == "string index 3 out of bounds"
    ));
    for source in [r#""abc"[..0]"#, r#""abc"[0..]"#, r#""abc"[3..1]"#] {
        assert!(
            matches!(run(source), Err(Error::Type(_))),
            "{source} should produce TypeError"
        );
    }
}

#[test]
fn formatting_supports_integer_bases_json_and_debug_output() {
    let value = run("(fmt \"%b %h %o\" 10 255 8)").unwrap();
    assert!(matches!(value, Value::Str(text) if text == "1010 ff 10"));

    let value = run("(fmt \"%8h %16h %32h %64h\" 255 255 255 255)").unwrap();
    assert!(matches!(
        value,
        Value::Str(text) if text == "ff 00ff 000000ff 00000000000000ff"
    ));

    let value = run("(fmt \"%8h %16h %32h %64h\" -1 -1 -1 -1)").unwrap();
    assert!(matches!(
        value,
        Value::Str(text) if text == "ff ffff ffffffff ffffffffffffffff"
    ));

    let value = run("(fmt \"%8b %16b %32b %64b\" 5 5 5 5)").unwrap();
    assert!(
        matches!(value, Value::Str(text) if text == "00000101 0000000000000101 00000000000000000000000000000101 0000000000000000000000000000000000000000000000000000000000000101")
    );

    let value = run("(fmt \"%t %t\" 1 [1])").unwrap();
    assert!(matches!(value, Value::Str(text) if text == "int array"));

    let value = run(r#"(fmt "%s" {name:"Yvan" contact:{gsm:"0102030405"} score:["ok" 2]})"#).unwrap();
    assert!(
        matches!(value, Value::Str(text) if text == r#"{name:"Yvan" contact:{gsm:"0102030405"} score:["ok" 2]}"#)
    );

    let value = run("(fmt \"%j\" {name: \"Ada\" values: [1 t _]})").unwrap();
    assert!(
        matches!(value, Value::Str(text) if text == r#"{"name":"Ada","values":[1,true,null]}"#)
    );

    let value = run("(fmt \"%v\" {name: \"Ada\" values: [1 t _]})").unwrap();
    assert!(
        matches!(value, Value::Str(text) if text == r#"Struct({name: Str("Ada"), values: Array([Int(1), Bool(true), Null])})"#)
    );

    let value = run("(fmt \"%v\" [1 \"x\"])").unwrap();
    assert!(matches!(value, Value::Str(text) if text == r#"Array([Int(1), Str("x")])"#));
}

#[test]
fn debug_function_identity_is_stable_for_aliases_and_distinct_for_new_closures() {
    let output = run(r#"(let clos (fn (x) x)) (let same clos) (fmt "%v %v" clos same)"#).unwrap();
    let Value::Str(output) = output else {
        panic!("expected debug output string");
    };
    let (first, second) = output.split_once(' ').unwrap();
    assert_eq!(first, second, "an alias must keep its closure's identity");
    assert!(
        first.starts_with("Function#") && first.ends_with("(x)"),
        "{first}"
    );

    let other = run(r#"(fn (x) x)"#).unwrap();
    let Value::Function(other) = other else {
        panic!("expected a function");
    };
    assert_ne!(first, debug_render(&Value::Function(other)));
}

#[test]
fn debug_render_keeps_module_descriptors_structural() {
    let Value::Str(output) = run(r#"(use "io") (fmt "%v" io.write)"#).unwrap() else {
        panic!("expected debug output string");
    };
    assert_eq!(
        output,
        r#"Struct({_: NativeFunction(io.write), spec: Struct({documentation: Str("Write a string to a file descriptor."), arity: Int(2), type: Array([Str("int"), Str("string")]), return: Array([Str("int")])})})"#
    );
}

#[test]
fn formatting_percent_q_quotes_strings_as_lisp_literals() {
    // %s inserts the raw contents; %q wraps them in a parseable Lisp literal.
    let value = run(r#"(fmt "%s|%q" "hello" "hello")"#).unwrap();
    assert!(matches!(value, Value::Str(text) if text == r#"hello|"hello""#));

    let cases: &[(&str, &str)] = &[
        (r#""hello""#, r#""hello""#),
        (r#""hello world""#, r#""hello world""#),
        (r#""hello \"world\"""#, r#""hello \"world\"""#),
        (r#""C:\\temp\\foo""#, r#""C:\\temp\\foo""#),
        (r#""line1\nline2""#, r#""line1\nline2""#),
        (r#""""#, r#""""#),
    ];
    for &(literal, expected) in cases {
        let value = run(&format!("(fmt \"%q\" {literal})")).unwrap();
        assert!(
            matches!(&value, Value::Str(text) if text == expected),
            "{literal} -> {}, expected {}",
            render(&value),
            expected
        );
    }
}

#[test]
fn formatting_percent_q_round_trips_through_the_reader() {
    let literals = [
        r#""hello""#,
        r#""hello world""#,
        r#""hello \"world\"""#,
        r#""C:\\temp\\foo""#,
        r#""line1\nline2""#,
        r#""""#,
    ];
    for literal in literals {
        let Value::Str(content) = run(&format!("(fmt \"%s\" {literal})")).unwrap() else {
            panic!("%s of {literal} was not a string");
        };
        let Value::Str(quoted) = run(&format!("(fmt \"%q\" {literal})")).unwrap() else {
            panic!("%q of {literal} was not a string");
        };
        assert!(
            quoted.starts_with('"') && quoted.ends_with('"'),
            "%q of {literal} is not a literal: {quoted:?}"
        );
        // The quoted output is itself a Lisp program: a single string literal.
        let reparsed = run(&quoted)
            .unwrap_or_else(|e| panic!("{quoted:?} (from {literal}) does not reparse: {e:?}"));
        assert!(
            matches!(reparsed, Value::Str(round) if round == content),
            "{literal} did not round-trip through %q"
        );
    }
}

#[test]
fn formatting_percent_q_follows_references() {
    let value = run(r#"(let s "hello") (fmt "%q" ^s)"#).unwrap();
    assert!(matches!(value, Value::Str(text) if text == r#""hello""#));
}

#[test]
fn formatting_percent_x_serializes_values_as_lisp_source() {
    let cases: &[(&str, &str)] = &[
        (r#"(fmt "%x" "hello")"#, r#""hello""#),
        (r#"(fmt "%x" "hello \"world\"")"#, r#""hello \"world\"""#),
        ("(fmt \"%x\" 42)", "42"),
        ("(fmt \"%x\" 3.14)", "3.14"),
        ("(fmt \"%x\" t)", "t"),
        ("(fmt \"%x\" f)", "f"),
        ("(fmt \"%x\" _)", "_"),
        ("(fmt \"%x\" [1 2 3])", "[1 2 3]"),
        (r#"(fmt "%x" [1 "hello" t _ 42])"#, r#"[1 "hello" t _ 42]"#),
        (
            r#"(fmt "%x" {name:"Yvan" age:56})"#,
            r#"{name:"Yvan" age:56}"#,
        ),
        (
            r#"(fmt "%x" {name:"Yvan" contact:{gsm:"0102030405"} scores:[10 20 30]})"#,
            r#"{name:"Yvan" contact:{gsm:"0102030405"} scores:[10 20 30]}"#,
        ),
        (r#"(fmt "%x" [1 "hello" [2 3]])"#, r#"[1 "hello" [2 3]]"#),
        (
            r#"(fmt "%x" {a:[1 {b:[2 [3]]}] c:"e"})"#,
            r#"{a:[1 {b:[2 [3]]}] c:"e"}"#,
        ),
        // Floats must serialize as reader-readiable plain decimals.
        ("(fmt \"%x\" 2.0)", "2.0"),
        ("(fmt \"%x\" (pow 10.0 21))", "1000000000000000000000.0"),
        ("(fmt \"%x\" Inf)", "Inf"),
        ("(fmt \"%x\" (mul Inf 0.0))", "NaN"),
        ("(fmt \"%x\" -0.0)", "-0.0"),
    ];
    for &(source, expected) in cases {
        let value = run(source).unwrap();
        assert!(
            matches!(&value, Value::Str(text) if text == expected),
            "{source} -> {}, expected {}",
            render(&value),
            expected
        );
    }
}

#[test]
fn formatting_percent_x_round_trips_through_the_reader() {
    let literals = [
        r#""hello""#,
        r#""hello \"world\"""#,
        "42",
        "3.14",
        "t",
        "f",
        "_",
        "[1 2 3]",
        r#"[1 "hello" [2 3]]"#,
        r#"{name:"Yvan" age:56}"#,
        r#"{name:"Yvan" contact:{gsm:"0102030405"}}"#,
        r#"[1 "hello" {inner:[t f _ {deep:"x"}]} [2 [3 4]]]"#,
        r#"{a:[1 {b:[2 [3]]}] c:{d:"e"}}"#,
    ];
    for literal in literals {
        let original = run(literal).unwrap_or_else(|e| panic!("{literal} does not parse: {e:?}"));
        let Value::Str(serialized) = run(&format!("(fmt \"%x\" {literal})")).unwrap() else {
            panic!("%x of {literal} was not a string");
        };
        let reparsed = run(&serialized)
            .unwrap_or_else(|e| panic!("{serialized:?} (from {literal}) does not reparse: {e:?}"));
        assert!(
            equals(&reparsed, &original),
            "{literal} -> {serialized} did not round-trip"
        );
    }
    // Computed values whose Rust rendering uses scientific notation or lacks
    // a decimal point must still round-trip through %x.
    for source in [
        "(pow 10.0 21)",
        "(pow 10.0 -7)",
        "2.0",
        "(div 1.0 2.0)",
        "(mul 3.0 1.5)",
        "Inf",
        "-Inf",
    ] {
        let original = run(source).unwrap();
        let Value::Str(serialized) = run(&format!("(fmt \"%x\" {source})")).unwrap() else {
            panic!("%x of {source} was not a string");
        };
        let reparsed = run(&serialized)
            .unwrap_or_else(|e| panic!("{serialized:?} (from {source}) does not reparse: {e:?}"));
        assert!(
            equals(&reparsed, &original),
            "{source} -> {serialized} did not round-trip"
        );
    }
}

#[test]
fn formatting_percent_x_follows_references() {
    let value = run(r#"(let s "hi") (fmt "%x" ^s)"#).unwrap();
    assert!(matches!(value, Value::Str(text) if text == r#""hi""#));
    let value = run(r#"(let a [1 2]) (fmt "%x" ^a)"#).unwrap();
    assert!(matches!(value, Value::Str(text) if text == "[1 2]"));
}

#[test]
fn formatting_percent_x_writes_a_function_as_its_own_source() {
    check(r#"(fmt "%x" (fn (y) (add y 1)))"#, r#""(fn (y) (add y 1))""#);
    check(r#"(fmt "%x" (fn () 7))"#, r#""(fn () 7)""#);
    // A body of several forms keeps its block, and a call body keeps the
    // parentheses that stop `fn` reading it as an immediate call.
    check(
        r#"(fmt "%x" (fn (b) ((let c 1) (add b c))))"#,
        r#""(fn (b) ((let c 1) (add b c)))""#,
    );
    check(
        r#"(fmt "%x" (fn (n) (fn (m) (add n m))))"#,
        r#""(fn (n) (fn (m) (add n m)))""#,
    );
    check(
        r#"(fmt "%x" (fn (x) (if (eq x 0) (break) x)))"#,
        r#""(fn (x) (if (eq x 0) (break) x))""#,
    );
    check(
        r#"(fmt "%x" (fn (x) (str.upper x)))"#,
        r#""(fn (x) (str.upper x))""#,
    );
    // Postfix forms print with no space, and a reference prints its `^` bare,
    // because that is the only spelling the reader takes back as a reference
    // rather than as a one-form block.
    check(r#"(fmt "%x" (fn (x) (^x)))"#, r#""(fn (x) (^x))""#);
    check(
        r#"(fmt "%x" (fn (x) [1 x "s" {k:1} t _]))"#,
        r#""(fn (x) [1 x \"s\" {k:1} t _])""#,
    );
}

#[test]
fn formatting_percent_x_of_a_function_never_grows_its_parentheses() {
    // A parenthesised `(^x)` would read back as a one-form block, so each trip
    // through `eval` would wrap the body once more.
    for body in [
        "(^x)",
        "(^x[0])",
        "(add x 1)",
        "x",
        "[1 x]",
        "((let c 1) x)",
    ] {
        let once = run(&format!(r#"(let myfn (fn (x) {body})) (fmt "%x" myfn)"#)).unwrap();
        let twice = run(&format!(
            r#"(let myfn (eval (fmt "%x" (fn (x) {body})))) (fmt "%x" myfn)"#
        ))
        .unwrap();
        assert!(
            equals(&once, &twice),
            "{body} did not print to a fixed point"
        );
    }
}

#[test]
fn formatting_percent_x_of_a_function_reads_back_as_an_equivalent_function() {
    check(
        r#"(let dbl (fn (n) (mul n 2)))
           (let copy (eval (fmt "%x" dbl)))
           [(copy 21) (dbl 21)]"#,
        "[42 42]",
    );
    // The name is a `let` annotation and `fn` refuses one, so the copy is
    // anonymous: equal in behaviour, never `eq` to the original.
    check(
        r#"(let dbl (fn (n) (mul n 2)))
           (eq (eval (fmt "%x" dbl)) dbl)"#,
        "f",
    );
}

#[test]
fn formatting_percent_x_drops_the_bindings_a_closure_captured() {
    // A function value carries its body, not its environment. The text is
    // valid and reads back, but a name the body closed over is only bound if
    // the reader happens to have it, and calling the copy then reports it.
    check(
        r#"(let make (fn (n) (fn (x) (add x n))))
           (let closure (make 10))
           [((fmt "%x" closure)) (closure 5)]"#,
        r#"["(fn (x) (add x n))" 15]"#,
    );
    let copied = run(r#"(let make (fn (n) (fn (x) (add x n))))
           (let closure (make 10))
           (eval (fmt "%x" closure))"#)
    .unwrap();
    assert!(
        matches!(copied, Value::Function(_)),
        "the copy is a function"
    );
    let error = match run(r#"(let make (fn (n) (fn (x) (add x n))))
           (let closure (make 10))
           (let copy (eval (fmt "%x" closure)))
           (copy 5)"#)
    {
        Err(error) => error,
        Ok(_) => panic!("a copy with no binding named n cannot be called"),
    };
    assert!(matches!(error, Error::Name(name) if name == "n"));
}

#[test]
fn formatting_percent_x_writes_a_native_as_its_module_path() {
    check(r#"(use "str") (fmt "%x" str.upper._)"#, r#""str.upper._""#);
    // A module member read as a value is a descriptor struct, so `%x` reaches
    // the native inside it and writes the path the reader can follow. A native
    // is compared by identity, so an `eq` here is a real round trip: the copy
    // holds the very same callable, not one that merely looks like it.
    for src in [
        r#"(eq str.upper._ (eval (fmt "%x" str.upper._)))"#,
        r#"(eq io.write (eval (fmt "%x" io.write)))"#,
        r#"(eq str (eval (fmt "%x" str)))"#,
    ] {
        check(&format!(r#"(use "io") (use "str") {src}"#), "t");
    }
}

#[test]
fn formatting_percent_x_writes_a_function_nested_in_a_composite() {
    check(r#"(fmt "%x" [(fn (a) a) 1])"#, r#""[(fn (a) a) 1]""#);
    check(r#"(fmt "%x" {k:(fn (a) a)})"#, r#""{k:(fn (a) a)}""#);
}

#[test]
fn formatting_percent_j_still_refuses_a_function() {
    for src in [
        r#"(fmt "%j" (fn (x) x))"#,
        r#"(use "str") (fmt "%j" str.upper._)"#,
    ] {
        let error = match run(src) {
            Err(error) => error,
            Ok(_) => panic!("%j has no way to encode a function"),
        };
        assert_eq!(
            error.to_string(),
            "FormatError: FormatTypeError: %j cannot encode function"
        );
    }
}

#[test]
fn formatting_percent_q_rejects_non_strings() {
    for src in [
        "(fmt \"%q\" 5)",
        "(fmt \"%q\" t)",
        "(fmt \"%q\" [1 2])",
        "(fmt \"%q\" {a:1})",
    ] {
        assert!(
            matches!(run(src), Err(Error::Format(message)) if message.contains("%q expects string")),
            "expected type error for {src}"
        );
    }
    assert!(matches!(
        run("(fmt \"%q\")"),
        Err(Error::Format(message)) if message == "FormatArityError"
    ));
    assert!(matches!(
        run(r#"(fmt "%q" "a" "b")"#),
        Err(Error::Format(message)) if message == "FormatArityError"
    ));
}

#[test]
fn format_type_and_value_of_null() {
    check(r#"(fmt "%t:%s" _ _)"#, r#""null:""#);
}

#[test]
fn format_type_and_value_of_true() {
    check(r#"(fmt "%t:%s" t t)"#, r#""bool:true""#);
}

#[test]
fn format_type_and_value_of_false() {
    check(r#"(fmt "%t:%s" f f)"#, r#""bool:false""#);
}

#[test]
fn format_type_and_value_of_zero() {
    check(r#"(fmt "%t:%s" 0 0)"#, r#""int:0""#);
}

#[test]
fn format_type_and_value_of_integer() {
    check(r#"(fmt "%t:%s" 127 127)"#, r#""int:127""#);
}

#[test]
fn format_type_and_value_of_negative_integer() {
    check(r#"(fmt "%t:%s" -127 -127)"#, r#""int:-127""#);
}

#[test]
fn format_type_and_value_of_hex_literal() {
    check(r#"(fmt "%t:%s" 0x7F 127)"#, r#""int:127""#);
}

#[test]
fn format_type_and_value_of_binary_literal() {
    check(r#"(fmt "%t:%s" 0b01111111 127)"#, r#""int:127""#);
}

#[test]
fn format_type_and_value_of_octal_literal() {
    check(r#"(fmt "%t:%s" 0o177 127)"#, r#""int:127""#);
}

#[test]
fn base_literals_parse_i64_min() {
    check("-0x8000000000000000", "-9223372036854775808");
    let binary = format!("-0b1{}", "0".repeat(63));
    check(&binary, "-9223372036854775808");
    let octal = format!("-0o1{}", "0".repeat(21));
    check(&octal, "-9223372036854775808");
}

#[test]
fn positive_literals_beyond_i64_max_are_rejected() {
    for src in [
        "9223372036854775808",
        "+9223372036854775808",
        "0x8000000000000000",
        "+0x8000000000000000",
        "0xFFFFFFFFFFFFFFFF",
    ] {
        assert!(
            matches!(
                run(src),
                Err(Error::Parse(message)) if message.contains("integer out of range")
            ),
            "{src} should be rejected as out of range"
        );
    }
}

#[test]
fn negative_literals_below_i64_min_are_rejected() {
    for src in [
        "-9223372036854775809",
        "-0x8000000000000001",
        "-0xFFFFFFFFFFFFFFFF",
    ] {
        assert!(
            matches!(
                run(src),
                Err(Error::Parse(message)) if message.contains("integer out of range")
            ),
            "{src} should be rejected as out of range"
        );
    }
}

#[test]
fn base_literal_with_invalid_digits_is_rejected() {
    assert!(matches!(
        run("0xZZ"),
        Err(Error::Parse(message)) if message.starts_with("invalid number")
    ));
}

#[test]
fn format_type_and_value_of_zero_float() {
    check(r#"(fmt "%t:%s" 0.0 0.0)"#, r#""float:0""#);
}

#[test]
fn format_type_and_value_of_float() {
    check(r#"(fmt "%t:%s" 1.5 1.5)"#, r#""float:1.5""#);
}

#[test]
fn format_type_and_value_of_negative_float() {
    check(r#"(fmt "%t:%s" -1.5 -1.5)"#, r#""float:-1.5""#);
}

#[test]
fn format_type_and_value_of_scientific_notation() {
    for (src, expected) in [
        ("1e3", "float:1000"),
        ("1E3", "float:1000"),
        ("1e+3", "float:1000"),
        ("1E+3", "float:1000"),
        ("1e-3", "float:0.001"),
        ("1E-3", "float:0.001"),
        ("1.5e2", "float:150"),
        ("1.5e-3", "float:0.0015"),
        ("-2.5E+4", "float:-25000"),
        ("1e0", "float:1"),
        ("1.0e0", "float:1"),
        ("0.0e0", "float:0"),
        ("5e-1", "float:0.5"),
    ] {
        let format = format!(r#"(fmt "%t:%s" {src} {src})"#);
        check(&format, &format!(r#""{expected}""#));
    }
}

#[test]
fn an_exponent_does_not_make_a_literal_an_integer() {
    // The value being integral is beside the point: the exponent decides the
    // type, so 1e0 is a float and only a bare 1 is an integer.
    for (src, expected) in [
        ("1", "int:1"),
        ("-42", "int:-42"),
        ("1.0", "float:1"),
        ("1e0", "float:1"),
        ("1.0e0", "float:1"),
        ("1e3", "float:1000"),
    ] {
        let format = format!(r#"(fmt "%t:%s" {src} {src})"#);
        check(&format, &format!(r#""{expected}""#));
    }
}

#[test]
fn an_exponent_is_read_the_same_way_by_arithmetic() {
    // The float is the same value the literal denotes, so it mixes with a
    // float operand and promotes an integer one.
    check(r#"(eq 1e3 1000.0)"#, "t");
    check(r#"(eq 1e3 1000)"#, "t");
    check(r#"(add 1e0 2)"#, "3.0");
    check(r#"(mul 1e3 1e3)"#, "1e6");
    check(r#"(div 1 1e-1)"#, "10.0");
    check(r#"(pow 1e1 3)"#, "1e3");
    // It is a float, so an integer-only builtin still refuses it.
    assert!(matches!(
        run("(bit-and 1e0 1)"),
        Err(Error::Type(message)) if message.contains("require integers")
    ));
}

#[test]
fn an_exponent_without_digits_is_rejected() {
    // The message names the token that failed, which ends at the sign: a second
    // sign is not part of it, so `1e+-` is reported as `1e+`.
    for (src, token) in [
        ("1e", "1e"),
        ("1E", "1E"),
        ("2e", "2e"),
        ("0e", "0e"),
        ("1e+", "1e+"),
        ("1e-", "1e-"),
        ("1E+", "1E+"),
        ("1E-", "1E-"),
        ("1.5e", "1.5e"),
        ("1.0e", "1.0e"),
        ("1e+-", "1e+"),
        ("-1e", "-1e"),
        ("+1e", "+1e"),
    ] {
        assert!(
            matches!(
                run(src),
                Err(Error::Parse(message)) if message == format!("invalid number `{token}`")
            ),
            "{src} should be rejected as a number with no exponent digits"
        );
    }
}

#[test]
fn an_exponent_may_not_be_followed_by_a_dot() {
    // The token ends at the exponent, so this is `1e` with nothing after it and
    // it is rejected as such rather than read as a field access on a float.
    assert!(matches!(
        run("1e.3"),
        Err(Error::Parse(message)) if message == "invalid number `1e`"
    ));
    // The same holds for a mantissa that does have its fraction.
    assert!(matches!(
        run("1.5e.3"),
        Err(Error::Parse(message)) if message == "invalid number `1.5e`"
    ));
}

#[test]
fn radix_literals_are_not_exponent_literals() {
    // `e` is a hexadecimal digit, so the radix prefix has to be recognised
    // before an exponent is looked for or every hex literal here would stop
    // being a number.
    for (src, expected) in [
        ("0x1e3", "int:483"),
        ("0x1E3", "int:483"),
        ("0xE", "int:14"),
        ("0xe", "int:14"),
        ("-0x1e3", "int:-483"),
        ("-0xE", "int:-14"),
    ] {
        let format = format!(r#"(fmt "%t:%s" {src} {src})"#);
        check(&format, &format!(r#""{expected}""#));
    }
    // Digits that are not valid in the base are still refused, and refused by
    // the radix path rather than the exponent one.
    for src in ["0b1e3", "0o1e3", "0x1p3"] {
        assert!(
            matches!(
                run(src),
                Err(Error::Parse(message)) if message == format!("invalid number `{src}`")
            ),
            "{src} should be rejected as an invalid radix literal"
        );
    }
}

#[test]
fn a_float_name_is_not_read_as_an_exponent_literal() {
    // f64 parsing would accept all of these, so the exponent is only looked for
    // in a token that starts with a digit. The capitalised NaN and Inf are the
    // float literals; these stay names and stay unbound. `-NaN` is not in this
    // list because it is a literal of its own now, and it is a name for a
    // different reason: the parser's table is what resolves it.
    for src in ["inf", "nan", "infinity", "e3", "E3"] {
        assert!(
            matches!(run(src), Err(Error::Name(_))),
            "{src} should still be a name and not a number"
        );
    }
    // A signed lowercase spelling is not a name at all: a name must start
    // with a letter, so the lexer refuses the run.
    for src in ["-inf", "-nan", "-infinity"] {
        assert!(
            matches!(run(src), Err(Error::Parse(message)) if message == format!("invalid name `{src}`")),
            "{src} should be refused as an invalid name"
        );
    }
    check(r#"(fmt "%t:%s" Inf Inf)"#, r#""float:Inf""#);
    check(r#"(fmt "%t:%s" NaN NaN)"#, r#""float:NaN""#);
}

#[test]
fn text_after_a_complete_exponent_is_a_separate_token() {
    // A literal ends at its last digit, exactly as `1.5x` already did, so the
    // remainder is lexed on its own rather than making the literal malformed.
    for (src, name) in [("1e3abc", "abc"), ("1e3x", "x"), ("1.5e2abc", "abc")] {
        assert!(
            matches!(run(src), Err(Error::Name(message)) if message == name),
            "{src} should read its literal and then fail on the name {name}"
        );
    }
}

#[test]
fn format_type_and_value_of_string() {
    check(r#"(fmt "%t:%s" "text" "text")"#, r#""string:text""#);
}

#[test]
fn format_type_and_value_of_array() {
    check(r#"(fmt "%t:%s" [1 2 3] [1 2 3])"#, r#""array:[1 2 3]""#);
}

#[test]
fn format_type_and_value_of_function() {
    check(r#"(fmt "%t:%s" (fn (x) x) (fn (x) x))"#, r#""function:<fn>""#);
}

#[test]
fn format_integer_as_binary() {
    check(r#"(fmt "%b" 5)"#, r#""101""#);
}

#[test]
fn format_binary_padded_to_eight() {
    check(r#"(fmt "%8b" 5)"#, r#""00000101""#);
}

#[test]
fn format_negative_binary_padded_to_eight() {
    check(r#"(fmt "%8b" -5)"#, r#""11111011""#);
}

#[test]
fn format_binary_padded_to_sixteen() {
    check(r#"(fmt "%16b" 5)"#, r#""0000000000000101""#);
}

#[test]
fn format_negative_binary_padded_to_sixteen() {
    check(r#"(fmt "%16b" -5)"#, r#""1111111111111011""#);
}

#[test]
fn format_binary_padded_to_thirty_two() {
    check(r#"(fmt "%32b" 5)"#, r#""00000000000000000000000000000101""#);
}

#[test]
fn format_binary_padded_to_sixty_four() {
    check(
        r#"(fmt "%64b" 5)"#,
        r#""0000000000000000000000000000000000000000000000000000000000000101""#,
    );
}

#[test]
fn format_minus_one_binary_padded_to_eight() {
    check(r#"(fmt "%8b" -1)"#, r#""11111111""#);
}

#[test]
fn format_minus_one_binary_padded_to_sixteen() {
    check(r#"(fmt "%16b" -1)"#, r#""1111111111111111""#);
}

#[test]
fn format_decimal_hex_and_octal_rendering() {
    check(r#"(fmt "%d %h %o" 127 127 127)"#, r#""127 7f 177""#);
}

#[test]
fn format_mixed_specifiers_render_inline() {
    check(r#"(fmt "%s %f %t" "text" 1.5 1.5)"#, r#""text 1.5 float""#);
}

#[test]
fn format_struct_as_json() {
    check(
        r#"(fmt "%j" {name:"Ada" values:[1 t _]})"#,
        r#""{\"name\":\"Ada\",\"values\":[1,true,null]}""#,
    );
}

#[test]
fn format_struct_with_debug_verb() {
    check(
        r#"(fmt "%v" {name:"Ada" values:[1 t _]})"#,
        r#""Struct({name: Str(\"Ada\"), values: Array([Int(1), Bool(true), Null])})""#,
    );
}

#[test]
fn format_array_with_debug_verb() {
    check(
        r#"(fmt "%v" [1 "text"])"#,
        r#""Array([Int(1), Str(\"text\")])""#,
    );
}

#[test]
fn format_escaped_percent_sign() {
    check(r#"(fmt "100%%")"#, r#""100%""#);
}

#[test]
fn format_type_and_value_of_nan() {
    check(r#"(fmt "%t:%s" NaN NaN)"#, r#""float:NaN""#);
}

#[test]
fn format_type_and_value_of_positive_nan() {
    check(r#"(fmt "%t:%s" +NaN +NaN)"#, r#""float:NaN""#);
}

#[test]
fn format_type_and_value_of_infinity() {
    check(r#"(fmt "%t:%s" Inf Inf)"#, r#""float:Inf""#);
}

#[test]
fn format_type_and_value_of_positive_infinity() {
    check(r#"(fmt "%t:%s" +Inf +Inf)"#, r#""float:Inf""#);
}

#[test]
fn format_type_and_value_of_negative_infinity() {
    check(r#"(fmt "%t:%s" -Inf -Inf)"#, r#""float:-Inf""#);
}

#[test]
fn format_d_renders_add_result() {
    check(r#"(fmt "%d" (add 2 3))"#, r#""5""#);
}

#[test]
fn format_d_renders_sub_result() {
    check(r#"(fmt "%d" (sub 10 3))"#, r#""7""#);
}

#[test]
fn format_d_renders_mul_result() {
    check(r#"(fmt "%d" (mul 6 7))"#, r#""42""#);
}

#[test]
fn format_d_renders_div_result() {
    check(r#"(fmt "%d" (div 10 2))"#, r#""5""#);
}

#[test]
fn format_d_renders_integer_division() {
    check(r#"(fmt "%d" (div 7 2))"#, r#""3""#);
}

#[test]
fn format_d_renders_negative_mod_result() {
    check(r#"(fmt "%d" (mod -7 2))"#, r#""-1""#);
}

#[test]
fn format_d_renders_pow_result() {
    check(r#"(fmt "%d" (pow 2 10))"#, r#""1024""#);
}

#[test]
fn format_f_renders_float_division() {
    check(r#"(fmt "%f" (div 7.0 2.0))"#, r#""3.5""#);
}

#[test]
fn format_f_renders_float_addition() {
    check(r#"(fmt "%f" (add 1.5 2.0))"#, r#""3.5""#);
}

#[test]
fn format_f_renders_float_subtraction() {
    check(r#"(fmt "%f" (sub 5.5 2.0))"#, r#""3.5""#);
}

#[test]
fn format_f_renders_float_multiplication() {
    check(r#"(fmt "%f" (mul 1.75 2.0))"#, r#""3.5""#);
}

#[test]
fn format_f_renders_float_power() {
    check(r#"(fmt "%f" (pow 1.5 2.0))"#, r#""2.25""#);
}

#[test]
fn format_f_renders_negative_operand_addition() {
    check(r#"(fmt "%f" (add -1.5 2.0))"#, r#""0.5""#);
}

#[test]
fn format_f_renders_negative_operand_subtraction() {
    check(r#"(fmt "%f" (sub -1.5 2.0))"#, r#""-3.5""#);
}

#[test]
fn format_f_renders_negative_operand_multiplication() {
    check(r#"(fmt "%f" (mul -1.5 2.0))"#, r#""-3""#);
}

#[test]
fn format_f_renders_negative_operand_division() {
    check(r#"(fmt "%f" (div -7.0 2.0))"#, r#""-3.5""#);
}

#[test]
fn format_f_renders_large_integer_without_precision_loss() {
    check(
        r#"(fmt "%f" 9223372036854775807)"#,
        r#""9223372036854775807""#,
    );
}

#[test]
fn format_f_renders_integer_above_f64_mantissa_exactly() {
    // 2^53 + 1 is not representable in f64; it must not round to 9007199254740992.
    check(r#"(fmt "%f" 9007199254740993)"#, r#""9007199254740993""#);
}

#[test]
fn format_f_renders_negative_large_integer_exactly() {
    check(r#"(fmt "%f" -9007199254740993)"#, r#""-9007199254740993""#);
}

#[test]
fn format_f_renders_min_integer_exactly() {
    check(
        r#"(fmt "%f" -9223372036854775808)"#,
        r#""-9223372036854775808""#,
    );
    check(
        r#"(fmt "%f" -0x8000000000000000)"#,
        r#""-9223372036854775808""#,
    );
}

#[test]
fn format_f_renders_small_integer_like_decimal() {
    check(r#"(fmt "%f" 42)"#, r#""42""#);
    check(r#"(fmt "%f" -127)"#, r#""-127""#);
}

#[test]
fn format_f_renders_promoted_integer_arithmetic_as_float() {
    check(r#"(fmt "%f" (add 1 2.5))"#, r#""3.5""#);
    check(r#"(fmt "%f" (div 7 2.0))"#, r#""3.5""#);
}

#[test]
fn format_type_of_integer_plus_float_is_float() {
    check(r#"(fmt "%t:%s" (add 1 2.5) (add 1 2.5))"#, r#""float:3.5""#);
}

#[test]
fn format_type_of_integer_minus_float_is_float() {
    check(r#"(fmt "%t:%s" (sub 5 1.5) (sub 5 1.5))"#, r#""float:3.5""#);
}

#[test]
fn format_type_of_integer_times_float_is_float() {
    check(r#"(fmt "%t:%s" (mul 7 0.5) (mul 7 0.5))"#, r#""float:3.5""#);
}

#[test]
fn format_type_of_integer_divided_by_float_is_float() {
    check(r#"(fmt "%t:%s" (div 7 2.0) (div 7 2.0))"#, r#""float:3.5""#);
}

#[test]
fn format_type_of_integer_division_is_integer() {
    check(r#"(fmt "%t:%s" (div 7 2)   (div 7 2))"#, r#""int:3""#);
}

#[test]
fn format_d_renders_bit_and_result() {
    check(r#"(fmt "%d" (bit-and 0b110 0b101))"#, r#""4""#);
}

#[test]
fn format_d_renders_bit_or_result() {
    check(r#"(fmt "%d" (bit-or 0b110 0b101))"#, r#""7""#);
}

#[test]
fn format_d_renders_bit_xor_result() {
    check(r#"(fmt "%d" (bit-xor 0b110 0b101))"#, r#""3""#);
}

#[test]
fn format_d_renders_bit_not_result() {
    check(r#"(fmt "%d" (bit-not 0b101))"#, r#""-6""#);
}

#[test]
fn format_d_renders_shift_left_result() {
    check(r#"(fmt "%d" (bit-shl 1 4))"#, r#""16""#);
}

#[test]
fn format_d_renders_shift_right_result() {
    check(r#"(fmt "%d" (bit-shr 16 2))"#, r#""4""#);
}

#[test]
fn format_tilde_regex_full_and_capture_shorthands() {
    check(
        r#"(fmt "%~%1" "mgU~^(.*) " "Hello the world")"#,
        r#""Hello ""#,
    );
    check(r#"(fmt "%~%2" "mgU~^(.*) " "Hello the world")"#, r#""Hello""#);
    check(
        r#"(fmt "%~it's not %2, it's Good morning" "mgU~^(.*) " "Hello the world")"#,
        r#""it's not Hello, it's Good morning""#,
    );
    check(r#"(fmt "%~%1" "(a)(b)" "xxabyyabzz")"#, r#""ab""#);
    check(r#"(fmt "%~%2" "(a)(b)" "xxabyyabzz")"#, r#""a""#);
    check(r#"(fmt "%~%3" "(a)(b)" "xxabyyabzz")"#, r#""b""#);
}

#[test]
fn format_tilde_regex_match_capture_selectors() {
    check(r#"(fmt "%~%1.1" "(a)(b)" "xxabyyabzz")"#, r#""ab""#);
    check(r#"(fmt "%~%1.2" "(a)(b)" "xxabyyabzz")"#, r#""a""#);
    check(r#"(fmt "%~%1.3" "(a)(b)" "xxabyyabzz")"#, r#""b""#);
    check(r#"(fmt "%~%2.1" "(a)(b)" "xxabyyabzz")"#, r#""ab""#);
    check(r#"(fmt "%~%2.2" "(a)(b)" "xxabyyabzz")"#, r#""a""#);
    check(r#"(fmt "%~%2.3" "(a)(b)" "xxabyyabzz")"#, r#""b""#);
    check(
        r#"(fmt "%~%1.1 and %1.2 and %2.1" "(a)(b)" "abab")"#,
        r#""ab and a and ab""#,
    );
}

#[test]
fn format_tilde_regex_option_default_gmu() {
    // default gmu: multiline anchors on, case-sensitive
    check(r#"(fmt "%~%1" "^b$" "a\nb")"#, r#""b""#);
    check(r#"(fmt "%~%1" "^hello$" "HELLO")"#, r#""f""#);
}

#[test]
fn format_tilde_regex_options_replace_defaults() {
    // explicit options replace the default set
    check(r#"(fmt "%~%1" "U~^b$" "a\nb")"#, r#""f""#);
    check(r#"(fmt "%~%1" "m~^b$" "a\nb")"#, r#""b""#);
    check(r#"(fmt "%~%1" "i~^hello$" "HELLO")"#, r#""HELLO""#);
    check(r#"(fmt "%~%1" "s~^a.b$" "a\nb")"#, r#""a\nb""#);
    check(r#"(fmt "%~%1" "x~a b" "ab")"#, r#""ab""#);
    // R: CRLF is a line terminator (with m), unlike m alone
    check(r#"(fmt "%~%1" "mR~ab$" "ab\r\ncd")"#, r#""ab""#);
    check(r#"(fmt "%~%1" "m~ab$" "ab\r\ncd")"#, r#""f""#);
}

#[test]
fn format_tilde_regex_g_finds_all_matches() {
    check(r#"(fmt "%~%2.1" "g~(a)|(b)" "ab")"#, r#""b""#);
    check(r#"(fmt "%~%2.2" "g~(a)|(b)" "ab")"#, r#""_""#);
}

#[test]
fn format_tilde_regex_without_g_finds_first_match_only() {
    check(r#"(fmt "%~%1.1" "U~(a)|(b)" "ab")"#, r#""a""#);
    assert!(matches!(
        run(r#"(fmt "%~%2.1" "U~(a)|(b)" "ab")"#),
        Err(Error::Format(message)) if message.contains("match index 2 out of range")
    ));
}

#[test]
fn format_tilde_regex_without_selector_emits_nothing() {
    check(r#"(fmt "%~" "(a)(b)" "xxabyyabzz")"#, r#""""#);
    check(
        r#"(fmt "before %~it's %1" "(a)(b)" "ab")"#,
        r#""before it's ab""#,
    );
}

#[test]
fn format_tilde_regex_no_match_emits_f() {
    check(r#"(fmt "%~%1" "^x" "abc")"#, r#""f""#);
    check(r#"(fmt "%~%1.1" "^x" "abc")"#, r#""f""#);
    check(r#"(fmt "%~%2" "^x(a)" "abc")"#, r#""f""#);
    check(r#"(fmt "%~%2.1" "^x" "abc")"#, r#""f""#);
}

#[test]
fn format_tilde_regex_optional_group_emits_underscore() {
    check(r#"(fmt "%~%1.2" "(x)?y" "y")"#, r#""_""#);
    check(r#"(fmt "%~%2" "(x)?y" "y")"#, r#""_""#);
    check(r#"(fmt "%~%1.1" "(x)?y" "y")"#, r#""y""#);
}

#[test]
fn regex_matches_scalars_while_string_indexing_uses_graphemes() {
    // Rust's regex matches Unicode scalar values: `(.)` captures a single scalar —
    // the base emoji — splitting the 👍🏽 grapheme (base + skin-tone modifier = 2
    // scalars, 1 grapheme). String indexing is grapheme-based: [1] yields it whole.
    check(r#"(fmt "%~%2" "(.)" "👍🏽")"#, r#""👍""#);
    check(r#"(let s "👍🏽") (expect s[1] "👍🏽")"#, r#"t"#);
}

#[test]
fn format_tilde_regex_identifier_regex_argument() {
    check(
        r#"(let regex "mgU~^(.*) ") (fmt "%~%2" regex "Hello the world")"#,
        r#""Hello""#,
    );
}

#[test]
fn format_tilde_regex_plain_specifiers_still_work() {
    check(r#"(fmt "100%% %~%1" "\\d+" "x42y")"#, r#""100% 42""#);
    check(r#"(fmt "user %~%1" "u~^(\\w+)" "alice")"#, r#""user alice""#);
}

#[test]
fn format_tilde_regex_selectors_require_pending_match() {
    assert!(matches!(
        run(r#"(fmt "%1" "x")"#),
        Err(Error::Format(message)) if message == "capture selector without preceding %~"
    ));
    assert!(matches!(
        run(r#"(fmt "%2.1" "x")"#),
        Err(Error::Format(message)) if message.contains("without preceding %~")
    ));
}

/// Every `FormatError` sub-message is bare: the category prefix comes from the
/// `Display` arm alone. This walks the whole error-producing surface of `fmt` and
/// asserts the rendered text carries exactly one `FormatError: `, so a site that
/// bakes the prefix into the sub-message is caught by a rendered-text assertion
/// rather than only by a substring check on the inner string.
#[test]
fn format_errors_never_repeat_the_category_prefix() {
    let sources = [
        r#"(fmt "%1" "x")"#,
        r#"(fmt "%2.1" "x")"#,
        r#"(fmt "%3d" 5)"#,
        r#"(fmt "%0" 5)"#,
        r#"(fmt "%0.1" "x")"#,
        r#"(fmt "%1.0" "x")"#,
        r#"(fmt "%1.1" 5)"#,
        r#"(fmt "%~%0" "^a$" "a")"#,
        r#"(fmt "%~%0.1" "^a$" "a")"#,
        r#"(fmt "%~%1.0" "^a$" "a")"#,
        r#"(fmt "%~%2.1" "^a$" "a")"#,
        r#"(fmt "%~%1.2" "^a$" "a")"#,
        r#"(fmt "%~%1." "^a$" "a")"#,
        r#"(fmt "%~%9" "^a$" "a")"#,
        r#"(fmt "%~%1.9" "^a$" "a")"#,
        r#"(fmt "x%")"#,
        r#"(fmt "%z" 5)"#,
        r#"(fmt "%7b" 5)"#,
        r#"(fmt "%9h" 5)"#,
        r#"(fmt "%d" "x")"#,
        r#"(fmt "%f" "x")"#,
        r#"(fmt "%q" 5)"#,
        r#"(fmt "%~" 5)"#,
        r#"(fmt "%j" (fn (x) x))"#,
        r#"(fmt "%j" +Inf)"#,
        r#"(fmt "%d")"#,
        r#"(fmt "%d" 1 2)"#,
        r#"(fmt "%d %d" 1)"#,
    ];
    for source in sources {
        let error = match run(source) {
            Err(error) => error,
            Ok(_) => panic!("{source} was expected to fail"),
        };
        let rendered = error.to_string();
        assert!(
            !rendered.contains("FormatError: FormatError:"),
            "{source} rendered as {rendered}"
        );
        if matches!(error, Error::Format(_)) {
            assert_eq!(
                rendered.matches("FormatError: ").count(),
                1,
                "{source} rendered as {rendered}"
            );
        }
    }
}

/// A string nested in a composite is rendered with the reader's own six
/// escapes, not JSON's, so `%s` output stays readable by `eval`. The two
/// escape sets differ only below 0x1F. This test takes the control character
/// from a file, which is how one arrives in practice; a `\u{1}` escape covers
/// the same ground from source, in the test below.
#[test]
fn nested_percent_s_escapes_with_the_readers_own_escapes() {
    let path = env::temp_dir().join(format!("small_lisp_nested_s_{}.bin", std::process::id()));
    fs::write(&path, b"a\x01b").unwrap();
    let path = path.display().to_string();
    let read = format!(r#"(io.read (io.open "file:{path}?mode=r"))"#);

    // A control character has to be inside a *nested* string, which is why this
    // goes through a file rather than a literal.
    let value = run(&format!(
        r#"(use "io")
           (let s {read})
           (eq (eval (fmt "%s" [s]))[1] s)"#
    ))
    .unwrap();
    assert!(matches!(value, Value::Bool(true)));

    // The rendered form carries no JSON escape, and %j still does.
    let rendered = run(&format!(
        r#"(use "io")
           (let s {read})
           (fmt "%s" [s s])"#
    ))
    .unwrap();
    assert!(!matches!(&rendered, Value::Str(text) if text.contains("\\u00")));

    let json = run(&format!(
        r#"(use "io")
           (let s {read})
           (fmt "%j" [s s])"#
    ))
    .unwrap();
    assert!(matches!(&json, Value::Str(text) if text.contains("\\u0001")));

    fs::remove_file(path).unwrap();
}

/// A `"..."` string accepts exactly the six documented escapes. Anything else
/// after a backslash is a `ParseError` naming the offending character, so a
/// stray backslash cannot silently swallow the character that follows it.
#[test]
fn unknown_string_escapes_are_a_parse_error() {
    for (source, message) in [
        (r#""\q""#, "unknown escape sequence \\q"),
        (r#""\0""#, "unknown escape sequence \\0"),
        (r#""\x41""#, "unknown escape sequence \\x"),
        (r#""\b""#, "unknown escape sequence \\b"),
        (r#""\f""#, "unknown escape sequence \\f"),
        (r#""\e""#, "unknown escape sequence \\e"),
        (r#""\a\zb""#, "unknown escape sequence \\a"),
    ] {
        assert!(
            matches!(run(source), Err(Error::Parse(actual)) if actual == message),
            "{source} was accepted"
        );
    }

    // The six accepted escapes still decode. %q renders each back so the
    // decoded character is visible without embedding a control byte here.
    for (escape, expected) in [
        (r#""\n""#, r#""\n""#),
        (r#""\t""#, r#""\t""#),
        (r#""\r""#, r#""\r""#),
        (r#""\\""#, r#""\\""#),
        (r#""\"""#, r#""\"""#),
        (r#""\'""#, r#""'""#),
    ] {
        let value = run(&format!(r#"(fmt "%q" {escape})"#)).unwrap();
        let Value::Str(text) = value else {
            panic!("{escape} did not render as a string");
        };
        assert_eq!(text, expected, "{escape} decoded wrongly");
    }

    // A raw string takes no escapes, so the backslash and following character
    // are retained verbatim.
    for (source, expected) in [
        (r#"'ab\ncd'"#, "ab\\ncd"),
        (r#"'ab\qcd'"#, "ab\\qcd"),
        (r#"'a\tb'"#, "a\\tb"),
    ] {
        assert!(
            matches!(run(source), Ok(Value::Str(value)) if value == expected),
            "raw string {source} should produce {expected:?}"
        );
    }
    assert!(matches!(
        run("(not 'it''s')"),
        Err(Error::Arity(message)) if message == "not expects 1 arguments, got 2"
    ));
}

/// `\u{HEX}` names one Unicode scalar value. `HEX` is one or more hexadecimal
/// digits in either case with no fixed width, so every way of writing a code
/// point is accepted. This is the only way to write a control character below
/// 0x20 from source.
#[test]
fn unicode_escapes_name_one_code_point() {
    for (source, expected) in [
        (r#""\u{41}""#, "A"),
        (r#""\u{00E9}""#, "é"),
        (r#""\u{1F600}""#, "😀"),
        (r#""\u{1f600}""#, "😀"),
        (r#""\u{0041}""#, "A"),
        (r#""\u{0}""#, "\u{0}"),
        (r#""\u{7f}""#, "\u{7f}"),
        (r#""\u{10FFFF}""#, "\u{10FFFF}"),
        (r#""\u{10fffe}""#, "\u{10FFFE}"),
        (r#""\u{d7ff}""#, "\u{D7FF}"),
        (r#""\u{e000}""#, "\u{E000}"),
        (r#""a\u{41}b""#, "aAb"),
    ] {
        let value = run(source).unwrap_or_else(|_| panic!("{source} was rejected"));
        let Value::Str(text) = value else {
            panic!("{source} did not produce a string");
        };
        assert_eq!(text, expected, "{source} decoded wrongly");
    }
}

/// A `\u{...}` naming a value that is not a Unicode scalar is a `ParseError`:
/// above the last code point, or in the surrogate range.
#[test]
fn unicode_escapes_reject_non_scalar_values() {
    for (source, message) in [
        (
            r#""\u{110000}""#,
            "\\u{110000} is above the last code point 10FFFF",
        ),
        (
            r#""\u{FFFFFF}""#,
            "\\u{FFFFFF} is above the last code point 10FFFF",
        ),
        (
            r#""\u{FFFFFFFFFFFFFFFF}""#,
            "\\u{FFFFFFFFFFFFFFFF} is above the last code point 10FFFF",
        ),
        (
            r#""\u{D800}""#,
            "\\u{D800} is in the surrogate range D800 to DFFF, which is not a character",
        ),
        (
            r#""\u{d800}""#,
            "\\u{d800} is in the surrogate range D800 to DFFF, which is not a character",
        ),
        (
            r#""\u{DBFF}""#,
            "\\u{DBFF} is in the surrogate range D800 to DFFF, which is not a character",
        ),
        (
            r#""\u{DC00}""#,
            "\\u{DC00} is in the surrogate range D800 to DFFF, which is not a character",
        ),
        (
            r#""\u{DFFF}""#,
            "\\u{DFFF} is in the surrogate range D800 to DFFF, which is not a character",
        ),
    ] {
        assert!(
            matches!(run(source), Err(Error::Parse(actual)) if actual == message),
            "{source} was accepted"
        );
    }
    let malformed = "a unicode escape must be written \\u{HEX}";
    for source in [
        r#""\u{ }""#,
        r#""\u41""#,
        r#""\u{41""#,
        r#""\u{4 1}""#,
        r#""\u{41x}""#,
        r#""\u{ZZ}""#,
        r#""\u{-1}""#,
    ] {
        assert!(
            matches!(run(source), Err(Error::Parse(actual)) if actual == malformed),
            "{source} was accepted"
        );
    }
    assert!(matches!(
        run(r#""\u{}""#),
        Err(Error::Parse(message)) if message == "a unicode escape needs at least one digit"
    ));
}

#[test]
fn integer_and_float_formatters_report_documented_type_errors() {
    for (source, expected) in [
        (
            r#"(fmt "%d" 1.0)"#,
            "FormatError: FormatTypeError: %d expects integer",
        ),
        (
            r#"(fmt "%d" "x")"#,
            "FormatError: FormatTypeError: %d expects integer",
        ),
        (
            r#"(fmt "%f" t)"#,
            "FormatError: FormatTypeError: %f expects number",
        ),
        (
            r#"(fmt "%f" "x")"#,
            "FormatError: FormatTypeError: %f expects number",
        ),
    ] {
        let error = match run(source) {
            Err(error) => error,
            Ok(_) => panic!("{source} should fail"),
        };
        assert_eq!(error.to_string(), expected, "{source}");
    }
}

/// `lisp_string` is the one place a string becomes source, and it backs `%q`,
/// `%x` and a nested `%s`. Its contract is that what it writes is a string
/// literal a person can copy and the reader can read back. Escaping every
/// control character as `\u{HEX}` is what makes that hold: a raw control byte
/// does survive `eval`, but it is invisible and unquotable in an editor, and
/// the C1 block can move a cursor or erase a line on a terminal.
#[test]
fn every_control_character_round_trips_through_q_x_and_nested_s() {
    let mut code_points: Vec<u32> = (0x00..=0x1F).collect();
    code_points.push(0x7F);
    code_points.extend(0x80..=0x9F);
    assert_eq!(code_points.len(), 65, "the C0 block, DEL and the C1 block");

    for code_point in code_points {
        let literal = format!(r#""\u{{{code_point:X}}}""#);
        let program = format!(
            r#"(and
                 (eq (eval (fmt "%q" {literal})) {literal})
                 (eq (eval (fmt "%x" {literal})) {literal})
                 (eq (eval (fmt "%s" [{literal}]))[1] {literal}))"#
        );
        let value = run(&program)
            .unwrap_or_else(|e| panic!("U+{code_point:04X} raised {e:?} instead of a value"));
        assert!(
            matches!(value, Value::Bool(true)),
            "U+{code_point:04X} did not read back through %q, %x and a nested %s"
        );
    }
}

/// The emitted form is pinned so a future change cannot quietly go back to a
/// raw byte, and so the short escapes are shown to survive.
#[test]
fn control_characters_are_rendered_as_unicode_escapes() {
    let cases: &[(&str, &str)] = &[
        (r#""\u{0}""#, r#""\u{0}""#),
        (r#""\u{1}""#, r#""\u{1}""#),
        (r#""\u{8}""#, r#""\u{8}""#),
        // 09, 0A and 0D are line feed's neighbours but have their own escapes.
        (r#""\t""#, r#""\t""#),
        (r#""\n""#, r#""\n""#),
        (r#""\r""#, r#""\r""#),
        (r#""\u{b}""#, r#""\u{B}""#),
        (r#""\u{c}""#, r#""\u{C}""#),
        (r#""\u{1f}""#, r#""\u{1F}""#),
        (r#""\u{7f}""#, r#""\u{7F}""#),
        (r#""\u{80}""#, r#""\u{80}""#),
        (r#""\u{9b}""#, r#""\u{9B}""#),
        (r#""\u{9f}""#, r#""\u{9F}""#),
    ];
    for (specifier, name) in [("%q", "q"), ("%x", "x")] {
        for &(literal, expected) in cases {
            let Value::Str(rendered) = run(&format!(r#"(fmt "{specifier}" {literal})"#)).unwrap()
            else {
                panic!("%{name} of {literal} was not a string");
            };
            assert_eq!(rendered, expected, "%{name} of {literal}");
            assert!(
                !rendered.chars().any(char::is_control),
                "%{name} of {literal} left a raw control byte: {rendered:?}"
            );
        }
    }
}

/// Escaping stops at the C1 block. Everything above it is written as itself, so
/// an accented letter, an emoji, a no-break space and a private use character
/// stay readable rather than turning into a wall of hex.
#[test]
fn printable_characters_are_rendered_literally() {
    for (specifier, name) in [("%q", "q"), ("%x", "x")] {
        // A printable character needs no escape of its own, so the rendered form
        // is the content wrapped in quotes and nothing else.
        for literal in [r#""é""#, r#""😀""#] {
            let Value::Str(rendered) = run(&format!(r#"(fmt "{specifier}" {literal})"#)).unwrap()
            else {
                panic!("%{name} of {literal} was not a string");
            };
            let Value::Str(content) = run(&format!(r#"(fmt "%s" {literal})"#)).unwrap() else {
                panic!("%s of {literal} was not a string");
            };
            assert_eq!(rendered, format!("\"{content}\""), "%{name} of {literal}");
        }
        // Quote and backslash are the two printable characters that do need an
        // escape, and their own escapes are not unicode escapes.
        let Value::Str(rendered) = run(&format!(r#"(fmt "{specifier}" "a\"b\\c")"#)).unwrap() else {
            panic!("%{name} of a quote and a backslash was not a string");
        };
        assert_eq!(
            rendered, r#""a\"b\\c""#,
            "%{name} of a quote and a backslash"
        );
        // U+00A0, U+00AD and U+10FFFF are not control characters, so they pass
        // through untouched, and they still read back.
        for code_point in [0xA0u32, 0xAD, 0x10FFFF] {
            let literal = format!(r#""\u{{{code_point:X}}}""#);
            let Value::Str(rendered) = run(&format!(r#"(fmt "{specifier}" {literal})"#)).unwrap()
            else {
                panic!("%{name} of U+{code_point:04X} was not a string");
            };
            // The character is written as itself, not spelled out in hex.
            let ch = char::from_u32(code_point).unwrap();
            assert_eq!(
                rendered,
                format!("\"{ch}\""),
                "%{name} rewrote U+{code_point:04X}"
            );
            assert!(
                !rendered.contains("u{"),
                "%{name} escaped U+{code_point:04X} into hex"
            );
            let value = run(&format!(
                r#"(eq (eval (fmt "{specifier}" {literal})) {literal})"#
            ))
            .unwrap();
            assert!(matches!(value, Value::Bool(true)));
        }
    }
}

/// Now that source can hold a control character, the interpolations that emit
/// one raw all read back, because the reader takes a control byte literally.
/// `%j` is the exception: it writes the brace-less JSON form, which the reader
/// does not accept, so it fails loudly instead of reading back wrong.
#[test]
fn control_characters_round_trip_through_every_specifier_but_json() {
    for specifier in ["%q", "%x"] {
        let value = run(&format!(
            r#"(eq (eval (fmt "{specifier}" "\u{{1}}\u{{1F600}}")) "\u{{1}}\u{{1F600}}")"#
        ))
        .unwrap();
        assert!(
            matches!(value, Value::Bool(true)),
            "{specifier} did not read back"
        );
    }
    let nested = run(r#"(eq (eval (fmt "%s" ["\u{1}"]))[1] "\u{1}")"#).unwrap();
    assert!(matches!(nested, Value::Bool(true)));
    let json = run(r#"(eval (fmt "%j" "\u{1}"))"#);
    assert!(
        matches!(json, Err(Error::Parse(message)) if message.contains("unicode escape")),
        "%j should be unreadable"
    );
}

#[test]
fn format_tilde_regex_index_bounds_errors() {
    assert!(matches!(
        run(r#"(fmt "%~%0" "^a$" "a")"#),
        Err(Error::Format(message)) if message.contains("capture index must be at least 1")
    ));
    assert!(matches!(
        run(r#"(fmt "%~%0.1" "^a$" "a")"#),
        Err(Error::Format(message)) if message.contains("match index must be at least 1")
    ));
    assert!(matches!(
        run(r#"(fmt "%~%1.0" "^a$" "a")"#),
        Err(Error::Format(message)) if message.contains("capture index must be at least 1")
    ));
    assert!(matches!(
        run(r#"(fmt "%~%2.1" "^a$" "a")"#),
        Err(Error::Format(message)) if message.contains("match index 2 out of range")
    ));
    assert!(matches!(
        run(r#"(fmt "%~%1.2" "^a$" "a")"#),
        Err(Error::Format(message)) if message.contains("capture index 2 out of range")
    ));
    assert!(matches!(
        run(r#"(fmt "%~%1." "^a$" "a")"#),
        Err(Error::Format(message)) if message.contains("invalid capture index")
    ));
    assert!(matches!(
        run(r#"(fmt "%~%1.x" "^a$" "a")"#),
        Err(Error::Format(message)) if message.contains("invalid capture index")
    ));
}

#[test]
fn format_tilde_regex_type_and_arity_errors() {
    assert!(matches!(
        run(r#"(fmt "%~%1" 42 "x")"#),
        Err(Error::Format(message)) if message.contains("%~ expects string")
    ));
    assert!(matches!(
        run(r#"(fmt "%~%1" "a" 42)"#),
        Err(Error::Format(message)) if message.contains("%~ expects string")
    ));
    assert!(matches!(
        run(r#"(fmt "%~" "a")"#),
        Err(Error::Format(message)) if message == "FormatArityError"
    ));
    assert!(matches!(
        run(r#"(fmt "%~%1" "a" "b" "c")"#),
        Err(Error::Format(message)) if message == "FormatArityError"
    ));
    assert!(matches!(
        run(r#"(fmt "%~%1" "(unclosed" "x")"#),
        Err(Error::Regex(message)) if message.contains("invalid regex")
    ));
}

#[test]
fn a_rejected_fixed_width_names_the_specifier_and_the_accepted_widths() {
    for (source, specifier) in [
        (r#"(fmt "%7b" 5)"#, "%7b"),
        (r#"(fmt "%9h" 5)"#, "%9h"),
        (r#"(fmt "%0b" 5)"#, "%0b"),
        (r#"(fmt "%1b" 5)"#, "%1b"),
        (r#"(fmt "%5b" 5)"#, "%5b"),
        (r#"(fmt "%128b" 5)"#, "%128b"),
        (r#"(fmt "%0h" 5)"#, "%0h"),
        (r#"(fmt "%5h" 5)"#, "%5h"),
        (r#"(fmt "%65h" 5)"#, "%65h"),
    ] {
        let error = match run(source) {
            Err(error) => error,
            Ok(_) => panic!("{source} was expected to fail"),
        };
        assert_eq!(
            error.to_string(),
            format!("FormatError: FormatTypeError: {specifier} supports widths 8, 16, 32, or 64")
        );
    }
    // The four accepted widths still work, so the rejection is about the width
    // and not about the specifier pair.
    check(r#"(fmt "%8b" 5)"#, r#""00000101""#);
    check(r#"(fmt "%16b" 5)"#, r#""0000000000000101""#);
    check(r#"(fmt "%32b" 5)"#, r#""00000000000000000000000000000101""#);
    check(
        r#"(fmt "%64b" 5)"#,
        r#""0000000000000000000000000000000000000000000000000000000000000101""#,
    );
    check(r#"(fmt "%8h" 5)"#, r#""05""#);
    check(r#"(fmt "%64h" 5)"#, r#""0000000000000005""#);
}

#[test]
fn a_width_too_large_to_parse_is_reported_without_a_sub_message() {
    for source in [
        r#"(fmt "%99999999999999999999b" 5)"#,
        r#"(fmt "%99999999999999999999h" 5)"#,
    ] {
        let error = match run(source) {
            Err(error) => error,
            Ok(_) => panic!("{source} was expected to fail"),
        };
        // A width that cannot be parsed is reported as a bare message: it
        // names neither the specifier nor the accepted widths, which is the
        // one place the format taxonomy splits. See the todo item.
        assert_eq!(error.to_string(), "FormatError: invalid binary width");
    }
    // The boundary: the largest 64 bit unsigned value still parses, so it is
    // the accepted-sizes message and not this one. The two shapes are adjacent.
    let error = match run(r#"(fmt "%18446744073709551615b" 5)"#) {
        Err(error) => error,
        Ok(_) => panic!("the largest width is not an accepted size"),
    };
    assert_eq!(
        error.to_string(),
        "FormatError: FormatTypeError: %18446744073709551615b supports widths 8, 16, 32, or 64"
    );
    let error = match run(r#"(fmt "%18446744073709551616b" 5)"#) {
        Err(error) => error,
        Ok(_) => panic!("one past the largest width does not parse"),
    };
    assert_eq!(error.to_string(), "FormatError: invalid binary width");
}

#[test]
fn only_binary_and_hexadecimal_forms_accept_a_fixed_width() {
    for source in [
        r#"(fmt "%8d" 5)"#,
        r#"(fmt "%8s" 5)"#,
        r#"(fmt "%8o" 5)"#,
        r#"(fmt "%8x" 5)"#,
        r#"(fmt "%8f" 5)"#,
        r#"(fmt "%8j" 5)"#,
        r#"(fmt "%8t" 5)"#,
        r#"(fmt "%8v" 5)"#,
        r#"(fmt "%8q" 5)"#,
    ] {
        let error = match run(source) {
            Err(error) => error,
            Ok(_) => panic!("{source} was expected to fail"),
        };
        // A width on any other specifier is a capture selector, not a width.
        assert_eq!(
            error.to_string(),
            "FormatError: capture selector without preceding %~"
        );
    }
}

#[test]
fn a_negative_value_is_two_complement_truncated_to_the_requested_width() {
    check(r#"(fmt "%8b" -1)"#, r#""11111111""#);
    check(r#"(fmt "%16b" -1)"#, r#""1111111111111111""#);
    check(r#"(fmt "%32b" -1)"#, r#""11111111111111111111111111111111""#);
    check(
        r#"(fmt "%64b" -1)"#,
        r#""1111111111111111111111111111111111111111111111111111111111111111""#,
    );
    // Truncation, not saturation: 256 is zero in 8 bits, and -256 keeps its
    // sign bit together with every bit above the requested width.
    check(r#"(fmt "%8b" 256)"#, r#""00000000""#);
    check(r#"(fmt "%8b" -256)"#, r#""00000000""#);
    check(r#"(fmt "%16b" -256)"#, r#""1111111100000000""#);
}

#[test]
fn octal_of_a_negative_integer_is_its_full_64_bit_two_complement() {
    check(r#"(fmt "%o" -1)"#, r#""1777777777777777777777""#);
    check(r#"(fmt "%o" 0)"#, r#""0""#);
    check(r#"(fmt "%o" 8)"#, r#""10""#);
}
