use super::run;
use crate::*;

#[test]
fn native_str_module_is_loaded_and_introspectable() {
    let value = run(r#"(use "str")
               (expect (str.upper "hello") "HELLO")
               (expect (str.lower "HELLO") "hello")
               (expect str.upper.spec.documentation "Convert a string to uppercase.")
               (expect str.upper.spec.arity 1)
               (expect str.upper.spec.type ["string"])
               (expect str.upper.spec.return ["string"])
               ($ "%s" str.upper)"#)
    .unwrap();
    assert!(matches!(
        value,
        Value::Str(text) if text == r#"{_:<native fn> spec:{documentation:"Convert a string to uppercase." arity:1 type:["string"] return:["string"]}}"#
    ));
}

#[test]
fn module_descriptor_spec_is_checked_live_on_every_call() {
    // The spec is read fresh at each call, so `set` into a bound module's
    // descriptor takes effect immediately — a cached (arity, type) would go
    // stale here.
    let value = run(r#"(use "str")
               (set str.upper.spec.arity 2)
               (set str.upper.spec.type ["string" "string"])
               (str.upper "a" "b")"#)
    .unwrap();
    assert!(matches!(value, Value::Str(text) if text == "A"));
    // An inconsistent mutated spec is caught at call time with the usual
    // message rather than being skipped.
    assert!(matches!(
        run(r#"(use "str")
           (set str.upper.spec.arity 2)
           (str.upper "a")"#),
        Err(Error::Type(message))
            if message.contains("spec.type length must match spec.arity")
    ));
}

#[test]
fn rebinding_a_module_value_copies_its_descriptors() {
    // A module value obeys ordinary value semantics when rebound: mutating the
    // copy's descriptor must not affect the bound module (and vice versa).
    let value = run(r#"(use "str")
               (let m2 str)
               (set m2.upper.spec.arity 2)
               ($ "%s:%s" str.upper.spec.arity m2.upper.spec.arity)"#)
    .unwrap();
    assert!(matches!(value, Value::Str(text) if text == "1:2"));
}

#[test]
fn use_accepts_string_names_and_rejects_unknown_modules() {
    let value = run(r#"(use "str") (str.upper "hello")"#).unwrap();
    assert!(matches!(value, Value::Str(text) if text == "HELLO"));
    let value = run(r#"(let module-name "str") (use module-name) (str.lower "ABC")"#).unwrap();
    assert!(matches!(value, Value::Str(text) if text == "abc"));
    // Binding the module to a name that already exists in the current scope is a
    // duplicate, not an overwrite: `use` must not modify an existing variable.
    assert!(matches!(
        run(r#"(let str "str") (use str)"#),
        Err(Error::DuplicateBinding(name)) if name == "str"
    ));
    assert!(matches!(
        run("(use str)"),
        Err(Error::Name(message)) if message == "str"
    ));
    assert!(matches!(
        run("(use \"missing\")"),
        Err(Error::Name(message)) if message == "unknown native module missing"
    ));
}

#[test]
fn lisp_modules_are_loaded_from_named_bindings_and_validate_descriptors() {
    let value = run(r#"(use (let mymodule {
                 add: {
                   _: (fn (x y) x)
                   spec: {
                     documentation: "Return the first string."
                     arity: 2
                     type: ["string" "string"]
                     return: []
                   }
                 }
               }))
               (expect mymodule.add.spec.documentation "Return the first string.")
               (expect mymodule.add.spec.arity 2)
               (expect mymodule.add.spec.type ["string" "string"])
               (mymodule.add "a" "b")"#)
    .unwrap();
    assert!(matches!(value, Value::Str(text) if text == "a"));

    assert!(matches!(
        run(r#"(use (let mymodule {
                add: {_: (fn (x y) x) spec: {documentation: "add" arity: 2 type: ["string" "string"] return: []}}
            })) (mymodule.add "a")"#),
        Err(Error::Arity(message)) if message.contains("expects 2 arguments")
    ));
    assert!(matches!(
        run(r#"(use (let mymodule {
                add: {_: (fn (x y) x) spec: {documentation: "add" arity: 2 type: ["string" "string"] return: []}}
            })) (mymodule.add 10 "b")"#),
        Err(Error::Type(message)) if message.contains("argument 1 expects string")
    ));
}

#[test]
fn native_descriptors_use_the_same_validation_path() {
    assert!(matches!(
        run("(use \"str\") (str.upper)"),
        Err(Error::Arity(message)) if message.contains("expects 1 arguments")
    ));
    assert!(matches!(
        run("(use \"str\") (str.upper 10)"),
        Err(Error::Type(message)) if message.contains("argument 1 expects string")
    ));
}

#[test]
fn module_argument_validation_reports_the_call_site_after_nested_evaluation() {
    let source = r#"
(use (let module {
    open: {
        _: (fn (x y) (io.write 1 ($ "Called open with args %s and %s\n" x y)))
        spec: {
            documentation: "open"
            arity: 2
            type: ["string" "string"]
            return: ["string"]
        }
    }
}))
(use "io")
(use "str")
(str.lower (module.open "args1" "args2"))
"#;
    let error = match run(source) {
        Err(error) => error,
        Ok(_) => panic!("expected a module argument type error"),
    };
    assert!(matches!(
        &error,
        Error::Type(message) if message.contains("argument 1 expects string, got int")
    ));
    let span = LAST_ERROR_SPAN.with(|span| *span.borrow());
    let rendered = diagnostic(&error, source, "sample.lisp", span);
    assert!(rendered.starts_with("sample.lisp:15:"));
    assert!(rendered.contains("(str.lower (module.open"));
}

#[test]
fn descriptor_registration_requires_a_complete_callable_spec() {
    let valid = r#"(use (let module {
            foo: {_: (fn (x) x) spec: {
                documentation: "foo" arity: 1 type: ["string"] return: []
            }}
        }))"#;
    assert!(run(valid).is_ok());

    let valid_return = r#"(use (let module {
            foo: {_: (fn (x) x) spec: {
                documentation: "foo" arity: 1 type: ["string"] return: ["string"]
            }}
        }))"#;
    assert!(run(valid_return).is_ok());

    for spec in [
        r#"{arity: 1 type: ["string"] return: []}"#,
        r#"{documentation: "foo" type: ["string"] return: []}"#,
        r#"{documentation: "foo" arity: 1 return: []}"#,
        r#"{documentation: "foo" arity: 1 type: ["string"]}"#,
        r#"{documentation: 1 arity: 1 type: ["string"] return: []}"#,
        r#"{documentation: "foo" arity: "1" type: ["string"] return: []}"#,
        r#"{documentation: "foo" arity: 1 type: "string" return: []}"#,
        r#"{documentation: "foo" arity: 1 type: ["string"] return: 1}"#,
    ] {
        let source = format!("(use (let module {{foo: {{_: (fn (x) x) spec: {spec}}}}}))");
        assert!(matches!(run(&source), Err(Error::Type(_))), "{source}");
    }

    assert!(matches!(
        run(r#"(use (let module {
                foo: {spec: {documentation: "foo" arity: 1 type: ["string"] return: []}}
            }))"#),
        Err(Error::Type(message)) if message.contains("contain _")
    ));
    assert!(matches!(
        run(r#"(use (let module {
                foo: {_: 1 spec: {documentation: "foo" arity: 1 type: ["string"] return: []}}
            }))"#),
        Err(Error::Type(message)) if message.contains("_ must be callable")
    ));
    assert!(run(r#"(use (let module {
            foo: {_: (fn () 1) spec: {
                documentation: "foo" arity: 0 type: _ return: _
            }}
        })) (module.foo)"#)
    .is_ok());
    assert!(matches!(
        run(r#"(use (let module {
                foo: {_: (fn () 1) spec: {
                    documentation: "foo" arity: 0 type: [] return: _
                }}
            }))"#),
        Err(Error::Type(message)) if message.contains("spec.type")
    ));
}

#[test]
fn spec_type_singleton_entries_bind_exactly_one_type_per_argument() {
    // A scalar singleton entry keeps the original contract: entry i is the exact
    // type allowed for argument i.
    let value = run(
        r#"(use (let m {
                inc: {_: (fn (x) (add x 1)) spec: {documentation: "inc" arity: 1 type: ["int"] return: []}}
            })) (expect (m.inc 3) 4) t"#,
    )
    .unwrap();
    assert!(matches!(value, Value::Bool(_)));
    assert!(matches!(
        run(
            r#"(use (let m {
                    inc: {_: (fn (x) (add x 1)) spec: {documentation: "inc" arity: 1 type: ["int"] return: []}}
                })) (m.inc "3")"#
        ),
        Err(Error::Type(message)) if message.contains("argument 1 expects int, got string")
    ));
}

#[test]
fn spec_type_alternative_sets_accept_any_member_per_argument() {
    // One argument with a bounded set: int OR float is accepted.
    let value = run(
        r#"(use (let m {
                twice: {_: (fn (x) (mul x 2)) spec: {documentation: "twice" arity: 1 type: [["int" "float"]] return: []}}
            }))
            (expect (m.twice 3) 6)
            (expect (m.twice 3.5) 7.0) t"#,
    )
    .unwrap();
    assert!(matches!(value, Value::Bool(_)));
    // A type outside the set is rejected, naming the allowed alternatives.
    assert!(matches!(
        run(
            r#"(use (let m {
                    twice: {_: (fn (x) (mul x 2)) spec: {documentation: "twice" arity: 1 type: [["int" "float"]] return: []}}
                })) (m.twice "3")"#
        ),
        Err(Error::Type(message))
            if message.contains("argument 1 expects one of int, float, got string")
    ));
}

#[test]
fn spec_type_alternatives_are_independent_per_argument_position() {
    // arg 1 ∈ {int, float} AND arg 2 = string — the accepted combinations are
    // the cross product, never whole-signature overloads.
    let module = r#"(use (let m {
            pick: {_: (fn (x y) x) spec: {documentation: "pick" arity: 2 type: [["int" "float"] "string"] return: []}}
        }))"#;
    let ok = run(&format!(
        "{module} (expect (m.pick 1 \"a\") 1) (expect (m.pick 1.5 \"a\") 1.5) t"
    ))
    .unwrap();
    assert!(matches!(ok, Value::Bool(_)));
    // Each position is checked against its own entry, in order.
    assert!(matches!(
        run(&format!("{module} (m.pick 1 1.5)")),
        Err(Error::Type(message)) if message.contains("argument 2 expects string, got float")
    ));
    assert!(matches!(
        run(&format!("{module} (m.pick \"a\" \"b\")")),
        Err(Error::Type(message))
            if message.contains("argument 1 expects one of int, float, got string")
    ));
}

#[test]
fn spec_type_any_as_a_standalone_entry_accepts_everything() {
    // Bare "any" singleton (the http-post-style body stopgap) accepts every type.
    let value = run(r#"(use (let m {
                f: {_: (fn (x) x) spec: {documentation: "f" arity: 1 type: ["any"] return: []}}
            }))
            (expect (m.f 42) 42)
            (expect (m.f "s") "s")
            (expect (m.f {a: 1}) {a: 1})
            (expect (m.f [1 2]) [1 2]) t"#)
    .unwrap();
    assert!(matches!(value, Value::Bool(_)));
}

#[test]
fn spec_type_ref_is_a_first_class_vocabulary_member() {
    // A ^ alias argument satisfies a "ref" expectation; reading it inside the
    // function derefs to the current value.
    let value = run(
        r#"(use (let m {
                head: {_: (fn (x) x) spec: {documentation: "head" arity: 1 type: ["ref"] return: []}}
            }))
            (let a [1 2])
            (expect (m.head ^a[1]) 1) t"#,
    )
    .unwrap();
    assert!(matches!(value, Value::Bool(_)));
    // A plain value is not a ref.
    assert!(matches!(
        run(
            r#"(use (let m {
                    head: {_: (fn (x) x) spec: {documentation: "head" arity: 1 type: ["ref"] return: []}}
                })) (m.head 5)"#
        ),
        Err(Error::Type(message)) if message.contains("argument 1 expects ref, got int")
    ));
}

#[test]
fn spec_type_registration_rejects_invalid_entries() {
    let descriptor = |type_field: &str| {
        format!(
            r#"(use (let m {{f: {{_: (fn (x) x) spec: {{documentation: "f" arity: 1 type: {type_field} return: []}}}}}}))"#
        )
    };
    // Unknown type names are rejected at registration, never silently registered
    // as never-matching declarations — singletons and set members alike.
    assert!(matches!(
        run(&descriptor(r#"["all"]"#)),
        Err(Error::Type(message)) if message.contains("`all` must name a known type")
    ));
    assert!(matches!(
        run(&descriptor(r#"[["int" "streng"]]"#)),
        Err(Error::Type(message)) if message.contains("`streng` must name a known type")
    ));
    // Empty alternative sets are rejected.
    assert!(matches!(
        run(&descriptor(r#"[[]]"#)),
        Err(Error::Type(message)) if message.contains("sets must not be empty")
    ));
    // "any" is the top/wildcard type and may only appear as a standalone entry:
    // combining it with alternative types in a set is a registration error
    // (never silently normalized to "any").
    assert!(matches!(
        run(&descriptor(r#"[["int" "any"]]"#)),
        Err(Error::Type(message))
            if message.contains("`any` cannot be combined with alternative types")
    ));
    // Even a set containing only "any" is not a standalone entry.
    assert!(matches!(
        run(&descriptor(r#"[["any"]]"#)),
        Err(Error::Type(message))
            if message.contains("`any` cannot be combined with alternative types")
    ));
    // Non-string set members are rejected.
    assert!(matches!(
        run(&descriptor(r#"[["int" 3]]"#)),
        Err(Error::Type(message)) if message.contains("set members must be strings")
    ));
    // Scalar non-string, non-array entries are rejected.
    assert!(matches!(
        run(&descriptor(r#"[3]"#)),
        Err(Error::Type(message)) if message.contains("entries must be strings or arrays")
    ));
    // The length must still match arity exactly, whatever the entry shape.
    assert!(matches!(
        run(&descriptor(r#"[["int" "float"]]"#).replace("arity: 1", "arity: 2")),
        Err(Error::Type(message))
            if message.contains("spec.type length must match spec.arity")
    ));
}

#[test]
fn spec_type_mutation_affects_subsequent_calls_live() {
    // The spec is read fresh at each call: mutating spec.type from a set to a
    // different set takes effect immediately.
    let value = run(
        r#"(use (let m {
                f: {_: (fn (x) x) spec: {documentation: "f" arity: 1 type: [["int" "float"]] return: []}}
            }))
            (expect (m.f 3) 3)
            (set m.f.spec.type [["string"]])
            (expect (m.f "ok") "ok") t"#,
    )
    .unwrap();
    assert!(matches!(value, Value::Bool(_)));
    // After the mutation the previously-valid argument type fails at call time.
    assert!(matches!(
        run(
            r#"(use (let m {
                    f: {_: (fn (x) x) spec: {documentation: "f" arity: 1 type: [["int" "float"]] return: []}}
                }))
                (set m.f.spec.type [["string"]])
                (m.f 3)"#
        ),
        Err(Error::Type(message)) if message.contains("argument 1 expects string, got int")
    ));
}

#[test]
fn use_binds_like_let_and_never_modifies_an_existing_binding() {
    // `use` defines a module name in the current scope (like `let`): it must
    // never overwrite an existing variable — only `set` modifies a binding (§4).
    assert!(matches!(
        run(r#"(let io "user value") (use "io")"#),
        Err(Error::DuplicateBinding(name)) if name == "io"
    ));
    assert!(matches!(
        run(r#"(use "io") (use "io")"#),
        Err(Error::DuplicateBinding(name)) if name == "io"
    ));
    // Shadowing across scopes stays allowed and leaves the outer binding alone.
    let value = run(r#"(use "io") (() (use "str")) ($ "%t" (use "str"))"#).unwrap();
    assert!(matches!(value, Value::Str(text) if text == "struct"));
}
