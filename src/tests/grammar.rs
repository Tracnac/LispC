use super::run;
use crate::*;
use regex::Regex;

/// The lexical grammar of a single token, as regular expressions (spec §1.1
/// and §2.1). The integer rule cannot express the 64 bit range check, so the
/// differential sweep below uses only in-range literals.
const INT_RE: &str = r"^[+-]?(?:[0-9]+|0x[0-9A-Fa-f]+|0b[01]+|0o[0-7]+)$";
const FLOAT_RE: &str = r"^[+-]?[0-9]+(?:\.[0-9]+(?:[eE][+-]?[0-9]+)?|[eE][+-]?[0-9]+)$";
const SPECIAL_FLOAT_RE: &str = r"^[+-]?(?:NaN|Inf)$";
const NAME_RE: &str = r"^[A-Za-z][A-Za-z0-9_-]*$";

/// What a one-token source lexes to. The special floats and `_` are Symbol
/// tokens at this stage; the reader maps them to literals afterwards.
#[derive(Debug, PartialEq, Clone, Copy)]
enum Class {
    Int(i64),
    Float(f64),
    SpecialFloat,
    Null,
    Name,
    Rejected,
}

fn classify(src: &str) -> Class {
    let tokens = match lex(src) {
        Ok(tokens) => tokens,
        Err(_) => return Class::Rejected,
    };
    let [token] = tokens.as_slice() else {
        panic!("{src:?} should lex as exactly one token, got {tokens:?}");
    };
    match &token.kind {
        TokKind::Int(n) => Class::Int(*n),
        TokKind::Float(n) => Class::Float(*n),
        TokKind::Symbol(s) => {
            assert_eq!(s, src, "the symbol token should be the whole source");
            match s.as_str() {
                "NaN" | "+NaN" | "-NaN" | "Inf" | "+Inf" | "-Inf" => Class::SpecialFloat,
                "_" => Class::Null,
                _ => Class::Name,
            }
        }
        other => panic!("{src:?} lexes as an unexpected token {other:?}"),
    }
}

#[test]
fn integer_literals_follow_the_int_rule() {
    for (src, value) in [
        ("0", 0),
        ("42", 42),
        ("+42", 42),
        ("-42", -42),
        ("007", 7),
        ("0x1F", 31),
        ("0xff", 255),
        ("0xFF", 255),
        ("-0x10", -16),
        ("+0b101", 5),
        ("0o17", 15),
    ] {
        assert_eq!(classify(src), Class::Int(value), "{src}");
    }
}

#[test]
fn float_literals_follow_the_float_rule() {
    for (src, value) in [
        ("0.0", 0.0),
        ("1.5", 1.5),
        ("-2.5", -2.5),
        ("1e3", 1000.0),
        ("1E3", 1000.0),
        ("1e+3", 1000.0),
        ("1e-3", 0.001),
        ("1.5e2", 150.0),
        ("1.5e-3", 0.0015),
        ("-2.5E+4", -25000.0),
        ("1e0", 1.0),
    ] {
        assert_eq!(classify(src), Class::Float(value), "{src}");
    }
    // A float literal past the f64 range saturates to infinity, the same way
    // the float arithmetic paths do.
    assert_eq!(classify("1e999"), Class::Float(f64::INFINITY));
}

#[test]
fn special_floats_and_null_are_symbols_before_the_reader_maps_them() {
    for src in ["NaN", "+NaN", "-NaN", "Inf", "+Inf", "-Inf"] {
        assert_eq!(classify(src), Class::SpecialFloat, "{src}");
        assert!(
            matches!(run(src), Ok(Value::Float(_))),
            "{src} should read as a float literal"
        );
    }
    assert_eq!(classify("_"), Class::Null);
    assert!(matches!(run("_"), Ok(Value::Null)));
}

#[test]
fn names_follow_the_name_rule() {
    for src in [
        "abc", "x1", "foo-bar", "a_b", "A", "Z9_-", "inf", "nan", "infinity", "e3", "E3", "t", "f",
    ] {
        assert_eq!(classify(src), Class::Name, "{src}");
    }
}

#[test]
fn invalid_runs_are_parse_errors_with_the_runs_name() {
    for src in [
        "1a", "1_000", "0x", "0b", "0o", "0X10", "0B11", "0O17", "a/b", "a=b", "-foo", "_x", "~",
        "+", "-", "+inf", "-nan",
    ] {
        assert!(
            matches!(run(src), Err(Error::Parse(message)) if message == format!("invalid name `{src}`")),
            "{src} should be an invalid-name ParseError"
        );
    }
}

#[test]
fn number_first_claims_keep_their_own_messages() {
    // A digit run followed by e or E claims an exponent, so these are invalid
    // numbers, not invalid names, and are reported against the prefix only.
    for (src, message) in [
        ("1e", "invalid number `1e`"),
        ("1e+", "invalid number `1e+`"),
        ("1e-", "invalid number `1e-`"),
        ("1ea", "invalid number `1e`"),
        ("1element", "invalid number `1e`"),
        ("0b102", "invalid number `0b102`"),
        ("0x1p3", "invalid number `0x1p3`"),
        (
            "99999999999999999999",
            "integer out of range `99999999999999999999`",
        ),
        ("0xFFFFFFFFFFFFFFFF", "integer out of range"),
    ] {
        assert!(
            matches!(run(src), Err(Error::Parse(m)) if m == message),
            "{src} should be ParseError: {message}"
        );
    }
}

#[test]
fn a_dot_is_a_delimiter_not_part_of_a_number_token() {
    assert!(
        matches!(run(".5"), Err(Error::Parse(m)) if m == "unexpected token Dot"),
        ".5 starts with the Dot token"
    );
    // A dot is field access only on a name path, so after a number it is not
    // postfix at all and fails exactly like a leading dot does.
    for src in ["5.", "5.x", "5.e2", "({a: 1}).a", "[1 2].x"] {
        assert!(
            matches!(run(src), Err(Error::Parse(m)) if m == "unexpected token Dot"),
            "{src} should be ParseError: unexpected token Dot"
        );
    }
    // On a name path the dot is postfix, and then the old messages apply.
    assert!(
        matches!(run("a."), Err(Error::Parse(m)) if m == "expected struct field after dot"),
        "a. is a field access missing its field"
    );
    // A literal ends at the last digit of its exponent; the rest is a new token.
    let tokens = lex("1e3abc").unwrap();
    assert!(
        matches!(&tokens[..], [Tok { kind: TokKind::Float(n), .. }, Tok { kind: TokKind::Symbol(s), .. }] if *n == 1000.0 && s == "abc"),
        "1e3abc should be the float 1e3 followed by the name abc"
    );
}

#[test]
fn the_regexes_and_the_lexer_agree() {
    let int = Regex::new(INT_RE).unwrap();
    let float = Regex::new(FLOAT_RE).unwrap();
    let special = Regex::new(SPECIAL_FLOAT_RE).unwrap();
    let name = Regex::new(NAME_RE).unwrap();
    let expected = |src: &str| {
        if int.is_match(src) {
            Class::Int(0)
        } else if float.is_match(src) {
            Class::Float(0.0)
        } else if special.is_match(src) {
            Class::SpecialFloat
        } else if src == "_" {
            Class::Null
        } else if name.is_match(src) {
            Class::Name
        } else {
            Class::Rejected
        }
    };
    let tag = |class: &Class| match class {
        Class::Int(_) => Class::Int(0),
        Class::Float(_) => Class::Float(0.0),
        other => *other,
    };
    let bodies = [
        // integers and radix literals
        "0", "1", "42", "007", "0x0", "0xff", "0xFF", "0xAb", "0XFF", "0b10", "0B10", "0o7", "0O7",
        "0x", "0b", "0o", // floats
        "0.0", "1.5", "1e3", "1E3", "1e+3", "1e-3", "1.5e2", "1.5e-3", "1.5E+4", "1e", "1e+",
        "1e-", "1ea", "1element", // names
        "a", "abc", "x1", "foo-bar", "a_b", "NaN", "Inf", "inf", "nan", "infinity", "e3", "E3",
        "t", "f", // rejected runs
        "1a", "1_000", "a/b", "a=b", "_x", "~", "9x",
    ];
    for sign in ["", "+", "-"] {
        for body in bodies {
            let src = format!("{sign}{body}");
            assert_eq!(
                tag(&classify(&src)),
                tag(&expected(&src)),
                "{src:?} should classify the same way in the lexer and the regexes"
            );
        }
    }
}
