use super::check;

#[test]
fn decimal_floats_do_not_conflict_with_field_access() {
    check("(expect (add 1 0) 1.0 \"integer float test\")", "t");
    check("(let values [10]) values[1]", "10");
}

#[test]
fn non_finite_float_literals_follow_numeric_semantics() {
    check(
        "(expect ($ \"%t:%s\" NaN NaN) \"float:NaN\") (expect ($ \"%t:%s\" +NaN +NaN) \"float:NaN\") (expect ($ \"%t:%s\" Inf Inf) \"float:Inf\") (expect ($ \"%t:%s\" +Inf +Inf) \"float:Inf\") (expect ($ \"%t:%s\" -Inf -Inf) \"float:-Inf\") (expect (eq +NaN NaN) f) (expect (eq +Inf Inf) t) (expect (eq NaN NaN) f) (expect (eq Inf Inf) t) (expect (eq -Inf -Inf) t) (expect (eq 0.0 -0.0) t) (expect (eq 5 5.0) t) (expect (lt 1.0 Inf) t) (expect (lt -Inf 1.0) t) (expect (lt NaN 1.0) f) (expect (gt NaN 1.0) f)",
        "t",
    );
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
