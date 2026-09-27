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
        Err(Error::Name(message)) if message.contains("out of bounds")
    ));
    assert!(matches!(
        run(r#""abc"[1.0]"#),
        Err(Error::Type(message)) if message.contains("integer")
    ));
}

#[test]
fn formatting_supports_integer_bases_json_and_debug_output() {
    let value = run("($ \"%b %h %o\" 10 255 8)").unwrap();
    assert!(matches!(value, Value::Str(text) if text == "1010 ff 10"));

    let value = run("($ \"%8h %16h %32h %64h\" 255 255 255 255)").unwrap();
    assert!(matches!(
        value,
        Value::Str(text) if text == "ff 00ff 000000ff 00000000000000ff"
    ));

    let value = run("($ \"%8h %16h %32h %64h\" -1 -1 -1 -1)").unwrap();
    assert!(matches!(
        value,
        Value::Str(text) if text == "ff ffff ffffffff ffffffffffffffff"
    ));

    let value = run("($ \"%8b %16b %32b %64b\" 5 5 5 5)").unwrap();
    assert!(
        matches!(value, Value::Str(text) if text == "00000101 0000000000000101 00000000000000000000000000000101 0000000000000000000000000000000000000000000000000000000000000101")
    );

    let value = run("($ \"%t %t\" 1 [1])").unwrap();
    assert!(matches!(value, Value::Str(text) if text == "int array"));

    let value = run(r#"($ "%s" {name:"Yvan" contact:{gsm:"0102030405"} score:["ok" 2]})"#).unwrap();
    assert!(
        matches!(value, Value::Str(text) if text == r#"{name:"Yvan" contact:{gsm:"0102030405"} score:["ok" 2]}"#)
    );

    let value = run("($ \"%j\" {name: \"Ada\" values: [1 t _]})").unwrap();
    assert!(
        matches!(value, Value::Str(text) if text == r#"{"name":"Ada","values":[1,true,null]}"#)
    );

    let value = run("($ \"%v\" {name: \"Ada\" values: [1 t _]})").unwrap();
    assert!(
        matches!(value, Value::Str(text) if text == r#"Struct({name: Str("Ada"), values: Array([Int(1), Bool(true), Null])})"#)
    );

    let value = run("($ \"%v\" [1 \"x\"])").unwrap();
    assert!(matches!(value, Value::Str(text) if text == r#"Array([Int(1), Str("x")])"#));
}

#[test]
fn formatting_percent_q_quotes_strings_as_lisp_literals() {
    // %s inserts the raw contents; %q wraps them in a parseable Lisp literal.
    let value = run(r#"($ "%s|%q" "hello" "hello")"#).unwrap();
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
        let value = run(&format!("($ \"%q\" {literal})")).unwrap();
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
        let Value::Str(content) = run(&format!("($ \"%s\" {literal})")).unwrap() else {
            panic!("%s of {literal} was not a string");
        };
        let Value::Str(quoted) = run(&format!("($ \"%q\" {literal})")).unwrap() else {
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
    let value = run(r#"(let s "hello") ($ "%q" ^s)"#).unwrap();
    assert!(matches!(value, Value::Str(text) if text == r#""hello""#));
}

#[test]
fn formatting_percent_x_serializes_values_as_lisp_source() {
    let cases: &[(&str, &str)] = &[
        (r#"($ "%x" "hello")"#, r#""hello""#),
        (r#"($ "%x" "hello \"world\"")"#, r#""hello \"world\"""#),
        ("($ \"%x\" 42)", "42"),
        ("($ \"%x\" 3.14)", "3.14"),
        ("($ \"%x\" t)", "t"),
        ("($ \"%x\" f)", "f"),
        ("($ \"%x\" _)", "_"),
        ("($ \"%x\" [1 2 3])", "[1 2 3]"),
        (r#"($ "%x" [1 "hello" t _ 42])"#, r#"[1 "hello" t _ 42]"#),
        (
            r#"($ "%x" {name:"Yvan" age:56})"#,
            r#"{name:"Yvan" age:56}"#,
        ),
        (
            r#"($ "%x" {name:"Yvan" contact:{gsm:"0102030405"} scores:[10 20 30]})"#,
            r#"{name:"Yvan" contact:{gsm:"0102030405"} scores:[10 20 30]}"#,
        ),
        (r#"($ "%x" [1 "hello" [2 3]])"#, r#"[1 "hello" [2 3]]"#),
        (
            r#"($ "%x" {a:[1 {b:[2 [3]]}] c:"e"})"#,
            r#"{a:[1 {b:[2 [3]]}] c:"e"}"#,
        ),
        // Floats must serialize as reader-readiable plain decimals.
        ("($ \"%x\" 2.0)", "2.0"),
        ("($ \"%x\" (pow 10.0 21))", "1000000000000000000000.0"),
        ("($ \"%x\" Inf)", "Inf"),
        ("($ \"%x\" (mul Inf 0.0))", "NaN"),
        ("($ \"%x\" -0.0)", "-0.0"),
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
        let Value::Str(serialized) = run(&format!("($ \"%x\" {literal})")).unwrap() else {
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
        let Value::Str(serialized) = run(&format!("($ \"%x\" {source})")).unwrap() else {
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
    let value = run(r#"(let s "hi") ($ "%x" ^s)"#).unwrap();
    assert!(matches!(value, Value::Str(text) if text == r#""hi""#));
    let value = run(r#"(let a [1 2]) ($ "%x" ^a)"#).unwrap();
    assert!(matches!(value, Value::Str(text) if text == "[1 2]"));
}

#[test]
fn formatting_percent_x_rejects_functions() {
    assert!(matches!(
        run(r#"($ "%x" (fn () 1))"#),
        Err(Error::Format(message)) if message.contains("%x cannot serialize function")
    ));
}

#[test]
fn formatting_percent_q_rejects_non_strings() {
    for src in [
        "($ \"%q\" 5)",
        "($ \"%q\" t)",
        "($ \"%q\" [1 2])",
        "($ \"%q\" {a:1})",
    ] {
        assert!(
            matches!(run(src), Err(Error::Format(message)) if message.contains("%q expects string")),
            "expected type error for {src}"
        );
    }
    assert!(matches!(
        run("($ \"%q\")"),
        Err(Error::Format(message)) if message == "FormatArityError"
    ));
    assert!(matches!(
        run(r#"($ "%q" "a" "b")"#),
        Err(Error::Format(message)) if message == "FormatArityError"
    ));
}

#[test]
fn format_type_and_value_of_null() {
    check(r#"($ "%t:%s" _ _)"#, r#""null:""#);
}

#[test]
fn format_type_and_value_of_true() {
    check(r#"($ "%t:%s" t t)"#, r#""bool:true""#);
}

#[test]
fn format_type_and_value_of_false() {
    check(r#"($ "%t:%s" f f)"#, r#""bool:false""#);
}

#[test]
fn format_type_and_value_of_zero() {
    check(r#"($ "%t:%s" 0 0)"#, r#""int:0""#);
}

#[test]
fn format_type_and_value_of_integer() {
    check(r#"($ "%t:%s" 127 127)"#, r#""int:127""#);
}

#[test]
fn format_type_and_value_of_negative_integer() {
    check(r#"($ "%t:%s" -127 -127)"#, r#""int:-127""#);
}

#[test]
fn format_type_and_value_of_hex_literal() {
    check(r#"($ "%t:%s" 0x7F 127)"#, r#""int:127""#);
}

#[test]
fn format_type_and_value_of_binary_literal() {
    check(r#"($ "%t:%s" 0b01111111 127)"#, r#""int:127""#);
}

#[test]
fn format_type_and_value_of_octal_literal() {
    check(r#"($ "%t:%s" 0o177 127)"#, r#""int:127""#);
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
    check(r#"($ "%t:%s" 0.0 0.0)"#, r#""float:0""#);
}

#[test]
fn format_type_and_value_of_float() {
    check(r#"($ "%t:%s" 1.5 1.5)"#, r#""float:1.5""#);
}

#[test]
fn format_type_and_value_of_negative_float() {
    check(r#"($ "%t:%s" -1.5 -1.5)"#, r#""float:-1.5""#);
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
        let format = format!(r#"($ "%t:%s" {src} {src})"#);
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
        let format = format!(r#"($ "%t:%s" {src} {src})"#);
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
        let format = format!(r#"($ "%t:%s" {src} {src})"#);
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
    for src in ["inf", "nan", "infinity", "-inf", "-nan", "e3", "E3"] {
        assert!(
            matches!(run(src), Err(Error::Name(_))),
            "{src} should still be a name and not a number"
        );
    }
    check(r#"($ "%t:%s" Inf Inf)"#, r#""float:Inf""#);
    check(r#"($ "%t:%s" NaN NaN)"#, r#""float:NaN""#);
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
    check(r#"($ "%t:%s" "text" "text")"#, r#""string:text""#);
}

#[test]
fn format_type_and_value_of_array() {
    check(r#"($ "%t:%s" [1 2 3] [1 2 3])"#, r#""array:[1 2 3]""#);
}

#[test]
fn format_type_and_value_of_function() {
    check(r#"($ "%t:%s" (fn (x) x) (fn (x) x))"#, r#""function:<fn>""#);
}

#[test]
fn format_integer_as_binary() {
    check(r#"($ "%b" 5)"#, r#""101""#);
}

#[test]
fn format_binary_padded_to_eight() {
    check(r#"($ "%8b" 5)"#, r#""00000101""#);
}

#[test]
fn format_negative_binary_padded_to_eight() {
    check(r#"($ "%8b" -5)"#, r#""11111011""#);
}

#[test]
fn format_binary_padded_to_sixteen() {
    check(r#"($ "%16b" 5)"#, r#""0000000000000101""#);
}

#[test]
fn format_negative_binary_padded_to_sixteen() {
    check(r#"($ "%16b" -5)"#, r#""1111111111111011""#);
}

#[test]
fn format_binary_padded_to_thirty_two() {
    check(r#"($ "%32b" 5)"#, r#""00000000000000000000000000000101""#);
}

#[test]
fn format_binary_padded_to_sixty_four() {
    check(
        r#"($ "%64b" 5)"#,
        r#""0000000000000000000000000000000000000000000000000000000000000101""#,
    );
}

#[test]
fn format_minus_one_binary_padded_to_eight() {
    check(r#"($ "%8b" -1)"#, r#""11111111""#);
}

#[test]
fn format_minus_one_binary_padded_to_sixteen() {
    check(r#"($ "%16b" -1)"#, r#""1111111111111111""#);
}

#[test]
fn format_decimal_hex_and_octal_rendering() {
    check(r#"($ "%d %h %o" 127 127 127)"#, r#""127 7f 177""#);
}

#[test]
fn format_mixed_specifiers_render_inline() {
    check(r#"($ "%s %f %t" "text" 1.5 1.5)"#, r#""text 1.5 float""#);
}

#[test]
fn format_struct_as_json() {
    check(
        r#"($ "%j" {name:"Ada" values:[1 t _]})"#,
        r#""{\"name\":\"Ada\",\"values\":[1,true,null]}""#,
    );
}

#[test]
fn format_struct_with_debug_verb() {
    check(
        r#"($ "%v" {name:"Ada" values:[1 t _]})"#,
        r#""Struct({name: Str(\"Ada\"), values: Array([Int(1), Bool(true), Null])})""#,
    );
}

#[test]
fn format_array_with_debug_verb() {
    check(
        r#"($ "%v" [1 "text"])"#,
        r#""Array([Int(1), Str(\"text\")])""#,
    );
}

#[test]
fn format_escaped_percent_sign() {
    check(r#"($ "100%%")"#, r#""100%""#);
}

#[test]
fn format_type_and_value_of_nan() {
    check(r#"($ "%t:%s" NaN NaN)"#, r#""float:NaN""#);
}

#[test]
fn format_type_and_value_of_positive_nan() {
    check(r#"($ "%t:%s" +NaN +NaN)"#, r#""float:NaN""#);
}

#[test]
fn format_type_and_value_of_infinity() {
    check(r#"($ "%t:%s" Inf Inf)"#, r#""float:Inf""#);
}

#[test]
fn format_type_and_value_of_positive_infinity() {
    check(r#"($ "%t:%s" +Inf +Inf)"#, r#""float:Inf""#);
}

#[test]
fn format_type_and_value_of_negative_infinity() {
    check(r#"($ "%t:%s" -Inf -Inf)"#, r#""float:-Inf""#);
}

#[test]
fn format_d_renders_add_result() {
    check(r#"($ "%d" (add 2 3))"#, r#""5""#);
}

#[test]
fn format_d_renders_sub_result() {
    check(r#"($ "%d" (sub 10 3))"#, r#""7""#);
}

#[test]
fn format_d_renders_mul_result() {
    check(r#"($ "%d" (mul 6 7))"#, r#""42""#);
}

#[test]
fn format_d_renders_div_result() {
    check(r#"($ "%d" (div 10 2))"#, r#""5""#);
}

#[test]
fn format_d_renders_integer_division() {
    check(r#"($ "%d" (div 7 2))"#, r#""3""#);
}

#[test]
fn format_d_renders_negative_mod_result() {
    check(r#"($ "%d" (mod -7 2))"#, r#""-1""#);
}

#[test]
fn format_d_renders_pow_result() {
    check(r#"($ "%d" (pow 2 10))"#, r#""1024""#);
}

#[test]
fn format_f_renders_float_division() {
    check(r#"($ "%f" (div 7.0 2.0))"#, r#""3.5""#);
}

#[test]
fn format_f_renders_float_addition() {
    check(r#"($ "%f" (add 1.5 2.0))"#, r#""3.5""#);
}

#[test]
fn format_f_renders_float_subtraction() {
    check(r#"($ "%f" (sub 5.5 2.0))"#, r#""3.5""#);
}

#[test]
fn format_f_renders_float_multiplication() {
    check(r#"($ "%f" (mul 1.75 2.0))"#, r#""3.5""#);
}

#[test]
fn format_f_renders_float_power() {
    check(r#"($ "%f" (pow 1.5 2.0))"#, r#""2.25""#);
}

#[test]
fn format_f_renders_negative_operand_addition() {
    check(r#"($ "%f" (add -1.5 2.0))"#, r#""0.5""#);
}

#[test]
fn format_f_renders_negative_operand_subtraction() {
    check(r#"($ "%f" (sub -1.5 2.0))"#, r#""-3.5""#);
}

#[test]
fn format_f_renders_negative_operand_multiplication() {
    check(r#"($ "%f" (mul -1.5 2.0))"#, r#""-3""#);
}

#[test]
fn format_f_renders_negative_operand_division() {
    check(r#"($ "%f" (div -7.0 2.0))"#, r#""-3.5""#);
}

#[test]
fn format_f_renders_large_integer_without_precision_loss() {
    check(
        r#"($ "%f" 9223372036854775807)"#,
        r#""9223372036854775807""#,
    );
}

#[test]
fn format_f_renders_integer_above_f64_mantissa_exactly() {
    // 2^53 + 1 is not representable in f64; it must not round to 9007199254740992.
    check(r#"($ "%f" 9007199254740993)"#, r#""9007199254740993""#);
}

#[test]
fn format_f_renders_negative_large_integer_exactly() {
    check(r#"($ "%f" -9007199254740993)"#, r#""-9007199254740993""#);
}

#[test]
fn format_f_renders_min_integer_exactly() {
    check(
        r#"($ "%f" -9223372036854775808)"#,
        r#""-9223372036854775808""#,
    );
    check(
        r#"($ "%f" -0x8000000000000000)"#,
        r#""-9223372036854775808""#,
    );
}

#[test]
fn format_f_renders_small_integer_like_decimal() {
    check(r#"($ "%f" 42)"#, r#""42""#);
    check(r#"($ "%f" -127)"#, r#""-127""#);
}

#[test]
fn format_f_renders_promoted_integer_arithmetic_as_float() {
    check(r#"($ "%f" (add 1 2.5))"#, r#""3.5""#);
    check(r#"($ "%f" (div 7 2.0))"#, r#""3.5""#);
}

#[test]
fn format_type_of_integer_plus_float_is_float() {
    check(r#"($ "%t:%s" (add 1 2.5) (add 1 2.5))"#, r#""float:3.5""#);
}

#[test]
fn format_type_of_integer_minus_float_is_float() {
    check(r#"($ "%t:%s" (sub 5 1.5) (sub 5 1.5))"#, r#""float:3.5""#);
}

#[test]
fn format_type_of_integer_times_float_is_float() {
    check(r#"($ "%t:%s" (mul 7 0.5) (mul 7 0.5))"#, r#""float:3.5""#);
}

#[test]
fn format_type_of_integer_divided_by_float_is_float() {
    check(r#"($ "%t:%s" (div 7 2.0) (div 7 2.0))"#, r#""float:3.5""#);
}

#[test]
fn format_type_of_integer_division_is_integer() {
    check(r#"($ "%t:%s" (div 7 2)   (div 7 2))"#, r#""int:3""#);
}

#[test]
fn format_d_renders_bit_and_result() {
    check(r#"($ "%d" (bit-and 0b110 0b101))"#, r#""4""#);
}

#[test]
fn format_d_renders_bit_or_result() {
    check(r#"($ "%d" (bit-or 0b110 0b101))"#, r#""7""#);
}

#[test]
fn format_d_renders_bit_xor_result() {
    check(r#"($ "%d" (bit-xor 0b110 0b101))"#, r#""3""#);
}

#[test]
fn format_d_renders_bit_not_result() {
    check(r#"($ "%d" (bit-not 0b101))"#, r#""-6""#);
}

#[test]
fn format_d_renders_shift_left_result() {
    check(r#"($ "%d" (bit-shl 1 4))"#, r#""16""#);
}

#[test]
fn format_d_renders_shift_right_result() {
    check(r#"($ "%d" (bit-shr 16 2))"#, r#""4""#);
}

#[test]
fn format_tilde_regex_full_and_capture_shorthands() {
    check(
        r#"($ "%~%1" "mgU~^(.*) " "Hello the world")"#,
        r#""Hello ""#,
    );
    check(r#"($ "%~%2" "mgU~^(.*) " "Hello the world")"#, r#""Hello""#);
    check(
        r#"($ "%~it's not %2, it's Good morning" "mgU~^(.*) " "Hello the world")"#,
        r#""it's not Hello, it's Good morning""#,
    );
    check(r#"($ "%~%1" "(a)(b)" "xxabyyabzz")"#, r#""ab""#);
    check(r#"($ "%~%2" "(a)(b)" "xxabyyabzz")"#, r#""a""#);
    check(r#"($ "%~%3" "(a)(b)" "xxabyyabzz")"#, r#""b""#);
}

#[test]
fn format_tilde_regex_match_capture_selectors() {
    check(r#"($ "%~%1.1" "(a)(b)" "xxabyyabzz")"#, r#""ab""#);
    check(r#"($ "%~%1.2" "(a)(b)" "xxabyyabzz")"#, r#""a""#);
    check(r#"($ "%~%1.3" "(a)(b)" "xxabyyabzz")"#, r#""b""#);
    check(r#"($ "%~%2.1" "(a)(b)" "xxabyyabzz")"#, r#""ab""#);
    check(r#"($ "%~%2.2" "(a)(b)" "xxabyyabzz")"#, r#""a""#);
    check(r#"($ "%~%2.3" "(a)(b)" "xxabyyabzz")"#, r#""b""#);
    check(
        r#"($ "%~%1.1 and %1.2 and %2.1" "(a)(b)" "abab")"#,
        r#""ab and a and ab""#,
    );
}

#[test]
fn format_tilde_regex_option_default_gmu() {
    // default gmu: multiline anchors on, case-sensitive
    check(r#"($ "%~%1" "^b$" "a\nb")"#, r#""b""#);
    check(r#"($ "%~%1" "^hello$" "HELLO")"#, r#""f""#);
}

#[test]
fn format_tilde_regex_options_replace_defaults() {
    // explicit options replace the default set
    check(r#"($ "%~%1" "U~^b$" "a\nb")"#, r#""f""#);
    check(r#"($ "%~%1" "m~^b$" "a\nb")"#, r#""b""#);
    check(r#"($ "%~%1" "i~^hello$" "HELLO")"#, r#""HELLO""#);
    check(r#"($ "%~%1" "s~^a.b$" "a\nb")"#, r#""a\nb""#);
    check(r#"($ "%~%1" "x~a b" "ab")"#, r#""ab""#);
    // R: CRLF is a line terminator (with m), unlike m alone
    check(r#"($ "%~%1" "mR~ab$" "ab\r\ncd")"#, r#""ab""#);
    check(r#"($ "%~%1" "m~ab$" "ab\r\ncd")"#, r#""f""#);
}

#[test]
fn format_tilde_regex_g_finds_all_matches() {
    check(r#"($ "%~%2.1" "g~(a)|(b)" "ab")"#, r#""b""#);
    check(r#"($ "%~%2.2" "g~(a)|(b)" "ab")"#, r#""_""#);
}

#[test]
fn format_tilde_regex_without_g_finds_first_match_only() {
    check(r#"($ "%~%1.1" "U~(a)|(b)" "ab")"#, r#""a""#);
    assert!(matches!(
        run(r#"($ "%~%2.1" "U~(a)|(b)" "ab")"#),
        Err(Error::Format(message)) if message.contains("match index 2 out of range")
    ));
}

#[test]
fn format_tilde_regex_without_selector_emits_nothing() {
    check(r#"($ "%~" "(a)(b)" "xxabyyabzz")"#, r#""""#);
    check(
        r#"($ "before %~it's %1" "(a)(b)" "ab")"#,
        r#""before it's ab""#,
    );
}

#[test]
fn format_tilde_regex_no_match_emits_f() {
    check(r#"($ "%~%1" "^x" "abc")"#, r#""f""#);
    check(r#"($ "%~%1.1" "^x" "abc")"#, r#""f""#);
    check(r#"($ "%~%2" "^x(a)" "abc")"#, r#""f""#);
    check(r#"($ "%~%2.1" "^x" "abc")"#, r#""f""#);
}

#[test]
fn format_tilde_regex_optional_group_emits_underscore() {
    check(r#"($ "%~%1.2" "(x)?y" "y")"#, r#""_""#);
    check(r#"($ "%~%2" "(x)?y" "y")"#, r#""_""#);
    check(r#"($ "%~%1.1" "(x)?y" "y")"#, r#""y""#);
}

#[test]
fn regex_matches_scalars_while_string_indexing_uses_graphemes() {
    // Rust's regex matches Unicode scalar values: `(.)` captures a single scalar —
    // the base emoji — splitting the 👍🏽 grapheme (base + skin-tone modifier = 2
    // scalars, 1 grapheme). String indexing is grapheme-based: [1] yields it whole.
    check(r#"($ "%~%2" "(.)" "👍🏽")"#, r#""👍""#);
    check(r#"(let s "👍🏽") (expect s[1] "👍🏽")"#, r#"t"#);
}

#[test]
fn format_tilde_regex_identifier_regex_argument() {
    check(
        r#"(let regex "mgU~^(.*) ") ($ "%~%2" regex "Hello the world")"#,
        r#""Hello""#,
    );
}

#[test]
fn format_tilde_regex_plain_specifiers_still_work() {
    check(r#"($ "100%% %~%1" "\\d+" "x42y")"#, r#""100% 42""#);
    check(r#"($ "user %~%1" "u~^(\\w+)" "alice")"#, r#""user alice""#);
}

#[test]
fn format_tilde_regex_selectors_require_pending_match() {
    assert!(matches!(
        run(r#"($ "%1" "x")"#),
        Err(Error::Format(message)) if message == "capture selector without preceding %~"
    ));
    assert!(matches!(
        run(r#"($ "%2.1" "x")"#),
        Err(Error::Format(message)) if message.contains("without preceding %~")
    ));
}

/// Every `FormatError` sub-message is bare: the category prefix comes from the
/// `Display` arm alone. This walks the whole error-producing surface of `$` and
/// asserts the rendered text carries exactly one `FormatError: `, so a site that
/// bakes the prefix into the sub-message is caught by a rendered-text assertion
/// rather than only by a substring check on the inner string.
#[test]
fn format_errors_never_repeat_the_category_prefix() {
    let sources = [
        r#"($ "%1" "x")"#,
        r#"($ "%2.1" "x")"#,
        r#"($ "%3d" 5)"#,
        r#"($ "%0" 5)"#,
        r#"($ "%0.1" "x")"#,
        r#"($ "%1.0" "x")"#,
        r#"($ "%1.1" 5)"#,
        r#"($ "%~%0" "^a$" "a")"#,
        r#"($ "%~%0.1" "^a$" "a")"#,
        r#"($ "%~%1.0" "^a$" "a")"#,
        r#"($ "%~%2.1" "^a$" "a")"#,
        r#"($ "%~%1.2" "^a$" "a")"#,
        r#"($ "%~%1." "^a$" "a")"#,
        r#"($ "%~%9" "^a$" "a")"#,
        r#"($ "%~%1.9" "^a$" "a")"#,
        r#"($ "x%")"#,
        r#"($ "%z" 5)"#,
        r#"($ "%7b" 5)"#,
        r#"($ "%9h" 5)"#,
        r#"($ "%d" "x")"#,
        r#"($ "%f" "x")"#,
        r#"($ "%q" 5)"#,
        r#"($ "%~" 5)"#,
        r#"($ "%j" (fn (x) x))"#,
        r#"($ "%j" +Inf)"#,
        r#"($ "%d")"#,
        r#"($ "%d" 1 2)"#,
        r#"($ "%d %d" 1)"#,
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
           (eq (eval ($ "%s" [s]))[1] s)"#
    ))
    .unwrap();
    assert!(matches!(value, Value::Bool(true)));

    // The rendered form carries no JSON escape, and %j still does.
    let rendered = run(&format!(
        r#"(use "io")
           (let s {read})
           ($ "%s" [s s])"#
    ))
    .unwrap();
    assert!(!matches!(&rendered, Value::Str(text) if text.contains("\\u00")));

    let json = run(&format!(
        r#"(use "io")
           (let s {read})
           ($ "%j" [s s])"#
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
    for source in [
        r#""\q""#,
        r#""\0""#,
        r#""\x41""#,
        r#""\b""#,
        r#""\f""#,
        r#""\e""#,
        r#""\a\zb""#,
    ] {
        assert!(
            matches!(
                run(source),
                Err(Error::Parse(message)) if message.starts_with("unknown escape sequence")
            ),
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
        let value = run(&format!(r#"($ "%q" {escape})"#)).unwrap();
        let Value::Str(text) = value else {
            panic!("{escape} did not render as a string");
        };
        assert_eq!(text, expected, "{escape} decoded wrongly");
    }

    // A raw string takes no escapes, so a backslash there is just a character.
    for source in [r#"'ab\ncd'"#, r#"'ab\qcd'"#, r#"'a\tb'"#] {
        assert!(run(source).is_ok(), "raw string {source} was rejected");
    }
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
    for (source, expected) in [
        (r#""\u{110000}""#, "above the last code point"),
        (r#""\u{FFFFFF}""#, "above the last code point"),
        (r#""\u{FFFFFFFFFFFFFFFF}""#, "above the last code point"),
        (r#""\u{D800}""#, "surrogate range"),
        (r#""\u{d800}""#, "surrogate range"),
        (r#""\u{DBFF}""#, "surrogate range"),
        (r#""\u{DC00}""#, "surrogate range"),
        (r#""\u{DFFF}""#, "surrogate range"),
    ] {
        assert!(
            matches!(
                run(source),
                Err(Error::Parse(message)) if message.contains(expected)
            ),
            "{source} was accepted"
        );
    }
    for source in [
        r#""\u{}""#,
        r#""\u{ }""#,
        r#""\u41""#,
        r#""\u{41""#,
        r#""\u{41x}""#,
        r#""\u{ZZ}""#,
        r#""\u{-1}""#,
    ] {
        assert!(
            matches!(
                run(source),
                Err(Error::Parse(message)) if message.contains("unicode escape")
            ),
            "{source} was accepted"
        );
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
                 (eq (eval ($ "%q" {literal})) {literal})
                 (eq (eval ($ "%x" {literal})) {literal})
                 (eq (eval ($ "%s" [{literal}]))[1] {literal}))"#
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
            let Value::Str(rendered) = run(&format!(r#"($ "{specifier}" {literal})"#)).unwrap()
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
            let Value::Str(rendered) = run(&format!(r#"($ "{specifier}" {literal})"#)).unwrap()
            else {
                panic!("%{name} of {literal} was not a string");
            };
            let Value::Str(content) = run(&format!(r#"($ "%s" {literal})"#)).unwrap() else {
                panic!("%s of {literal} was not a string");
            };
            assert_eq!(rendered, format!("\"{content}\""), "%{name} of {literal}");
        }
        // Quote and backslash are the two printable characters that do need an
        // escape, and their own escapes are not unicode escapes.
        let Value::Str(rendered) = run(&format!(r#"($ "{specifier}" "a\"b\\c")"#)).unwrap() else {
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
            let Value::Str(rendered) = run(&format!(r#"($ "{specifier}" {literal})"#)).unwrap()
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
                r#"(eq (eval ($ "{specifier}" {literal})) {literal})"#
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
            r#"(eq (eval ($ "{specifier}" "\u{{1}}\u{{1F600}}")) "\u{{1}}\u{{1F600}}")"#
        ))
        .unwrap();
        assert!(
            matches!(value, Value::Bool(true)),
            "{specifier} did not read back"
        );
    }
    let nested = run(r#"(eq (eval ($ "%s" ["\u{1}"]))[1] "\u{1}")"#).unwrap();
    assert!(matches!(nested, Value::Bool(true)));
    let json = run(r#"(eval ($ "%j" "\u{1}"))"#);
    assert!(
        matches!(json, Err(Error::Parse(message)) if message.contains("unicode escape")),
        "%j should be unreadable"
    );
}

#[test]
fn format_tilde_regex_index_bounds_errors() {
    assert!(matches!(
        run(r#"($ "%~%0" "^a$" "a")"#),
        Err(Error::Format(message)) if message.contains("capture index must be at least 1")
    ));
    assert!(matches!(
        run(r#"($ "%~%0.1" "^a$" "a")"#),
        Err(Error::Format(message)) if message.contains("match index must be at least 1")
    ));
    assert!(matches!(
        run(r#"($ "%~%1.0" "^a$" "a")"#),
        Err(Error::Format(message)) if message.contains("capture index must be at least 1")
    ));
    assert!(matches!(
        run(r#"($ "%~%2.1" "^a$" "a")"#),
        Err(Error::Format(message)) if message.contains("match index 2 out of range")
    ));
    assert!(matches!(
        run(r#"($ "%~%1.2" "^a$" "a")"#),
        Err(Error::Format(message)) if message.contains("capture index 2 out of range")
    ));
    assert!(matches!(
        run(r#"($ "%~%1." "^a$" "a")"#),
        Err(Error::Format(message)) if message.contains("invalid capture index")
    ));
    assert!(matches!(
        run(r#"($ "%~%1.x" "^a$" "a")"#),
        Err(Error::Format(message)) if message.contains("invalid capture index")
    ));
}

#[test]
fn format_tilde_regex_type_and_arity_errors() {
    assert!(matches!(
        run(r#"($ "%~%1" 42 "x")"#),
        Err(Error::Format(message)) if message.contains("%~ expects string")
    ));
    assert!(matches!(
        run(r#"($ "%~%1" "a" 42)"#),
        Err(Error::Format(message)) if message.contains("%~ expects string")
    ));
    assert!(matches!(
        run(r#"($ "%~" "a")"#),
        Err(Error::Format(message)) if message == "FormatArityError"
    ));
    assert!(matches!(
        run(r#"($ "%~%1" "a" "b" "c")"#),
        Err(Error::Format(message)) if message == "FormatArityError"
    ));
    assert!(matches!(
        run(r#"($ "%~%1" "(unclosed" "x")"#),
        Err(Error::Regex(message)) if message.contains("invalid regex")
    ));
}
