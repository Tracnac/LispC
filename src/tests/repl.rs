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
