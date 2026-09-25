use super::{check, run};
use crate::*;

#[test]
fn arrays_support_bracket_indexing_and_inclusive_slices() {
    check(
        "(let value [10 20 30 40 50]) (expect value[1] 10) (expect value[-1] 50) (expect value[2..4] [20 30 40]) (expect value[..2] [10 20]) (expect value[4..] [40 50]) (expect value[[-1 1]] [50 10])",
        "t",
    );
    check(
        "(let value {score:[23 [1 2 3] 66]}) (expect value.score[2][3] 3) (expect value.score[2][[1 3]] [1 3])",
        "t",
    );
}

#[test]
fn fully_open_slice_of_empty_array_is_empty() {
    check("(let a []) (let b a[..]) b", "[]");
}

#[test]
fn array_dot_access_is_rejected() {
    assert!(matches!(
        run("(let value [1]) value.1"),
        Err(Error::Parse(message)) if message.contains("struct field")
    ));
}

#[test]
fn duplicate_struct_key_is_an_error() {
    assert!(matches!(run("{x: 1 x: 2}"), Err(Error::DuplicateKey(_))));
    assert!(matches!(run("{_: 1 _: 2}"), Err(Error::DuplicateKey(key)) if key == "_"));
}

#[test]
fn struct_keys_use_identifier_syntax() {
    check(
        "(let value {name:\"Yvan\" visits:2}) value.name",
        "\"Yvan\"",
    );
    assert!(matches!(
        run(r#"{"name":"Yvan"}"#),
        Err(Error::Parse(message)) if message == "struct key must be an identifier"
    ));
}

#[test]
fn structs_unwrap_the_underscore_field_only_in_operator_position() {
    check(
        "(let module {open: {_: (fn () 42) spec: {documentation: \"open\" arity: 0 type: _ return: []}}}) (module.open)",
        "42",
    );
    check(
        "(let module {open: {_: (fn () 42) spec: {documentation:\"open\" arity: 0 type: _ return: []}}}) ($ \"%s\" module.open)",
        r#""{_:<fn> spec:{documentation:\"open\" arity:0 type: return:[]}}""#,
    );
}

#[test]
fn only_full_callable_descriptors_unwrap_in_operator_position() {
    // A `_` field alone does not make a struct callable: only the descriptor
    // pair {_: callable spec: ...} unwraps, and only in operator position.
    assert!(matches!(
        run(r#"(let wtf {a:"Some" _:(fn () 10)}) (wtf)"#),
        Err(Error::Type(message)) if message == "value is not callable"
    ));
    assert!(matches!(
        run(r#"(let fn-as-field {_:(fn (x y) (add x y))}) (fn-as-field 2 3)"#),
        Err(Error::Type(message)) if message == "value is not callable"
    ));
    assert!(matches!(
        run(r#"(let nested {inner:{_:(fn () 7)}}) (nested.inner)"#),
        Err(Error::Type(message)) if message == "value is not callable"
    ));
}

#[test]
fn descriptor_unwraps_exactly_one_level_and_rejects_non_callable_slots() {
    // The descriptor form {_: callable spec: ...} is the only callable struct.
    check(
        "(let module {open: {_: (fn () 42) spec: {documentation: \"open\" arity: 0 type: _ return: []}}}) (module.open)",
        "42",
    );
    // A descriptor whose `_` is itself a struct (not a function) fails
    // validation — no `_` chain is ever unwrapped.
    assert!(matches!(
        run(
            r#"(let inner {_: (fn () 7) spec: {documentation: "i" arity: 0 type: _ return: []}})
               (let outer {_: inner spec: {documentation: "o" arity: 0 type: _ return: []}})
               (outer)"#
        ),
        Err(Error::Type(message)) if message == "module descriptor _ must be callable"
    ));
}

#[test]
fn array_literal_preserves_its_elements() {
    check(r#"[1 2 3]"#, r#"[1 2 3]"#);
}

#[test]
fn complex_struct_literal_preserves_all_fields() {
    check(
        r#"(let complex-struct
  {name:"Yvan"
   age:56
   address:"1 rue de paris"
   city:"Paris"
   contact:{
     gsm:"0102030405"
     fax:"0102030406"
   }
   score:[
     1
     2
     [21 [211 212] 22 23]
     3
   ]})
complex-struct"#,
        r#"{name:"Yvan"
   age:56
   address:"1 rue de paris"
   city:"Paris"
   contact:{
     gsm:"0102030405"
     fax:"0102030406"
   }
   score:[1 2 [21 [211 212] 22 23] 3]}"#,
    );
}

#[test]
fn struct_name_field_access_returns_string() {
    check(
        r#"(let complex-struct
  {name:"Yvan"
   age:56
   address:"1 rue de paris"
   city:"Paris"
   contact:{
     gsm:"0102030405"
     fax:"0102030406"
   }
   score:[
     1
     2
     [21 [211 212] 22 23]
     3
   ]})
complex-struct.name"#,
        r#""Yvan""#,
    );
}

#[test]
fn struct_age_field_access_returns_integer() {
    check(
        r#"(let complex-struct
  {name:"Yvan"
   age:56
   address:"1 rue de paris"
   city:"Paris"
   contact:{
     gsm:"0102030405"
     fax:"0102030406"
   }
   score:[
     1
     2
     [21 [211 212] 22 23]
     3
   ]})
complex-struct.age"#,
        r#"56"#,
    );
}

#[test]
fn struct_city_field_access_returns_string() {
    check(
        r#"(let complex-struct
  {name:"Yvan"
   age:56
   address:"1 rue de paris"
   city:"Paris"
   contact:{
     gsm:"0102030405"
     fax:"0102030406"
   }
   score:[
     1
     2
     [21 [211 212] 22 23]
     3
   ]})
complex-struct.city"#,
        r#""Paris""#,
    );
}

#[test]
fn nested_struct_gsm_field_access_returns_string() {
    check(
        r#"(let complex-struct
  {name:"Yvan"
   age:56
   address:"1 rue de paris"
   city:"Paris"
   contact:{
     gsm:"0102030405"
     fax:"0102030406"
   }
   score:[
     1
     2
     [21 [211 212] 22 23]
     3
   ]})
complex-struct.contact.gsm"#,
        r#""0102030405""#,
    );
}

#[test]
fn nested_struct_fax_field_access_returns_string() {
    check(
        r#"(let complex-struct
  {name:"Yvan"
   age:56
   address:"1 rue de paris"
   city:"Paris"
   contact:{
     gsm:"0102030405"
     fax:"0102030406"
   }
   score:[
     1
     2
     [21 [211 212] 22 23]
     3
   ]})
complex-struct.contact.fax"#,
        r#""0102030406""#,
    );
}

#[test]
fn score_index_one_returns_first_element() {
    check(
        r#"(let complex-struct
  {name:"Yvan"
   age:56
   address:"1 rue de paris"
   city:"Paris"
   contact:{
     gsm:"0102030405"
     fax:"0102030406"
   }
   score:[
     1
     2
     [21 [211 212] 22 23]
     3
   ]})
complex-struct.score[1]"#,
        r#"1"#,
    );
}

#[test]
fn score_index_two_returns_second_element() {
    check(
        r#"(let complex-struct
  {name:"Yvan"
   age:56
   address:"1 rue de paris"
   city:"Paris"
   contact:{
     gsm:"0102030405"
     fax:"0102030406"
   }
   score:[
     1
     2
     [21 [211 212] 22 23]
     3
   ]})
complex-struct.score[2]"#,
        r#"2"#,
    );
}

#[test]
fn score_index_three_returns_third_element() {
    check(
        r#"(let complex-struct
  {name:"Yvan"
   age:56
   address:"1 rue de paris"
   city:"Paris"
   contact:{
     gsm:"0102030405"
     fax:"0102030406"
   }
   score:[
     1
     2
     [21 [211 212] 22 23]
     3
   ]})
complex-struct.score[3]"#,
        r#"[21 [211 212] 22 23]"#,
    );
}

#[test]
fn score_index_four_returns_fourth_element() {
    check(
        r#"(let complex-struct
  {name:"Yvan"
   age:56
   address:"1 rue de paris"
   city:"Paris"
   contact:{
     gsm:"0102030405"
     fax:"0102030406"
   }
   score:[
     1
     2
     [21 [211 212] 22 23]
     3
   ]})
complex-struct.score[4]"#,
        r#"3"#,
    );
}

#[test]
fn nested_score_index_one_returns_first_element() {
    check(
        r#"(let complex-struct
  {name:"Yvan"
   age:56
   address:"1 rue de paris"
   city:"Paris"
   contact:{
     gsm:"0102030405"
     fax:"0102030406"
   }
   score:[
     1
     2
     [21 [211 212] 22 23]
     3
   ]})
complex-struct.score[3][1]"#,
        r#"21"#,
    );
}

#[test]
fn nested_score_index_two_returns_second_element() {
    check(
        r#"(let complex-struct
  {name:"Yvan"
   age:56
   address:"1 rue de paris"
   city:"Paris"
   contact:{
     gsm:"0102030405"
     fax:"0102030406"
   }
   score:[
     1
     2
     [21 [211 212] 22 23]
     3
   ]})
complex-struct.score[3][2]"#,
        r#"[211 212]"#,
    );
}

#[test]
fn nested_score_index_three_returns_third_element() {
    check(
        r#"(let complex-struct
  {name:"Yvan"
   age:56
   address:"1 rue de paris"
   city:"Paris"
   contact:{
     gsm:"0102030405"
     fax:"0102030406"
   }
   score:[
     1
     2
     [21 [211 212] 22 23]
     3
   ]})
complex-struct.score[3][3]"#,
        r#"22"#,
    );
}

#[test]
fn nested_score_index_four_returns_fourth_element() {
    check(
        r#"(let complex-struct
  {name:"Yvan"
   age:56
   address:"1 rue de paris"
   city:"Paris"
   contact:{
     gsm:"0102030405"
     fax:"0102030406"
   }
   score:[
     1
     2
     [21 [211 212] 22 23]
     3
   ]})
complex-struct.score[3][4]"#,
        r#"23"#,
    );
}

#[test]
fn deep_index_returns_first_sub_element() {
    check(
        r#"(let complex-struct
  {name:"Yvan"
   age:56
   address:"1 rue de paris"
   city:"Paris"
   contact:{
     gsm:"0102030405"
     fax:"0102030406"
   }
   score:[
     1
     2
     [21 [211 212] 22 23]
     3
   ]})
complex-struct.score[3][2][1]"#,
        r#"211"#,
    );
}

#[test]
fn deep_index_returns_second_sub_element() {
    check(
        r#"(let complex-struct
  {name:"Yvan"
   age:56
   address:"1 rue de paris"
   city:"Paris"
   contact:{
     gsm:"0102030405"
     fax:"0102030406"
   }
   score:[
     1
     2
     [21 [211 212] 22 23]
     3
   ]})
complex-struct.score[3][2][2]"#,
        r#"212"#,
    );
}

#[test]
fn index_with_array_selects_score_elements() {
    check(
        r#"(let complex-struct
  {name:"Yvan"
   age:56
   address:"1 rue de paris"
   city:"Paris"
   contact:{
     gsm:"0102030405"
     fax:"0102030406"
   }
   score:[
     1
     2
     [21 [211 212] 22 23]
     3
   ]})
complex-struct.score[3][[1 3]]"#,
        r#"[21 22]"#,
    );
}

#[test]
fn index_with_array_selects_first_and_last_elements() {
    check(
        r#"(let complex-struct
  {name:"Yvan"
   age:56
   address:"1 rue de paris"
   city:"Paris"
   contact:{
     gsm:"0102030405"
     fax:"0102030406"
   }
   score:[
     1
     2
     [21 [211 212] 22 23]
     3
   ]})
complex-struct.score[3][[1 4]]"#,
        r#"[21 23]"#,
    );
}

#[test]
fn nested_index_with_array_selects_elements() {
    check(
        r#"(let complex-struct
  {name:"Yvan"
   age:56
   address:"1 rue de paris"
   city:"Paris"
   contact:{
     gsm:"0102030405"
     fax:"0102030406"
   }
   score:[
     1
     2
     [21 [211 212] 22 23]
     3
   ]})
complex-struct.score[3][[2 3]]"#,
        r#"[[211 212] 22]"#,
    );
}

#[test]
fn deep_index_with_array_selects_elements() {
    check(
        r#"(let complex-struct
  {name:"Yvan"
   age:56
   address:"1 rue de paris"
   city:"Paris"
   contact:{
     gsm:"0102030405"
     fax:"0102030406"
   }
   score:[
     1
     2
     [21 [211 212] 22 23]
     3
   ]})
complex-struct.score[3][2][[1 2]]"#,
        r#"[211 212]"#,
    );
}

#[test]
fn negative_index_selects_last_score_element() {
    check(
        r#"(let complex-struct
  {name:"Yvan"
   age:56
   address:"1 rue de paris"
   city:"Paris"
   contact:{
     gsm:"0102030405"
     fax:"0102030406"
   }
   score:[
     1
     2
     [21 [211 212] 22 23]
     3
   ]})
complex-struct.score[-1]"#,
        r#"3"#,
    );
}

#[test]
fn negative_index_selects_second_to_last_score_element() {
    check(
        r#"(let complex-struct
  {name:"Yvan"
   age:56
   address:"1 rue de paris"
   city:"Paris"
   contact:{
     gsm:"0102030405"
     fax:"0102030406"
   }
   score:[
     1
     2
     [21 [211 212] 22 23]
     3
   ]})
complex-struct.score[-2]"#,
        r#"[21 [211 212] 22 23]"#,
    );
}

#[test]
fn negative_index_selects_last_nested_element() {
    check(
        r#"(let complex-struct
  {name:"Yvan"
   age:56
   address:"1 rue de paris"
   city:"Paris"
   contact:{
     gsm:"0102030405"
     fax:"0102030406"
   }
   score:[
     1
     2
     [21 [211 212] 22 23]
     3
   ]})
complex-struct.score[3][-1]"#,
        r#"23"#,
    );
}

#[test]
fn negative_index_selects_last_deep_element() {
    check(
        r#"(let complex-struct
  {name:"Yvan"
   age:56
   address:"1 rue de paris"
   city:"Paris"
   contact:{
     gsm:"0102030405"
     fax:"0102030406"
   }
   score:[
     1
     2
     [21 [211 212] 22 23]
     3
   ]})
complex-struct.score[3][2][-1]"#,
        r#"212"#,
    );
}

#[test]
fn negative_slice_selects_last_two_score_elements() {
    check(
        r#"(let complex-struct
  {name:"Yvan"
   age:56
   address:"1 rue de paris"
   city:"Paris"
   contact:{
     gsm:"0102030405"
     fax:"0102030406"
   }
   score:[
     1
     2
     [21 [211 212] 22 23]
     3
   ]})
complex-struct.score[-2..-1]"#,
        r#"[[21 [211 212] 22 23] 3]"#,
    );
}

#[test]
fn negative_slice_selects_last_three_nested_elements() {
    check(
        r#"(let complex-struct
  {name:"Yvan"
   age:56
   address:"1 rue de paris"
   city:"Paris"
   contact:{
     gsm:"0102030405"
     fax:"0102030406"
   }
   score:[
     1
     2
     [21 [211 212] 22 23]
     3
   ]})
complex-struct.score[3][-3..-1]"#,
        r#"[[211 212] 22 23]"#,
    );
}
