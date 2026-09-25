use super::run;
use crate::*;
use std::collections::VecDeque;

/// Drive the next `(repl)` evaluation with scripted lines instead of stdin.
fn feed(lines: &[&str]) {
    REPL_INPUT.with(|queue| {
        *queue.borrow_mut() = Some(VecDeque::from(
            lines
                .iter()
                .map(|line| line.to_string())
                .collect::<Vec<_>>(),
        ));
    });
}

/// Start capturing REPL console output (prompts, notices, command output and
/// echoed results) instead of writing it to the terminal.
fn capture_start() {
    REPL_OUTPUT.with(|output| *output.borrow_mut() = Some(Vec::new()));
}

/// Stop capturing and return everything the REPL printed, in order.
fn capture_take() -> String {
    REPL_OUTPUT
        .with(|output| output.borrow_mut().take().unwrap_or_default())
        .join("")
}

#[test]
fn repl_returns_the_last_evaluated_value_and_resumes_on_continue() {
    feed(&["(add 2 3)", ":c"]);
    let value = run(r#"(let first (repl)) (add first 10)"#).unwrap();
    REPL_INPUT.with(|queue| *queue.borrow_mut() = None);
    assert!(matches!(value, Value::Int(x) if x == 15));
}

#[test]
fn repl_set_mutates_program_variables() {
    feed(&["(set a 99)", ":c"]);
    let value = run(r#"(let a 1) (repl) a"#).unwrap();
    REPL_INPUT.with(|queue| *queue.borrow_mut() = None);
    assert!(matches!(value, Value::Int(x) if x == 99));
}

#[test]
fn repl_let_binds_only_inside_the_session() {
    feed(&["(let a 1)", ":c"]);
    let value = run(r#"(let a 10) (repl) a"#).unwrap();
    REPL_INPUT.with(|queue| *queue.borrow_mut() = None);
    assert!(matches!(value, Value::Int(x) if x == 10));
}

#[test]
fn repl_errors_are_reported_and_do_not_propagate() {
    feed(&["(no-such-name)", "(add 1 1)", ":c"]);
    let value = run(r#"(repl) (add 1 2)"#).unwrap();
    REPL_INPUT.with(|queue| *queue.borrow_mut() = None);
    assert!(matches!(value, Value::Int(x) if x == 3));
}

#[test]
fn repl_break_acts_on_the_enclosing_loop() {
    feed(&["(break)", ":c"]);
    let value = run(r#"(loop (repl)) (add 1 1)"#).unwrap();
    REPL_INPUT.with(|queue| *queue.borrow_mut() = None);
    assert!(matches!(value, Value::Int(x) if x == 2));
}

#[test]
fn repl_quit_aborts_the_run() {
    feed(&[":q"]);
    let error = match run(r#"(repl) (add 1 1)"#) {
        Err(error) => error,
        Ok(_) => panic!("expected the :q command to abort the run"),
    };
    REPL_INPUT.with(|queue| *queue.borrow_mut() = None);
    assert!(matches!(error, Error::Quit(message) if message.contains(":quit")));
}

#[test]
fn repl_accepts_an_optional_label_and_eof_returns_null() {
    // EOF (no lines at all) leaves the REPL immediately, returning Null.
    feed(&[]);
    let value = run(r#"(repl "breakpoint") (add 1 1)"#).unwrap();
    REPL_INPUT.with(|queue| *queue.borrow_mut() = None);
    assert!(matches!(value, Value::Int(x) if x == 2));
}

#[test]
fn open_repl_console_uses_the_queued_hook_without_touching_the_tty() {
    feed(&["(add 1 1)", ":c"]);
    let console = open_repl_console().expect("queued console");
    match console {
        ReplConsole::Queued(lines) => assert_eq!(lines.len(), 2),
        ReplConsole::Tty { .. } => panic!("expected the queued test console, not /dev/tty"),
    }
    REPL_INPUT.with(|queue| *queue.borrow_mut() = None);
}

#[test]
fn repl_context_prompt_shows_the_execution_point() {
    capture_start();
    feed(&[":c"]);
    run("(let a 1)\n(repl)\n").unwrap();
    REPL_INPUT.with(|queue| *queue.borrow_mut() = None);
    let out = capture_take();
    assert_eq!(out, "<test>:2> ");
}

#[test]
fn repl_i_lists_effective_bindings() {
    capture_start();
    feed(&[":i", ":c"]);
    run("(let x 1) (let y 2) (repl)").unwrap();
    REPL_INPUT.with(|queue| *queue.borrow_mut() = None);
    let out = capture_take();
    assert!(out.contains("x = 1\n"), "output was:\n{out}");
    assert!(out.contains("y = 2\n"), "output was:\n{out}");
    assert!(
        out.contains("2 bindings across 2 scopes"),
        "output was:\n{out}"
    );
}

#[test]
fn repl_i_marks_shadowed_bindings() {
    // The session sees the block's `a = 2`; the program-level `a = 1` is
    // shadowed and marked with `*` (its value is not listed again).
    capture_start();
    feed(&[":i", ":c"]);
    run("(let a 1) ((let a 2) (repl))").unwrap();
    REPL_INPUT.with(|queue| *queue.borrow_mut() = None);
    let out = capture_take();
    assert!(out.contains("a = 2 *\n"), "output was:\n{out}");
    assert!(!out.contains("a = 1"), "output was:\n{out}");
}

#[test]
fn repl_i_inspects_a_single_binding() {
    capture_start();
    feed(&[":i x", ":c"]);
    run("(let x 42) (repl)").unwrap();
    REPL_INPUT.with(|queue| *queue.borrow_mut() = None);
    let out = capture_take();
    assert!(
        out.contains("x = 42   int   scope 1 (enclosing)\n"),
        "output was:\n{out}"
    );
}

#[test]
fn repl_i_inspect_shows_the_shadow_chain() {
    capture_start();
    feed(&[":i a", ":c"]);
    run("(let a 1) ((let a 2) (repl))").unwrap();
    REPL_INPUT.with(|queue| *queue.borrow_mut() = None);
    let out = capture_take();
    assert!(
        out.contains("a = 2   int   scope 1 (enclosing)") && out.contains("outer a = 1 @ scope 2"),
        "output was:\n{out}"
    );
}

#[test]
fn repl_i_inspect_a_function_shows_its_definition_line() {
    capture_start();
    feed(&[":i fn1", ":c"]);
    run("(let fn1 (fn (n) (mul n 2)))\n(repl)\n").unwrap();
    REPL_INPUT.with(|queue| *queue.borrow_mut() = None);
    let out = capture_take();
    assert!(
        out.contains("fn1 = (fn fn1 (n))   function") && out.contains("def: <test>:1"),
        "output was:\n{out}"
    );
}

#[test]
fn repl_i_reports_unknown_bindings() {
    capture_start();
    feed(&[":i nope", ":c"]);
    run("(repl)").unwrap();
    REPL_INPUT.with(|queue| *queue.borrow_mut() = None);
    let out = capture_take();
    assert!(
        out.contains("no binding named `nope`"),
        "output was:\n{out}"
    );
}

#[test]
fn repl_l_shows_source_around_the_execution_point() {
    capture_start();
    feed(&[":l", ":c"]);
    run("(let a 1)\n(let b 2)\n(repl)\n(add a b)\n").unwrap();
    REPL_INPUT.with(|queue| *queue.borrow_mut() = None);
    let out = capture_take();
    assert!(
        out.contains("@ <test>:3 (execution point)\n"),
        "output was:\n{out}"
    );
    assert!(out.contains(" > 3  (repl)\n"), "output was:\n{out}");
    assert!(out.contains("(let a 1)"), "output was:\n{out}");
    assert!(out.contains("(add a b)"), "output was:\n{out}");
}

#[test]
fn repl_l_accepts_a_custom_window() {
    capture_start();
    feed(&[":l 1", ":c"]);
    run("(let a 1)\n(let b 2)\n(repl)\n(add a b)\n").unwrap();
    REPL_INPUT.with(|queue| *queue.borrow_mut() = None);
    let out = capture_take();
    assert!(out.contains("(let b 2)"), "output was:\n{out}");
    assert!(!out.contains("(let a 1)"), "output was:\n{out}");
}

#[test]
fn repl_bt_lists_the_call_stack_innermost_first() {
    capture_start();
    feed(&[":bt", ":c"]);
    run("(let inner (fn () (repl)))\n(let outer (fn () (inner)))\n(outer)\n").unwrap();
    REPL_INPUT.with(|queue| *queue.borrow_mut() = None);
    let out = capture_take();
    assert!(out.contains("backtrace (2 frames)"), "output was:\n{out}");
    assert!(out.contains("at <test>:2:"), "output was:\n{out}");
    assert!(out.contains("at <test>:3:"), "output was:\n{out}");
}

#[test]
fn repl_bt_is_empty_at_the_top_level() {
    capture_start();
    feed(&[":bt", ":c"]);
    run("(repl)").unwrap();
    REPL_INPUT.with(|queue| *queue.borrow_mut() = None);
    let out = capture_take();
    assert!(out.contains("backtrace is empty"), "output was:\n{out}");
}

#[test]
fn repl_interrupt_is_contained_to_the_session_and_restored_on_exit() {
    // An interrupt arriving while a session is open must cancel the current
    // line and keep the session alive; the pre-session state (here: Ctrl-C
    // already pending) is restored when the session ends. Driven through the
    // per-thread test hook so it never races other tests.
    feed(&["(add 1 1)", ":c"]);
    REPL_INTERRUPT.with(|flag| flag.set(true));
    let env = new_env(None);
    let result = run_repl(&env, 0, 0, "", Span { start: 0, end: 0 }, None);
    let restored = REPL_INTERRUPT.with(|flag| flag.get());
    REPL_INTERRUPT.with(|flag| flag.set(false));
    REPL_INPUT.with(|queue| *queue.borrow_mut() = None);
    assert!(result.is_ok(), "the session must survive the interrupt");
    assert!(restored, "the pre-session Ctrl-C state must be restored");
}
