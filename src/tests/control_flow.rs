use super::{check, run};
use crate::*;

#[test]
fn break_inside_match_inside_loop_exits_the_loop() {
    check(
        "(let i 0) (let r (loop (set i (add i 1)) (match (eq i 5) (break i) t ()))) r",
        "5",
    );
    assert!(matches!(
        run("(match f 1 t (break 42))"),
        Err(Error::BreakOutside)
    ));
    assert!(matches!(
        run("(match f 1 t (continue))"),
        Err(Error::ContinueOutsideLoop)
    ));
    check(
        "(let r (loop (loop (match t (break \"inner\") t ())) (break \"outer\"))) r",
        "\"outer\"",
    );
    check(
        "(let a 1) (match (eq a 1) \"Case1\" (eq a 1) \"Case2\" t _)",
        "\"Case1\"",
    );
}

#[test]
fn and_of_true_and_true_is_true() {
    check(r#"(and t t)"#, r#"t"#);
}

#[test]
fn and_of_true_and_false_is_false() {
    check(r#"(and t f)"#, r#"f"#);
}

#[test]
fn and_of_false_and_true_is_false() {
    check(r#"(and f t)"#, r#"f"#);
}

#[test]
fn and_of_false_and_false_is_false() {
    check(r#"(and f f)"#, r#"f"#);
}

#[test]
fn and_returns_last_truthy_operand() {
    check(r#"(and 1 2)"#, r#"2"#);
}

#[test]
fn and_returns_falsy_zero_operand() {
    check(r#"(and 1 0)"#, r#"0"#);
}

#[test]
fn and_returns_zero_when_first_operand_is_falsy() {
    check(r#"(and 0 2)"#, r#"0"#);
}

#[test]
fn and_treats_float_zero_as_falsy() {
    check(r#"(and 0.0 2)"#, r#"0.0"#);
}

#[test]
fn and_treats_null_as_falsy() {
    check(r#"(and 1 _)"#, r#"_"#);
}

#[test]
fn and_returns_string_operand() {
    check(r#"(and 1 "foo")"#, r#""foo""#);
}

#[test]
fn or_of_true_and_true_is_true() {
    check(r#"(or t t)"#, r#"t"#);
}

#[test]
fn or_of_true_and_false_is_true() {
    check(r#"(or t f)"#, r#"t"#);
}

#[test]
fn or_of_false_and_true_is_true() {
    check(r#"(or f t)"#, r#"t"#);
}

#[test]
fn or_of_false_and_false_is_false() {
    check(r#"(or f f)"#, r#"f"#);
}

#[test]
fn or_returns_first_truthy_integer() {
    check(r#"(or 1 2)"#, r#"1"#);
}

#[test]
fn or_skips_zero_and_returns_second_operand() {
    check(r#"(or 0 2)"#, r#"2"#);
}

#[test]
fn or_treats_float_zero_as_falsy() {
    check(r#"(or 0.0 2)"#, r#"2"#);
}

#[test]
fn or_treats_null_as_falsy() {
    check(r#"(or _ 2)"#, r#"2"#);
}

#[test]
fn or_returns_string_operand() {
    check(r#"(or f "foo")"#, r#""foo""#);
}

#[test]
fn not_negates_true_to_false() {
    check(r#"(not t)"#, r#"f"#);
}

#[test]
fn not_negates_false_to_true() {
    check(r#"(not f)"#, r#"t"#);
}

#[test]
fn not_treats_zero_as_false() {
    check(r#"(not 0)"#, r#"t"#);
}

#[test]
fn not_treats_float_zero_as_false() {
    check(r#"(not 0.0)"#, r#"t"#);
}

#[test]
fn not_treats_null_as_false() {
    check(r#"(not _)"#, r#"t"#);
}

#[test]
fn not_treats_one_as_true() {
    check(r#"(not 1)"#, r#"f"#);
}

#[test]
fn not_treats_negative_integer_as_true() {
    check(r#"(not -1)"#, r#"f"#);
}

#[test]
fn not_treats_strings_as_true() {
    check(r#"(not "foo")"#, r#"f"#);
}

#[test]
fn not_treats_arrays_as_true() {
    check(r#"(not [])"#, r#"f"#);
}

#[test]
fn and_skips_second_operand_after_false() {
    check(r#"(and f (div 1 0))"#, r#"f"#);
}

#[test]
fn or_skips_second_operand_after_true() {
    check(r#"(or t (div 1 0))"#, r#"t"#);
}

#[test]
fn if_without_else_returns_value_on_true() {
    check(r#"(if t 42)"#, r#"42"#);
}

#[test]
fn if_without_else_returns_null_on_false() {
    check(r#"(if f 42)"#, r#"_"#);
}

#[test]
fn if_true_selects_then_branch() {
    check(r#"(if t 42 21)"#, r#"42"#);
}

#[test]
fn if_false_selects_else_branch() {
    check(r#"(if f 42 21)"#, r#"21"#);
}

#[test]
fn if_treats_zero_as_false() {
    check(r#"(if 0 "zero" "non-zero")"#, r#""non-zero""#);
}

#[test]
fn if_treats_float_zero_as_false() {
    check(r#"(if 0.0 "zero" "non-zero")"#, r#""non-zero""#);
}

#[test]
fn if_treats_null_as_false() {
    check(r#"(if _ "value" "null")"#, r#""null""#);
}

#[test]
fn if_treats_strings_as_true() {
    check(r#"(if "foo" "yes" "no")"#, r#""yes""#);
}

#[test]
fn match_uses_value_of_first_true_condition() {
    check(
        r#"(match (eq 1 2) "no" (eq 1 1) "yes" t "default")"#,
        r#""yes""#,
    );
}

#[test]
fn match_falls_back_to_default_condition() {
    check(
        r#"(match (eq 1 2) "no" (gt 1 2) "greater" t "default")"#,
        r#""default""#,
    );
}

#[test]
fn match_returns_value_of_matching_condition() {
    check(r#"(match (eq 5 5) 42 t 0)"#, r#"42"#);
}

#[test]
fn match_returns_default_when_nothing_matches() {
    check(r#"(match (eq 5 6) 42 t 0)"#, r#"0"#);
}

#[test]
fn loop_break_returns_integer_value() {
    check(
        r#"(loop
    (break 42))"#,
        r#"42"#,
    );
}

#[test]
fn loop_break_returns_string_value() {
    check(
        r#"(loop
    (break "done"))"#,
        r#""done""#,
    );
}

#[test]
fn loop_break_returns_bool_value() {
    check(
        r#"(loop
    (break t))"#,
        r#"t"#,
    );
}

#[test]
fn loop_bare_break_returns_null() {
    check(
        r#"(loop
    (break))"#,
        r#"_"#,
    );
}

#[test]
fn loop_break_inside_if_returns_value() {
    check(
        r#"(loop
    (if t
        (break 42)))"#,
        r#"42"#,
    );
}

#[test]
fn loop_break_after_false_if_returns_next_value() {
    check(
        r#"(loop
    (if f
        (break 42))
    (break 99))"#,
        r#"99"#,
    );
}

#[test]
fn match_is_bindable_as_identifier() {
    check(
        r#"(let match "old")
match"#,
        r#""old""#,
    );
}
