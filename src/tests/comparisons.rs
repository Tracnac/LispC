use super::check;

#[test]
fn integer_order_comparisons_are_exact_beyond_f64_precision() {
    for (source, expected) in [
        ("(lt 9007199254740993 9007199254740994)", true),
        ("(gt 9007199254740993 9007199254740992)", true),
        ("(le 9007199254740993 9007199254740993)", true),
        ("(ge 9007199254740994 9007199254740993)", true),
        (
            "(lt 9007199254740992 9007199254740993 9007199254740994)",
            true,
        ),
        ("(lt -9007199254740993 -9007199254740992)", true),
        ("(lt 9007199254740994 9007199254740993)", false),
        ("(gt 9007199254740992 9007199254740993)", false),
        ("(le 9007199254740994 9007199254740993)", false),
        ("(ge 9007199254740993 9007199254740994)", false),
    ] {
        check(source, if expected { "t" } else { "f" });
    }
    check("(lt 1 2.0)", "t");
    check("(le 2 2)", "t");
}

#[test]
fn eq_arrays_with_same_elements_are_equal() {
    check(r#"(eq [1 2 3] [1 2 3])"#, r#"t"#);
}

#[test]
fn eq_array_element_order_matters() {
    check(r#"(eq [1 2 3] [3 2 1])"#, r#"f"#);
}

#[test]
fn eq_nested_arrays_are_equal() {
    check(r#"(eq [1 [2 3]] [1 [2 3]])"#, r#"t"#);
}

#[test]
fn eq_nested_array_element_order_matters() {
    check(r#"(eq [1 [2 3]] [1 [3 2]])"#, r#"f"#);
}

#[test]
fn eq_structs_with_same_fields_are_equal() {
    check(r#"(eq {a:1 b:2} {a:1 b:2})"#, r#"t"#);
}

#[test]
fn eq_struct_field_order_is_irrelevant() {
    check(r#"(eq {a:1 b:2} {b:2 a:1})"#, r#"t"#);
}

#[test]
fn eq_structs_with_different_values_differ() {
    check(r#"(eq {a:1 b:2} {a:1 b:3})"#, r#"f"#);
}

#[test]
fn eq_structs_with_nested_arrays_are_equal() {
    check(r#"(eq {a:[1 2]} {a:[1 2]})"#, r#"t"#);
}

#[test]
fn eq_struct_nested_array_order_matters() {
    check(r#"(eq {a:[1 2]} {a:[2 1]})"#, r#"f"#);
}

#[test]
fn eq_same_integers_are_equal() {
    check(r#"(eq 5 5)"#, r#"t"#);
}

#[test]
fn eq_different_integers_are_not_equal() {
    check(r#"(eq 5 6)"#, r#"f"#);
}

#[test]
fn eq_negative_integers_are_equal() {
    check(r#"(eq -5 -5)"#, r#"t"#);
}

#[test]
fn eq_negative_and_positive_integers_differ() {
    check(r#"(eq -5 5)"#, r#"f"#);
}

#[test]
fn eq_zero_is_equal_to_itself() {
    check(r#"(eq 0 0)"#, r#"t"#);
}

#[test]
fn eq_integer_equals_float_with_same_value() {
    check(r#"(eq 5 5.0)"#, r#"t"#);
}

#[test]
fn eq_float_equals_integer_with_same_value() {
    check(r#"(eq 5.0 5)"#, r#"t"#);
}

#[test]
fn eq_integer_and_different_float_are_not_equal() {
    check(r#"(eq 5 5.1)"#, r#"f"#);
}

#[test]
fn eq_float_and_different_integer_are_not_equal() {
    check(r#"(eq 5.1 5)"#, r#"f"#);
}

#[test]
fn eq_zero_and_float_zero_are_equal() {
    check(r#"(eq 0 0.0)"#, r#"t"#);
}

#[test]
fn eq_negative_integer_and_matching_float_are_equal() {
    check(r#"(eq -5 -5.0)"#, r#"t"#);
}

#[test]
fn eq_close_float_literals_round_to_same_value() {
    check(r#"(eq 0.1 0.10000000000000001)"#, r#"t"#);
}

#[test]
fn eq_float_literals_differing_in_value_are_not_equal() {
    check(r#"(eq 0.1 0.10000000000000002)"#, r#"f"#);
}

#[test]
fn eq_same_floats_are_equal() {
    check(r#"(eq 1.0 1.0)"#, r#"t"#);
}

#[test]
fn eq_different_floats_are_not_equal() {
    check(r#"(eq 1.0 2.0)"#, r#"f"#);
}

#[test]
fn eq_negative_floats_are_equal() {
    check(r#"(eq -1.5 -1.5)"#, r#"t"#);
}

#[test]
fn eq_negative_and_positive_floats_differ() {
    check(r#"(eq -1.5 1.5)"#, r#"f"#);
}

#[test]
fn eq_nan_is_not_equal_to_itself() {
    check(r#"(eq NaN NaN)"#, r#"f"#);
}

#[test]
fn eq_infinity_is_equal_to_itself() {
    check(r#"(eq Inf Inf)"#, r#"t"#);
}

#[test]
fn eq_negative_infinity_is_equal_to_itself() {
    check(r#"(eq -Inf -Inf)"#, r#"t"#);
}

#[test]
fn eq_integer_and_infinity_are_not_equal() {
    check(r#"(eq 5 Inf)"#, r#"f"#);
}

#[test]
fn eq_float_and_infinity_are_not_equal() {
    check(r#"(eq 5.0 Inf)"#, r#"f"#);
}

#[test]
fn lt_float_is_less_than_infinity() {
    check(r#"(lt 1.0 Inf)"#, r#"t"#);
}

#[test]
fn lt_negative_infinity_is_less_than_float() {
    check(r#"(lt -Inf 1.0)"#, r#"t"#);
}

#[test]
fn gt_negative_infinity_is_not_greater_than_float() {
    check(r#"(gt -Inf 1.0)"#, r#"f"#);
}

#[test]
fn lt_nan_is_not_less_than_float() {
    check(r#"(lt NaN 1.0)"#, r#"f"#);
}

#[test]
fn gt_nan_is_not_greater_than_float() {
    check(r#"(gt NaN 1.0)"#, r#"f"#);
}

#[test]
fn eq_zero_and_negative_zero_are_equal() {
    check(r#"(eq 0.0 -0.0)"#, r#"t"#);
}

#[test]
fn ne_different_integers_are_not_equal() {
    check(r#"(ne 5 6)"#, r#"t"#);
}

#[test]
fn ne_same_integers_are_not_different() {
    check(r#"(ne 5 5)"#, r#"f"#);
}

#[test]
fn ne_integer_and_matching_float_are_not_different() {
    check(r#"(ne 5 5.0)"#, r#"f"#);
}

#[test]
fn ne_integer_and_different_float_are_different() {
    check(r#"(ne 5 5.1)"#, r#"t"#);
}

#[test]
fn ne_different_floats_are_not_equal() {
    check(r#"(ne 1.0 2.0)"#, r#"t"#);
}

#[test]
fn ne_same_floats_are_not_different() {
    check(r#"(ne 1.0 1.0)"#, r#"f"#);
}

#[test]
fn ne_nan_is_not_equal_to_itself() {
    check(r#"(ne NaN NaN)"#, r#"t"#);
}

#[test]
fn ne_zero_and_negative_zero_are_not_different() {
    check(r#"(ne 0.0 -0.0)"#, r#"f"#);
}

#[test]
fn lt_smaller_integer_is_less() {
    check(r#"(lt 2 3)"#, r#"t"#);
}

#[test]
fn lt_larger_integer_is_not_less() {
    check(r#"(lt 3 2)"#, r#"f"#);
}

#[test]
fn lt_equal_integers_are_not_less() {
    check(r#"(lt 3 3)"#, r#"f"#);
}

#[test]
fn lt_integer_is_less_than_larger_float() {
    check(r#"(lt 2 3.0)"#, r#"t"#);
}

#[test]
fn lt_float_is_not_less_than_smaller_integer() {
    check(r#"(lt 3.0 2)"#, r#"f"#);
}

#[test]
fn lt_equal_floats_are_not_less() {
    check(r#"(lt 3.0 3.0)"#, r#"f"#);
}

#[test]
fn gt_larger_integer_is_greater() {
    check(r#"(gt 3 2)"#, r#"t"#);
}

#[test]
fn gt_smaller_integer_is_not_greater() {
    check(r#"(gt 2 3)"#, r#"f"#);
}

#[test]
fn gt_equal_integers_are_not_greater() {
    check(r#"(gt 3 3)"#, r#"f"#);
}

#[test]
fn gt_integer_is_greater_than_smaller_float() {
    check(r#"(gt 3 2.0)"#, r#"t"#);
}

#[test]
fn gt_float_is_not_greater_than_larger_integer() {
    check(r#"(gt 2.0 3)"#, r#"f"#);
}

#[test]
fn le_smaller_integer_is_less_or_equal() {
    check(r#"(le 2 3)"#, r#"t"#);
}

#[test]
fn le_equal_integers_are_less_or_equal() {
    check(r#"(le 3 3)"#, r#"t"#);
}

#[test]
fn le_larger_integer_is_not_less_or_equal() {
    check(r#"(le 3 2)"#, r#"f"#);
}

#[test]
fn le_integer_is_less_or_equal_to_larger_float() {
    check(r#"(le 2 3.0)"#, r#"t"#);
}

#[test]
fn le_float_is_less_or_equal_to_same_integer() {
    check(r#"(le 3.0 3)"#, r#"t"#);
}

#[test]
fn le_float_is_not_less_or_equal_to_smaller_integer() {
    check(r#"(le 3.0 2)"#, r#"f"#);
}

#[test]
fn ge_larger_integer_is_greater_or_equal() {
    check(r#"(ge 3 2)"#, r#"t"#);
}

#[test]
fn ge_equal_integers_are_greater_or_equal() {
    check(r#"(ge 3 3)"#, r#"t"#);
}

#[test]
fn ge_smaller_integer_is_not_greater_or_equal() {
    check(r#"(ge 2 3)"#, r#"f"#);
}

#[test]
fn ge_integer_is_greater_or_equal_to_smaller_float() {
    check(r#"(ge 3 2.0)"#, r#"t"#);
}

#[test]
fn ge_float_is_greater_or_equal_to_same_integer() {
    check(r#"(ge 3.0 3)"#, r#"t"#);
}

#[test]
fn ge_float_is_not_greater_or_equal_to_larger_integer() {
    check(r#"(ge 2.0 3)"#, r#"f"#);
}

#[test]
fn eq_with_no_operands_is_true() {
    check(r#"(eq)"#, r#"t"#);
}

#[test]
fn eq_with_single_operand_is_true() {
    check(r#"(eq 1)"#, r#"t"#);
}

#[test]
fn eq_two_equal_operands_are_equal() {
    check(r#"(eq 1 1)"#, r#"t"#);
}

#[test]
fn eq_all_equal_operands_are_equal() {
    check(r#"(eq 1 1 1)"#, r#"t"#);
}

#[test]
fn eq_is_false_when_an_operand_differs() {
    check(r#"(eq 1 1 2)"#, r#"f"#);
}

#[test]
fn eq_two_element_arrays_with_same_contents_are_equal() {
    check(r#"(eq [1 2] [1 2])"#, r#"t"#);
}

#[test]
fn eq_two_element_arrays_differ_by_order() {
    check(r#"(eq [1 2] [2 1])"#, r#"f"#);
}

#[test]
fn ne_with_no_operands_is_false() {
    check(r#"(ne)"#, r#"f"#);
}

#[test]
fn ne_with_single_operand_is_false() {
    check(r#"(ne 1)"#, r#"f"#);
}

#[test]
fn ne_different_operands_are_not_equal() {
    check(r#"(ne 1 2)"#, r#"t"#);
}

#[test]
fn ne_all_different_operands_are_not_equal() {
    check(r#"(ne 1 2 3)"#, r#"t"#);
}

#[test]
fn ne_is_true_when_any_pair_differs() {
    check(r#"(ne 1 2 1)"#, r#"t"#);
}

#[test]
fn eq_is_pairwise_not_just_adjacent() {
    // 9007199254740993 (int) is equal to 9007199254740992.0 (float) once
    // rounded to f64, and that float equals 9007199254740992 (int); but the
    // two ints differ. eq must therefore be false for all three.
    check(
        r#"(eq 9007199254740993 9007199254740992.0 9007199254740992)"#,
        r#"f"#,
    );
    check(
        r#"(ne 9007199254740993 9007199254740992.0 9007199254740992)"#,
        r#"t"#,
    );
}

#[test]
fn eq_and_ne_are_exact_complements() {
    check(
        "(expect (eq 1 1) t) (expect (ne 1 1) f) (expect (eq 1 2) f) (expect (ne 1 2) t) (expect (eq 1 1 1) t) (expect (ne 1 1 1) f) (expect (eq 1 1 2) f) (expect (ne 1 1 2) t) (expect (eq 1 2 3) f) (expect (ne 1 2 3) t)",
        "t",
    );
}

#[test]
fn lt_with_no_operands_is_true() {
    check(r#"(lt)"#, r#"t"#);
}

#[test]
fn lt_with_single_operand_is_true() {
    check(r#"(lt 1)"#, r#"t"#);
}

#[test]
fn lt_increasing_pair_is_true() {
    check(r#"(lt 1 2)"#, r#"t"#);
}

#[test]
fn lt_strictly_increasing_chain_is_true() {
    check(r#"(lt 1 2 3)"#, r#"t"#);
}

#[test]
fn lt_longer_increasing_chain_is_true() {
    check(r#"(lt 1 2 3 4 5)"#, r#"t"#);
}

#[test]
fn lt_chain_breaks_on_decrease() {
    check(r#"(lt 1 3 2)"#, r#"f"#);
}

#[test]
fn lt_chain_breaks_on_equal_neighbors() {
    check(r#"(lt 1 2 2)"#, r#"f"#);
}

#[test]
fn lt_chain_mixes_integer_and_float() {
    check(r#"(lt 1 2.0 3)"#, r#"t"#);
}

#[test]
fn gt_with_no_operands_is_true() {
    check(r#"(gt)"#, r#"t"#);
}

#[test]
fn gt_with_single_operand_is_true() {
    check(r#"(gt 5)"#, r#"t"#);
}

#[test]
fn gt_strictly_decreasing_chain_is_true() {
    check(r#"(gt 5 4 3 2 1)"#, r#"t"#);
}

#[test]
fn gt_chain_breaks_on_equal_neighbors() {
    check(r#"(gt 5 4 4)"#, r#"f"#);
}

#[test]
fn gt_chain_breaks_on_increase() {
    check(r#"(gt 5 3 4)"#, r#"f"#);
}

#[test]
fn le_with_no_operands_is_true() {
    check(r#"(le)"#, r#"t"#);
}

#[test]
fn le_with_single_operand_is_true() {
    check(r#"(le 1)"#, r#"t"#);
}

#[test]
fn le_increasing_pair_is_true() {
    check(r#"(le 1 2)"#, r#"t"#);
}

#[test]
fn le_chain_allows_equal_neighbors() {
    check(r#"(le 1 2 2 3)"#, r#"t"#);
}

#[test]
fn le_chain_breaks_on_decrease() {
    check(r#"(le 1 2 1)"#, r#"f"#);
}

#[test]
fn ge_with_no_operands_is_true() {
    check(r#"(ge)"#, r#"t"#);
}

#[test]
fn ge_with_single_operand_is_true() {
    check(r#"(ge 3)"#, r#"t"#);
}

#[test]
fn ge_chain_allows_equal_neighbors() {
    check(r#"(ge 3 2 2 1)"#, r#"t"#);
}

#[test]
fn ge_chain_breaks_on_increase() {
    check(r#"(ge 3 2 4)"#, r#"f"#);
}

#[test]
fn le_nan_is_not_less_or_equal_to_float() {
    check(r#"(le NaN 1.0)"#, r#"f"#);
}

#[test]
fn ge_nan_is_not_greater_or_equal_to_float() {
    check(r#"(ge NaN 1.0)"#, r#"f"#);
}
