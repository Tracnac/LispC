use super::{check, run};
use crate::*;

#[test]
fn shebang_line_is_stripped_before_lexing() {
    assert_eq!(
        strip_shebang("#!/usr/bin/env small-lisp\n(add 1 2)"),
        "(add 1 2)"
    );
    // A shebang-only script is an empty program.
    assert_eq!(strip_shebang("#!/usr/bin/env small-lisp"), "");
    assert_eq!(strip_shebang(""), "");
    // No shebang: source unchanged.
    assert_eq!(strip_shebang("(add 1 2)"), "(add 1 2)");
    // A `#!` that is not at byte 0 is not a shebang.
    assert_eq!(
        strip_shebang(" #!/bin/sh\n(add 1 2)"),
        " #!/bin/sh\n(add 1 2)"
    );
}

#[test]
fn script_with_shebang_still_runs() {
    let source = strip_shebang("#!/usr/bin/env small-lisp\n(expect (add 20 22) 42)");
    check(source, "t");
}

/// Runs `check` on a thread with a large explicit stack, mirroring the real
/// interpreter (main() runs on a 256 MB thread). The recursion tests must
/// reach MAX_CALL_DEPTH — far beyond what a 2 MB default test-thread stack
/// can physically hold — so the guard, not the harness, is what stops them.
/// The closure returns a Send-able verdict: Err(message) fails the test.
fn run_deep(check: impl FnOnce() -> Result<(), String> + Send + 'static) {
    let verdict = thread::Builder::new()
        .stack_size(INTERPRETER_STACK)
        .spawn(check)
        .unwrap()
        .join()
        .unwrap();
    if let Err(message) = verdict {
        panic!("{message}");
    }
}

#[test]
fn recursion_beyond_the_depth_limit_is_a_clean_error() {
    run_deep(|| match run("(let boom (fn () (boom))) (boom)") {
        Err(Error::Recursion(message)) if message.contains(&MAX_CALL_DEPTH.to_string()) => Ok(()),
        Err(other) => Err(format!(
            "expected RecursionError mentioning the limit, got: {other}"
        )),
        Ok(_) => Err("runaway recursion must not succeed".into()),
    });
}

#[test]
fn recursion_that_stays_within_the_limit_still_works() {
    // Count down and return: a genuine recursion that reaches the very edge
    // of the limit without tripping the guard. (`(down N)` deepens to N+1
    // live frames, so N = limit - 1 lands exactly on the limit.)
    let program = format!(
        "(let down (fn (n) (if (eq n 0) t (down (sub n 1))))) (down {})",
        MAX_CALL_DEPTH - 1
    );
    run_deep(move || match run(&program) {
        Ok(value) if matches!(&value, Value::Bool(true)) => Ok(()),
        Ok(_) => Err("expected t from the countdown".into()),
        Err(error) => Err(format!(
            "recursion within the limit must succeed, got: {error}"
        )),
    });
}

#[test]
fn recursion_error_diagnostic_carries_the_call_chain() {
    // The guard's span and call trace live in thread-locals, so the whole
    // error path must run on the fat thread too (Value is not Send).
    let source = "(let boom (fn () (boom))) (boom)".to_owned();
    run_deep(move || {
        let error = match run(&source) {
            Err(error) => error,
            Ok(_) => return Err("expected runtime error".into()),
        };
        let span = LAST_ERROR_SPAN.with(|span| span.get());
        let rendered = diagnostic(&error, &source, "r.lisp", span);
        let checks = [
            rendered.contains("RecursionError"),
            rendered.contains("call trace:"),
            rendered.contains("boom at r.lisp:1:"),
            rendered.contains("more frame(s) omitted"),
        ];
        if checks.into_iter().all(|ok| ok) {
            Ok(())
        } else {
            Err(format!("unexpected diagnostic was:\n{rendered}"))
        }
    });
}

#[test]
fn closure_recursion_and_integer_division_work() {
    check(
        "(let fact (fn (n) (if (eq n 0) 1 (mul n (fact (sub n 1)))))) (add (fact 5) (div 7 2))",
        "123",
    );
}

#[test]
fn variadic_builtins_follow_their_declared_arities() {
    check(
        "(expect (add) 0) (expect (add 1 2 3 4) 10) (expect (mul) 1) (expect (mul 2 3 4) 24) (expect (sub 10) -10) (expect (sub 10 3 2) 5) (expect (div 20) 0.05) (expect (div 20 2 2) 5) (expect (eq) t) (expect (eq 1) t) (expect (ne) f) (expect (ne 1) f) (expect (ne 1 2 1) t) (expect (ne 1 2 3) t) (expect (lt 1 2 3) t) (expect (ge 3 2 2) t) (expect (bit-and) -1) (expect (bit-and 15 7 3) 3) (expect (bit-or) 0) (expect (bit-or 1 2 4) 7) (expect (bit-xor 7 3 1) 5)",
        "t",
    );
}

#[test]
fn fixed_builtin_and_user_function_arities_remain_strict() {
    for source in ["(mod 4)", "(mod 4 2 1)", "(pow 2)", "(bit-not 1 2)"] {
        assert!(matches!(run(source), Err(Error::Arity(_))));
    }
    assert!(matches!(
        run("(let sum (fn (x y) (add x y))) (sum 1)"),
        Err(Error::Arity(_))
    ));
    assert!(matches!(
        run("(let sum (fn (x y) (add x y))) (sum 1 2 3)"),
        Err(Error::Arity(_))
    ));
}

#[test]
fn references_mutate_the_original_location() {
    check(
        "(let a [1 2]) (let setzero (fn (x) (set x[1] 0))) (setzero ^a) a[1]",
        "0",
    );
}

#[test]
fn references_are_not_callable() {
    assert!(matches!(
        run("(let a {value: \"Hello\"}) (^a)"),
        Ok(Value::Ref(_))
    ));
    let function = Value::Function(Rc::new(Function {
        params: vec![],
        body: Expr {
            kind: ExprKind::Lit(Literal::Null),
            span: Span { start: 0, end: 0 },
        },
        env: new_env(None),
        name: None,
    }));
    assert!(!is_callable(&Value::Ref(Rc::new(RefLocation {
        root: Rc::new(RefCell::new(function)),
        path: vec![],
    }))));
}

#[test]
fn cyclic_references_are_reported_as_lisp_errors() {
    for source in [
        "(let x _) (set x ^x)",
        "(let x _) (let y _) (set x ^y) (set y ^x)",
        "(let x _) (set x [^x])",
        "(let x _) (set x {self: ^x})",
        "(let a [1 2]) (set a[1] (^ a))",
        "(let a [1 2]) (set a[2] (^ a[1])) (set a[1] (^ a[2]))",
        "(let s {x: 1}) (set s.x (^ s))",
        "(let a [1 2]) (let b (^ a[1])) (set b (^ a))",
    ] {
        assert!(matches!(
            run(source),
            Err(Error::Type(message)) if message == "cyclic reference"
        ));
    }
}

#[test]
fn aliasing_outside_of_the_referenced_collection_is_not_cyclic() {
    assert!(run("(let a [1 2]) (set a[2] (^ a[1])) a").is_ok());
    assert!(run("(let a [1 2]) (let b [3 4]) (set a[1] (^ b[1])) a").is_ok());
}

#[test]
fn stale_references_are_rejected_not_snapshots() {
    // Model P: a Ref identifies a logical location (root cell + path), never
    // an intermediate node's current cell. Replacing the whole value at the
    // root therefore invalidates element/field aliases; resolving one must
    // fail deterministically — and must never expose the old element.
    assert!(matches!(
        run("(let a [1 2]) (let p (^ a[1])) (set a 5) p"),
        Err(Error::Type(message)) if message == "indexing requires an array"
    ));
    assert!(matches!(
        run(r#"(let a [{name:"Alice"}]) (let p (^ a[1].name)) (set a 42) p"#),
        Err(Error::Type(message)) if message == "indexing requires an array"
    ));
    // Reading through a reference that WALKS back up to a still-valid element
    // survives (the location exists again); only resolving must be exact.
    check(
        "(let a [1 2]) (let p (^ a[1])) (set a [9 8 7]) (expect p 9) t",
        "t",
    );
    // Renderers degrade to placeholders; fallible serializers propagate the
    // deref failure instead of printing stale data.
    let stale = Value::Ref(Rc::new(RefLocation {
        root: Rc::new(RefCell::new(Value::Int(5))),
        path: vec![PathStep::Index(1)],
    }));
    assert_eq!(render(&stale), "<invalid reference>");
    assert_eq!(render_nested(&stale), "null");
    assert_eq!(debug_render(&stale), "Ref(<invalid>)");
    assert!(lisp_source(&stale).is_err());
    assert!(json_render(&stale).is_err());
    // equals: a stale reference resolves to nothing and equals nothing.
    assert!(!equals(&stale, &Value::Int(5)));
}

#[test]
fn writes_write_through_intermediates_and_replace_at_the_leaf() {
    // Write-through at an intermediate position: a[1] holds an alias to b, so
    // a[1][2] writes b[2] while a keeps the alias.
    check(
        "(let a [1 2]) (let b [3 4]) (set a[1] (^ b)) (set a[1][2] 9) (expect b [3 9]) (expect a[1] [3 9]) t",
        "t",
    );
    // Replace-at-leaf: writing to a position that currently holds an alias
    // replaces that alias itself (it is a normal value), it does not write
    // through — the referenced location is untouched.
    check(
        "(let a [1 2]) (let b [9]) (set a[1] (^ b[1])) (set a[1] 7) (expect a[1] 7) (expect b [9]) t",
        "t",
    );
}

#[test]
fn rebinding_a_value_copies_it_deeply_and_independently() {
    // Value semantics: a rebind deep-copies, so mutating the source (or the
    // copy) never leaks into the other side. Only `^` shares (aliases tests).
    check(
        "(let a [1 2]) (let b a) (set a[1] 9) (expect b [1 2]) t",
        "t",
    );
    check(
        "(let a [1 2]) (let b a) (set b[2] 9) (expect a [1 2]) t",
        "t",
    );
    // Structs.
    check("(let s {x: 1}) (let c s) (set s.x 9) (expect c.x 1) t", "t");
    check("(let s {x: 1}) (let c s) (set c.x 9) (expect s.x 1) t", "t");
    // Nested structure: the copy is recursive, field by field.
    check(
        "(let a {x: [1 2]}) (let b a) (set b.x[1] 9) (expect a.x [1 2]) t",
        "t",
    );
    check(
        "(let a {x: [1 2]}) (let b a) (set a.x[1] 9) (expect b.x [1 2]) t",
        "t",
    );
}

#[test]
fn function_arguments_are_copies_unless_aliased() {
    // By-value: the callee's writes hit its own copy.
    check(
        "(let bump (fn (x) ((set x[1] 9) x))) (let a [1 2]) (bump a) (expect a [1 2]) t",
        "t",
    );
    // ^ makes the argument an alias: the callee mutates the caller's value.
    check(
        "(let bump (fn (x) (set x[1] 9))) (let a [1 2]) (bump ^a) (expect a [9 2]) t",
        "t",
    );
}

#[test]
fn aliases_write_through_in_both_directions() {
    // Whole-variable alias.
    check(
        "(let a [1 2]) (let q (^ a)) (set a[1] 9) (expect q [9 2]) t",
        "t",
    );
    check(
        "(let a [1 2]) (let q (^ a)) (set q[1] 9) (expect a [9 2]) t",
        "t",
    );
    // Element alias.
    check(
        "(let a [1 2]) (let p (^ a[1])) (set a[1] 9) (expect p 9) t",
        "t",
    );
    check(
        "(let a [1 2]) (let p (^ a[1])) (set p 9) (expect a[1] 9) t",
        "t",
    );
    // Struct field alias.
    check(
        "(let s {x: 1}) (let p (^ s.x)) (set s.x 9) (expect p 9) t",
        "t",
    );
    check(
        "(let s {x: 1}) (let p (^ s.x)) (set p 9) (expect s.x 9) t",
        "t",
    );
    // Nested alias: rewrites through an intermediate location.
    check(
        "(let a {x: [1 2]}) (let p (^ a.x[1])) (set a.x[1] 9) (expect p 9) t",
        "t",
    );
    check(
        "(let a {x: [1 2]}) (let p (^ a.x[1])) (set p 9) (expect a.x [9 2]) t",
        "t",
    );
}

#[test]
fn index_reads_via_location_shaped_bases_copy_only_the_element() {
    // A1 routes location-shaped index bases (var/field/nested index) straight
    // to the cell; these must behave exactly like the old evaluate-then-index
    // path, which the rest of the suite exercises too.
    // String single-index reads still dispatch to grapheme slicing.
    check("(let s {msg: \"hello\"}) (expect s.msg[1] \"h\") t", "t");
    // Slice reads keep evaluating the base as a value, not a location.
    check("(let xs [1 2 3 4 5]) (expect xs[1..3] [1 2 3]) t", "t");
    // Index-of-index reads are still deep value reads (no aliasing).
    check("(let a [[1 2] [3 4]]) (expect a[2][1] 3) t", "t");
    check(
        "(let a [[1 2] [3 4]]) (let p a[2][1]) (set a[2][1] 9) (expect p 3) t",
        "t",
    );
}

#[test]
fn expect_checks_values_and_reports_comments() {
    let value = run("(expect (eq 1 1) t \"integers compare equally\")").unwrap();
    assert!(matches!(value, Value::Bool(true)));

    let value = run("(expect (add 1 1) (sub 3 1))").unwrap();
    assert!(matches!(value, Value::Bool(true)));

    assert!(matches!(
        run("(expect (add 1 1) 3 \"addition regression\")"),
        Err(Error::Expect(message))
            if message.contains("addition regression")
                && message.contains("expected Int(3), got Int(2)")
    ));
}

#[test]
fn duplicate_let_in_same_scope_is_an_error() {
    assert!(matches!(
        run("(let x 1) (let x 2)"),
        Err(Error::DuplicateBinding(name)) if name == "x"
    ));
    assert!(matches!(
        run("(let x 1) (set x 2) (let x 3)"),
        Err(Error::DuplicateBinding(name)) if name == "x"
    ));
}

#[test]
fn let_shadows_bindings_from_outer_scopes() {
    check("(let x 1) ((let x 2) (expect x 2)) x", "1");
}

#[test]
fn binding_form_and_builtin_names_is_rejected() {
    for src in [
        "(let let 1)",
        "(let set 1)",
        "(let if 1)",
        "(let fn 1)",
        "(let expect 1)",
        "(let add 1)",
        "(let $ 1)",
        "(let eval 1)",
    ] {
        assert!(
            matches!(run(src), Err(Error::Type(message)) if message.contains("reserved name cannot be bound")),
            "expected reserved-name error for {src}"
        );
    }
    // The define-then-use module flow goes through the same check: the defining
    // `let` is evaluated normally by `use` and rejects reserved names.
    assert!(matches!(
        run("(let set 1) (use \"set\")"),
        Err(Error::Type(message)) if message.contains("reserved name cannot be bound")
    ));
    // Function parameters are protected too.
    assert!(matches!(
        run("(let bad (fn (let) 1))"),
        Err(Error::Type(message)) if message.contains("reserved name cannot be used as a parameter")
    ));
    assert!(matches!(
        run("(let bad (fn (x add) 1))"),
        Err(Error::Type(message)) if message.contains("reserved name cannot be used as a parameter")
    ));
}

#[test]
fn repeated_let_in_loop_scope_is_a_duplicate_error() {
    assert!(matches!(
        run("(loop\n  (let line 0)\n  (set line (add line 1))\n  (if (gt line 1) (break line)))"),
        Err(Error::DuplicateBinding(name)) if name == "line"
    ));
}

#[test]
fn set_updates_loop_scope_bindings_without_creating_new_ones() {
    check(
        "(let count 0) (loop (set count (add count 1)) (if (eq count 5) (break count)))",
        "5",
    );
    check(
        "(loop (let i 0) (set i (add i 1)) (if (eq i 1) (break i)))",
        "1",
    );
    check(
        "(loop (() (let i 0) (set i (add i 1)) (if (eq i 1) (break i))))",
        "1",
    );
}

#[test]
fn set_does_not_create_a_new_binding() {
    assert!(matches!(run("(set missing 1)"), Err(Error::Name(_))));
}

#[test]
fn reference_value_has_ref_type_tag() {
    check(
        r#"((let value 42)($ "%t:%s" (^ value) (^ value)))"#,
        r#""ref:42""#,
    );
}

#[test]
fn set_updates_existing_binding() {
    check(
        r#"(let x 1)
(set x 2)
x"#,
        r#"2"#,
    );
}

#[test]
fn let_binding_is_readable_after_definition() {
    check(
        r#"(let x 1)
x"#,
        r#"1"#,
    );
}

#[test]
fn set_inside_block_mutates_outer_binding() {
    check(
        r#"(let x 1)
(
  (let y 2)
  (set x 3)
)
x"#,
        r#"3"#,
    );
}

#[test]
fn loop_break_value_becomes_let_value() {
    check(
        r#"(let x 1)
(let result
  (loop
    (if (eq x 0) (break "fini"))
    (set x (sub x 1))))
result"#,
        r#""fini""#,
    );
}

#[test]
fn loop_mutates_outer_binding_until_break() {
    check(
        r#"(let x 1)
(let result
  (loop
    (if (eq x 0) (break "fini"))
    (set x (sub x 1))))
x"#,
        r#"0"#,
    );
}

#[test]
fn loop_break_value_is_counter_at_five() {
    check(
        r#"(let x 1)
(let result
  (loop
    (if (eq x 0) (break "fini"))
    (set x (sub x 1))))
(set result
  (loop
    (if (eq x 5)
        (break x))
    (set x (add x 1))))
result"#,
        r#"5"#,
    );
}

#[test]
fn loop_counter_reaches_five() {
    check(
        r#"(let x 1)
(let result
  (loop
    (if (eq x 0) (break "fini"))
    (set x (sub x 1))))
(set result
  (loop
    (if (eq x 5)
        (break x))
    (set x (add x 1))))
x"#,
        r#"5"#,
    );
}

#[test]
fn loop_result_counts_iterations_after_continue_skips() {
    check(
        r#"(let x 1)
(let result
  (loop
    (if (eq x 0) (break "fini"))
    (set x (sub x 1))))
(let count 0)
(set result
  (loop
    (if (eq x 5)
        (break count))
    (set x (add x 1))
    (if (eq x 3)
        (continue))
    (set count (add count 1))))
result"#,
        r#"4"#,
    );
}

#[test]
fn loop_count_skips_increment_after_continue() {
    check(
        r#"(let x 1)
(let result
  (loop
    (if (eq x 0) (break "fini"))
    (set x (sub x 1))))
(let count 0)
(set result
  (loop
    (if (eq x 5)
        (break count))
    (set x (add x 1))
    (if (eq x 3)
        (continue))
    (set count (add count 1))))
count"#,
        r#"4"#,
    );
}

#[test]
fn loop_counter_reaches_five_before_break() {
    check(
        r#"(let x 1)
(let result
  (loop
    (if (eq x 0) (break "fini"))
    (set x (sub x 1))))
(let count 0)
(set result
  (loop
    (if (eq x 5)
        (break count))
    (set x (add x 1))
    (if (eq x 3)
        (continue))
    (set count (add count 1))))
x"#,
        r#"5"#,
    );
}

#[test]
fn loop_continue_guard_falls_through_to_break() {
    check(
        r#"(let x 1)
(let result
  (loop
    (if (eq x 0) (break "fini"))
    (set x (sub x 1))))
(set result
  (loop
    (set x (add x 1))
    (if (lt x 5)
        (continue))
    (break x)))
result"#,
        r#"5"#,
    );
}

#[test]
fn loop_skips_three_then_breaks_at_five() {
    check(
        r#"(let x 1)
(let result
  (loop
    (if (eq x 0) (break "fini"))
    (set x (sub x 1))))
(set result
  (loop
    (set x (add x 1))
    (if (eq x 3)
        (continue))
    (if (eq x 5)
        (break x))))
result"#,
        r#"5"#,
    );
}

#[test]
fn set_overwrites_binding_value() {
    check(
        r#"(let x 1)
(set x 10)
x"#,
        r#"10"#,
    );
}

#[test]
fn set_inside_bare_block_updates_outer_binding() {
    check(
        r#"(let x 1)
(
  (set x 42)
)
x"#,
        r#"42"#,
    );
}

#[test]
fn loop_bare_break_stops_counter_at_five() {
    check(
        r#"(let x 1)
(
  (loop
    (if (eq x 5)
        (break))
    (set x (add x 1)))
)
x"#,
        r#"5"#,
    );
}

#[test]
fn outer_loop_break_value_wins() {
    check(
        r#"(let x 1)
(let result
  (loop
    (if (eq x 0) (break "fini"))
    (set x (sub x 1))))
(let outer 0)
(let inner 0)
(set result
  (loop
    (set outer (add outer 1))
    (set inner 0)

    (loop
      (set inner (add inner 1))
      (if (eq inner 3)
          (break "inner")))

    (if (eq outer 2)
        (break "outer"))))
result"#,
        r#""outer""#,
    );
}

#[test]
fn outer_loop_runs_twice() {
    check(
        r#"(let x 1)
(let result
  (loop
    (if (eq x 0) (break "fini"))
    (set x (sub x 1))))
(let outer 0)
(let inner 0)
(set result
  (loop
    (set outer (add outer 1))
    (set inner 0)

    (loop
      (set inner (add inner 1))
      (if (eq inner 3)
          (break "inner")))

    (if (eq outer 2)
        (break "outer"))))
outer"#,
        r#"2"#,
    );
}

#[test]
fn inner_loop_runs_three_times_per_outer_pass() {
    check(
        r#"(let x 1)
(let result
  (loop
    (if (eq x 0) (break "fini"))
    (set x (sub x 1))))
(let outer 0)
(let inner 0)
(set result
  (loop
    (set outer (add outer 1))
    (set inner 0)

    (loop
      (set inner (add inner 1))
      (if (eq inner 3)
          (break "inner")))

    (if (eq outer 2)
        (break "outer"))))
inner"#,
        r#"3"#,
    );
}

#[test]
fn outer_break_wins_when_inner_loop_breaks_immediately() {
    check(
        r#"(let x 1)
(let result
  (loop
    (if (eq x 0) (break "fini"))
    (set x (sub x 1))))
(let outer 0)
(let inner 0)
(set result
  (loop
    (set outer (add outer 1))
    (set inner 0)

    (loop
      (set inner (add inner 1))
      (break "inner"))

    (if (eq outer 3)
        (break "outer"))))
result"#,
        r#""outer""#,
    );
}

#[test]
fn outer_loop_runs_three_times() {
    check(
        r#"(let x 1)
(let result
  (loop
    (if (eq x 0) (break "fini"))
    (set x (sub x 1))))
(let outer 0)
(let inner 0)
(set result
  (loop
    (set outer (add outer 1))
    (set inner 0)

    (loop
      (set inner (add inner 1))
      (break "inner"))

    (if (eq outer 3)
        (break "outer"))))
outer"#,
        r#"3"#,
    );
}

#[test]
fn inner_loop_runs_once_before_breaking() {
    check(
        r#"(let x 1)
(let result
  (loop
    (if (eq x 0) (break "fini"))
    (set x (sub x 1))))
(let outer 0)
(let inner 0)
(set result
  (loop
    (set outer (add outer 1))
    (set inner 0)

    (loop
      (set inner (add inner 1))
      (break "inner"))

    (if (eq outer 3)
        (break "outer"))))
inner"#,
        r#"1"#,
    );
}

#[test]
fn user_function_returns_added_arguments() {
    check(
        r#"(let sum-two
  (fn (a b)
    (add a b)))
(sum-two 1 2)"#,
        r#"3"#,
    );
}

#[test]
fn user_function_arguments_evaluate_left_to_right() {
    check(
        r#"(let x 1)
(let record
  (fn (value)
    ((set x value) value)))
(add
  (record 1)
  (record 2)
  (record 3))
x"#,
        r#"3"#,
    );
}

#[test]
fn let_shadows_binding_inside_block() {
    check("(let x 1) ((let x 2) x)", "2");
}

#[test]
fn set_inside_block_mutates_inner_shadow() {
    check("(let x 1) ((let x 2) (set x 3) x)", "3");
}

#[test]
fn loop_scope_preserves_outer_binding() {
    check("(let x 10) ((loop (let y 42) (break y))) x", "10");
}

#[test]
fn nested_blocks_inner_binding_does_not_leak() {
    check("(let x 1) ((let x 2) ((let x 3) x) x) x", "1");
}

#[test]
fn nested_blocks_middle_binding_shadows_outer() {
    check("(let x 1) ((let x 2) ((let x 3) x) x)", "2");
}

#[test]
fn nested_blocks_inner_binding_wins() {
    check("(let x 1) ((let x 2) ((let x 3) x))", "3");
}

#[test]
fn eval_returns_the_last_forms_value() {
    check(r#"(eval "(add 1 2)")"#, "3");
    check(r#"(eval "(let a 1)(add a 2)")"#, "3");
    check(r#"(eval "")"#, "_");
}

#[test]
fn eval_accepts_variable_names_and_source_in_variables() {
    check(r#"(let x 42) (eval "x")"#, "42");
    check(r#"(let code "(mul 2 3)") (eval code)"#, "6");
}

#[test]
fn eval_runs_in_the_callers_environment() {
    check(r#"(let y 5) (eval "(add y 1)")"#, "6");
    check(r#"(let z 1) (eval "(set z 9)") z"#, "9");
    check(r#"(eval "(let fresh 7)") fresh"#, "7");
}

#[test]
fn eval_propagates_control_flow_to_the_callers_loop() {
    check(r#"(loop (eval "(break 42)"))"#, "42");
    check(r#"(let i 0) (loop (eval "(break)") (set i 1)) i"#, "0");
}

#[test]
fn eval_propagates_errors_from_the_evald_code() {
    assert!(matches!(run(r#"(eval "(div 1 0)")"#), Err(Error::Math(_))));
    assert!(matches!(
        run(r#"(eval "missing")"#),
        Err(Error::Name(name)) if name == "missing"
    ));
    assert!(matches!(run(r#"(eval "(add")"#), Err(Error::Parse(_))));
    assert!(matches!(
        run(r#"(eval "(let let 1)")"#),
        Err(Error::Type(message)) if message.contains("reserved name cannot be bound")
    ));
}

#[test]
fn eval_rejects_non_string_arguments_and_bad_arity() {
    for src in ["(eval 5)", "(eval [1 2])", "(eval t)", "(eval (fn (x) x))"] {
        assert!(
            matches!(run(src), Err(Error::Type(message)) if message == "expected string"),
            "expected type error for {src}"
        );
    }
    assert!(matches!(run("(eval)"), Err(Error::Arity(_))));
    assert!(matches!(run(r#"(eval "1" "2")"#), Err(Error::Arity(_))));
}

#[test]
fn eval_failure_keeps_the_diagnostic_pointing_at_the_call() {
    let source = r#"(let a 1) (eval "(div 1 0)") (add a 1)"#;
    assert!(run(source).is_err());
    let span = LAST_ERROR_SPAN
        .with(|span| span.get())
        .expect("span recorded");
    assert!(span.start <= source.len() && span.end <= source.len());
    assert!(
        source[..span.end].contains("eval"),
        "span should cover the eval call, got {span:?}"
    );
}
