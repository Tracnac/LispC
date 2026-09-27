use super::{check, run};
use crate::*;

#[test]
fn decimal_floats_do_not_conflict_with_field_access() {
    check("(expect (add 1 0) 1.0 \"integer float test\")", "t");
    check("(let values [10]) values[1]", "10");
}

#[test]
fn non_finite_float_literals_follow_numeric_semantics() {
    check(
        "(expect ($ \"%t:%s\" NaN NaN) \"float:NaN\") (expect ($ \"%t:%s\" +NaN +NaN) \"float:NaN\") (expect ($ \"%t:%s\" -NaN -NaN) \"float:NaN\") (expect ($ \"%t:%s\" Inf Inf) \"float:Inf\") (expect ($ \"%t:%s\" +Inf +Inf) \"float:Inf\") (expect ($ \"%t:%s\" -Inf -Inf) \"float:-Inf\") (expect (eq +NaN NaN) f) (expect (eq +Inf Inf) t) (expect (eq NaN NaN) f) (expect (eq Inf Inf) t) (expect (eq -Inf -Inf) t) (expect (eq 0.0 -0.0) t) (expect (eq 5 5.0) t) (expect (lt 1.0 Inf) t) (expect (lt -Inf 1.0) t) (expect (lt NaN 1.0) f) (expect (gt NaN 1.0) f) (expect (eq -NaN NaN) f) (expect (eq -NaN -NaN) f) (expect (lt -NaN 1.0) f) (expect ($ \"%s\" (add -NaN 1.0)) ($ \"%s\" NaN)) (expect ($ \"%s\" (mul -NaN 2.0)) ($ \"%s\" NaN)) (expect ($ \"%s\" (div -NaN 1.0)) ($ \"%s\" NaN))",
        "t",
    );
}

#[test]
fn a_negative_nan_carries_the_sign_bit() {
    // The lexical rule is that a leading `-` negates, which for a NaN means
    // setting the sign bit rather than producing a different number. Nothing in
    // the language can observe that, since every renderer writes any NaN as
    // `NaN`, so the bit is asserted here on the value itself.
    for (src, signed) in [("NaN", false), ("+NaN", false), ("-NaN", true)] {
        let value = run(src).unwrap_or_else(|_| panic!("{src} should be a literal"));
        let Value::Float(n) = value else {
            panic!("{src} should be a float");
        };
        assert!(n.is_nan(), "{src} should be a NaN");
        assert_eq!(
            n.is_sign_negative(),
            signed,
            "{src} should have its sign bit {}",
            if signed { "set" } else { "clear" }
        );
        // However it is spelled, it writes as `NaN` and not as `-NaN`.
        check(&format!(r#"($ "%t:%s" {src} {src})"#), r#""float:NaN""#);
    }
}

#[test]
fn a_negative_nan_is_reserved_like_a_negative_infinity() {
    // -NaN was a name and gave a NameError while -Inf has always been a
    // literal. The asymmetry was syntactic only, and IEEE 754 is what settles
    // it: a NaN does carry a sign, so `-NaN` is the NaN with that sign.
    for src in ["NaN", "+NaN", "-NaN", "Inf", "+Inf", "-Inf"] {
        assert!(
            matches!(run(src), Ok(Value::Float(_))),
            "{src} should be a float literal"
        );
    }
    // Being a literal makes it a reserved word, which is what -Inf already was.
    for src in ["(let -NaN 1)", "(fn (-NaN) 1)", "(let -Inf 1)"] {
        assert!(
            matches!(run(src), Err(Error::Type(_))),
            "{src} should be refused as a binding"
        );
    }
    // Only the capitalised spellings were promoted. The lowercase ones were
    // names before this and are names still, so the float parser must not be
    // reaching them.
    for src in ["-nan", "-inf", "nan", "inf", "-infinity", "e3"] {
        assert!(
            matches!(run(src), Err(Error::Name(_))),
            "{src} should still be a name"
        );
    }
}

#[test]
fn mul_with_float_operand_promotes_to_float() {
    check(r#"($ "%t:%s" (mul 2 2.5) (mul 2 2.5))"#, r#""float:5""#);
}

#[test]
fn sub_with_float_operand_promotes_to_float() {
    check(r#"($ "%t:%s" (sub 10 2.5) (sub 10 2.5))"#, r#""float:7.5""#);
}

#[test]
fn div_fold_keeps_integer_division() {
    check(r#"($ "%t:%s" (div 10 2 4) (div 10 2 4))"#, r#""int:1""#);
}
