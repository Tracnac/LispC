use super::{check, run};
use crate::*;

#[test]
fn variadic_folds_preserve_numeric_errors() {
    assert!(matches!(
        run("(add 1 2 9223372036854775807)"),
        Err(Error::Math(message)) if message == "IntegerOverflow"
    ));
    assert!(matches!(
        run("(div 20 2 0)"),
        Err(Error::Math(message)) if message == "DivisionByZero"
    ));
    assert!(matches!(
        run("(bit-or 1 2.0)"),
        Err(Error::Type(message)) if message == "bitwise operations require integers"
    ));
}

#[test]
fn integer_builtins_turn_operator_overflows_into_lisp_errors() {
    for source in [
        "(div -9223372036854775808 -1)",
        "(mod -9223372036854775808 -1)",
        "(bit-shl 4611686018427387904 2)",
    ] {
        assert!(matches!(
            run(source),
            Err(Error::Math(message)) if message == "IntegerOverflow"
        ));
    }
}

#[test]
fn integer_bitwise_operations_work() {
    check("(bit-or (bit-and 0b110 0b101) (bit-xor 0b110 0b101))", "7");
    check("(bit-shr (bit-shl 1 4) 2)", "4");
    check("(bit-not 0b101)", "-6");
}

#[test]
fn bitwise_operations_require_integers_and_valid_shift_counts() {
    for source in [
        "(bit-and 1.0 2)",
        "(bit-or 1 \"2\")",
        "(bit-xor 1 t)",
        "(bit-not _)",
        "(bit-shl 1 2.0)",
        "(bit-shr 4 f)",
    ] {
        assert!(matches!(
            run(source),
            Err(Error::Type(message)) if message == "bitwise operations require integers"
        ));
    }
    assert!(matches!(
        run("(bit-shl 1 64)"),
        Err(Error::Math(message)) if message == "InvalidShiftCount"
    ));
}

#[test]
fn add_with_no_operands_returns_zero() {
    check(r#"(add)"#, r#"0"#);
}

#[test]
fn add_with_single_operand_returns_itself() {
    check(r#"(add 1)"#, r#"1"#);
}

#[test]
fn add_sums_two_integers() {
    check(r#"(add 1 2)"#, r#"3"#);
}

#[test]
fn add_sums_all_operands() {
    check(r#"(add 1 2 3 4)"#, r#"10"#);
}

#[test]
fn add_sums_negative_and_positive_operands() {
    check(r#"(add -1 2 -3 4)"#, r#"2"#);
}

#[test]
fn mul_with_no_operands_returns_one() {
    check(r#"(mul)"#, r#"1"#);
}

#[test]
fn mul_with_single_operand_returns_itself() {
    check(r#"(mul 2)"#, r#"2"#);
}

#[test]
fn mul_multiplies_two_integers() {
    check(r#"(mul 2 3)"#, r#"6"#);
}

#[test]
fn mul_multiplies_all_operands() {
    check(r#"(mul 2 3 4)"#, r#"24"#);
}

#[test]
fn mul_multiplies_negative_and_positive_operands() {
    check(r#"(mul -2 3 -4)"#, r#"24"#);
}

#[test]
fn sub_with_single_operand_negates_it() {
    check(r#"(sub 5)"#, r#"-5"#);
}

#[test]
fn sub_subtracts_second_from_first() {
    check(r#"(sub 10 3)"#, r#"7"#);
}

#[test]
fn sub_folds_left_to_right() {
    check(r#"(sub 10 3 2)"#, r#"5"#);
}

#[test]
fn sub_subtracts_all_operands() {
    check(r#"(sub 20 5 3 2)"#, r#"10"#);
}

#[test]
fn div_with_single_operand_returns_reciprocal() {
    check(r#"($ "%t:%s" (div 10) (div 10))"#, r#""float:0.1""#);
}

#[test]
fn div_with_single_float_operand_returns_reciprocal() {
    check(r#"($ "%t:%s" (div 2.5) (div 2.5))"#, r#""float:0.4""#);
}

#[test]
fn div_divides_first_by_second() {
    check(r#"($ "%t:%s" (div 10 2) (div 10 2))"#, r#""int:5""#);
}

#[test]
fn div_folds_left_to_right() {
    check(r#"($ "%t:%s" (div 10 2 5) (div 10 2 5))"#, r#""int:1""#);
}

#[test]
fn mod_returns_remainder_of_integer_division() {
    check(r#"(mod 7 2)"#, r#"1"#);
}

#[test]
fn pow_raises_base_to_exponent() {
    check(r#"(pow 2 10)"#, r#"1024"#);
}

#[test]
fn bit_and_with_no_operands_returns_minus_one() {
    check(r#"(bit-and)"#, r#"-1"#);
}

#[test]
fn bit_and_with_single_operand_returns_itself() {
    check(r#"(bit-and 7)"#, r#"7"#);
}

#[test]
fn bit_and_combines_two_integers() {
    check(r#"(bit-and 7 3)"#, r#"3"#);
}

#[test]
fn bit_and_folds_all_operands() {
    check(r#"(bit-and 7 3 1)"#, r#"1"#);
}

#[test]
fn bit_or_with_no_operands_returns_zero() {
    check(r#"(bit-or)"#, r#"0"#);
}

#[test]
fn bit_or_with_single_operand_returns_itself() {
    check(r#"(bit-or 7)"#, r#"7"#);
}

#[test]
fn bit_or_folds_all_operands() {
    check(r#"(bit-or 1 2 4)"#, r#"7"#);
}

#[test]
fn bit_xor_with_no_operands_returns_zero() {
    check(r#"(bit-xor)"#, r#"0"#);
}

#[test]
fn bit_xor_with_single_operand_returns_itself() {
    check(r#"(bit-xor 7)"#, r#"7"#);
}

#[test]
fn bit_xor_folds_all_operands() {
    check(r#"(bit-xor 7 3 1)"#, r#"5"#);
}

#[test]
fn bit_xor_of_equal_operands_returns_zero() {
    check(r#"(bit-xor 7 7)"#, r#"0"#);
}

#[test]
fn bit_not_of_zero_returns_minus_one() {
    check(r#"(bit-not 0)"#, r#"-1"#);
}

#[test]
fn bit_not_inverts_all_bits() {
    check(r#"(bit-not 5)"#, r#"-6"#);
}

#[test]
fn bit_not_of_minus_one_returns_zero() {
    check(r#"(bit-not -1)"#, r#"0"#);
}

#[test]
fn add_sums_two_positive_integers() {
    check(r#"(add 2 3)"#, r#"5"#);
}

#[test]
fn mul_multiplies_two_positive_integers() {
    check(r#"(mul 6 7)"#, r#"42"#);
}

#[test]
fn mod_keeps_dividend_sign() {
    check(r#"(mod -7 2)"#, r#"-1"#);
}
