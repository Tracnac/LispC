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
fn repl_lexer_errors_are_not_reported_against_the_previous_line() {
    // A lexer error used to be reported against the span the parser left behind
    // from the previous line, so it came back at the wrong column, with the
    // caret past the end of a short line. The error is deliberately not in
    // column 1, because a stale span and a cleared one only differ elsewhere.
    capture_start();
    feed(&[
        "\"a much longer line than the next one\"",
        "(let a 1) \"abc",
        "(add 1 1)",
        ":c",
    ]);
    run("(repl)").unwrap();
    REPL_INPUT.with(|queue| *queue.borrow_mut() = None);
    // The whole session transcript. The second line is the point: it used to
    // read the offset the first line's last token left behind.
    let out = capture_take();
    assert_eq!(
        out,
        concat!(
            "<test>:1> \"a much longer line than the next one\"\n",
            "<test>:1> <repl>:1:11: ParseError: unterminated string\n",
            "(let a 1) \"abc\n",
            "          ^\n",
            "<test>:1> 2\n",
            "<test>:1> ",
        ),
        "unexpected session transcript"
    );
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
    // The label names the session, and it shows in the prompt.
    capture_start();
    feed(&[":c"]);
    run(r#"(let a 1)
(repl "breakpoint")"#)
    .unwrap();
    REPL_INPUT.with(|queue| *queue.borrow_mut() = None);
    let out = capture_take();
    assert!(out.contains("breakpoint@<test>:2> "), "output was:\n{out}");

    // No label, so the prompt is the bare execution point.
    capture_start();
    feed(&[":c"]);
    run("(repl)").unwrap();
    REPL_INPUT.with(|queue| *queue.borrow_mut() = None);
    let out = capture_take();
    assert!(out.contains("<test>:1> "), "output was:\n{out}");
    assert!(!out.contains('@'), "an unlabelled prompt has no @:\n{out}");

    // EOF (no lines at all) leaves the REPL immediately, returning Null, so a
    // bare (repl) at the end of a program evaluates to Null.
    feed(&[]);
    let value = run(r#"(repl "breakpoint")"#).unwrap();
    REPL_INPUT.with(|queue| *queue.borrow_mut() = None);
    assert!(matches!(value, Value::Null));

    // The session resumes normally on :c, and the program carries on.
    feed(&[":c"]);
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
fn repl_console_sinks_send_a_real_session_to_the_terminal_only() {
    // A real session writes to the controlling terminal, never to the program's
    // own stdout or stderr, so redirecting a program's output cannot capture or
    // interleave with the session. A temporary file stands in for the terminal
    // here, because opening /dev/tty needs a controlling terminal the test
    // runner has no reason to have, and the routing does not care which file it
    // is.
    let path = std::env::temp_dir().join(format!("repl-sink-{}", std::process::id()));
    let file = || {
        fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(&path)
    };
    let mut console = ReplConsole::Tty {
        reader: BufReader::new(file().expect("scratch reader")),
        writer: file().expect("scratch writer"),
    };

    assert_eq!(console.echo_sink(), ReplSink::Terminal);
    assert_eq!(console.notify_sink(), ReplSink::Terminal);

    console.echo("42");
    console.notify("prompt> ");
    drop(console);
    let written = fs::read_to_string(&path).expect("scratch contents");
    assert_eq!(written, "42\nprompt> ", "both go to the terminal handle");
    let _ = fs::remove_file(&path);
}

#[test]
fn repl_capture_hook_never_hijacks_a_real_session() {
    // The capture hook exists to collect test output. If it were consulted for
    // a real session it would divert a terminal session into a buffer, so the
    // terminal arm must win even with a capture collecting.
    capture_start();
    let path = std::env::temp_dir().join(format!("repl-hijack-{}", std::process::id()));
    let file = || {
        fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(&path)
    };
    let mut console = ReplConsole::Tty {
        reader: BufReader::new(file().expect("scratch reader")),
        writer: file().expect("scratch writer"),
    };
    assert_eq!(console.echo_sink(), ReplSink::Terminal);
    assert_eq!(console.notify_sink(), ReplSink::Terminal);
    console.echo("42");
    drop(console);
    let written = fs::read_to_string(&path).expect("scratch contents");
    let _ = fs::remove_file(&path);
    assert_eq!(written, "42\n", "the echo still reaches the terminal");
    assert!(
        capture_take().is_empty(),
        "a real session must not write into the capture hook"
    );
}

#[test]
fn repl_console_sinks_send_a_queued_session_to_the_program_streams() {
    // Without a capture collecting, the test console falls back to the
    // program's own streams: a result on stdout, a prompt on stderr.
    REPL_OUTPUT.with(|output| *output.borrow_mut() = None);
    let console = ReplConsole::Queued(VecDeque::new());
    assert_eq!(console.echo_sink(), ReplSink::ProgramStdout);
    assert_eq!(console.notify_sink(), ReplSink::ProgramStderr);
    drop(console);

    // With a capture collecting, it is preferred over both.
    capture_start();
    let console = ReplConsole::Queued(VecDeque::new());
    assert_eq!(console.echo_sink(), ReplSink::Captured);
    assert_eq!(console.notify_sink(), ReplSink::Captured);
    drop(console);
    capture_take();
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
fn repl_interrupt_before_the_prompt_is_reported_and_redraws() {
    // A Ctrl-C that lands before the prompt is drawn has no line to cancel: it
    // is reported and the session draws a fresh prompt. Driven through the
    // per-thread hook so it never races the process-wide INTERRUPTED flag.
    capture_start();
    feed(&["(add 1 1)", ":c"]);
    REPL_INTERRUPT.with(|flag| flag.set(true));
    let env = new_env(None);
    let result = run_repl(&env, 0, "", Span { start: 0, end: 0 }, None);
    let restored = REPL_INTERRUPT.with(|flag| flag.get());
    REPL_INTERRUPT.with(|flag| flag.set(false));
    REPL_INPUT.with(|queue| *queue.borrow_mut() = None);
    assert!(result.is_ok(), "the session must survive the interrupt");
    assert!(restored, "the pre-session Ctrl-C state must be restored");
    // The whole session: the cancel is reported once, a fresh prompt is drawn,
    // and the line after it still runs and is echoed.
    let out = capture_take();
    assert_eq!(
        out, "repl: interrupted (:c continues, :q quits)\nrepl> 2\nrepl> ",
        "unexpected session transcript"
    );
}

#[test]
fn repl_interrupt_during_the_read_keeps_the_line_that_follows_it() {
    // A real Ctrl-C is only visible once a whole line has been typed, because
    // the terminal driver has already flushed what was being typed and std
    // retries the read. The line the read returns is therefore the one typed
    // *after* the cancel, so it must be evaluated and echoed. Cancelling it too
    // used to swallow it: the result was never shown and the input was lost.
    capture_start();
    feed(&["(add 1 1)", ":c"]);
    REPL_INTERRUPT_AFTER_READ.with(|flag| flag.set(true));
    let env = new_env(None);
    let result = run_repl(&env, 0, "", Span { start: 0, end: 0 }, None);
    REPL_INTERRUPT_AFTER_READ.with(|flag| flag.set(false));
    REPL_INPUT.with(|queue| *queue.borrow_mut() = None);
    assert!(result.is_ok(), "the session must survive the interrupt");
    // The whole session. The prompt is drawn before the read, so the notice
    // lands after it: the cancel is reported once, then the line typed after it
    // is evaluated and its result echoed. It used to be dropped here, so the
    // transcript ended at the notice.
    let out = capture_take();
    assert_eq!(
        out, "repl> repl: interrupted (:c continues, :q quits)\n2\nrepl> ",
        "unexpected session transcript"
    );
}

#[test]
fn repl_interrupt_hooks_are_restored_when_the_session_ends() {
    // Both interrupt hooks are per-thread state that a session consumes. The
    // pre-session value has to come back on exit, so a hook set by a test is
    // still set afterwards and cannot leak into the next session.
    feed(&[":c"]);
    REPL_INTERRUPT.with(|flag| flag.set(false));
    REPL_INTERRUPT_AFTER_READ.with(|flag| flag.set(true));
    let env = new_env(None);
    let result = run_repl(&env, 0, "", Span { start: 0, end: 0 }, None);
    let after_read = REPL_INTERRUPT_AFTER_READ.with(|flag| flag.get());
    REPL_INTERRUPT_AFTER_READ.with(|flag| flag.set(false));
    REPL_INPUT.with(|queue| *queue.borrow_mut() = None);
    assert!(result.is_ok(), "the session must end cleanly");
    assert!(
        after_read,
        "the pre-session state of the after-read hook must be restored"
    );
}

#[test]
fn a_ctrl_c_opens_a_session_and_the_program_continues_after_it() {
    // A Ctrl-C no longer ends the run. It opens a session at the point it
    // interrupted, `:c` resumes the interrupted computation, and the program
    // carries on to its end.
    capture_start();
    feed(&["(add 1 2)", ":c"]);
    INTERRUPT_PENDING.with(|flag| flag.set(true));
    let value = run("(add 20 22)").unwrap();
    INTERRUPT_PENDING.with(|flag| flag.set(false));
    REPL_INPUT.with(|queue| *queue.borrow_mut() = None);
    assert!(
        matches!(value, Value::Int(x) if x == 42),
        "the program resumed"
    );
    // The notice comes before the first prompt, so a session that was not asked
    // for says why it is there, and the session's own work is echoed.
    let out = capture_take();
    assert!(
        out.starts_with("repl: interrupted (:c continues, :q quits)\n<test>:1> "),
        "unexpected session transcript:\n{out}"
    );
    assert!(
        out.contains("<test>:1> 3\n"),
        "the session evaluated its line:\n{out}"
    );
}

#[test]
fn a_ctrl_c_session_can_quit_the_whole_run() {
    // `:q` at any depth ends the run, so a session opened by a Ctrl-C is a way
    // to stop the program after all.
    feed(&[":q"]);
    INTERRUPT_PENDING.with(|flag| flag.set(true));
    let outcome = run("(add 20 22)");
    INTERRUPT_PENDING.with(|flag| flag.set(false));
    REPL_INPUT.with(|queue| *queue.borrow_mut() = None);
    match outcome {
        Err(Error::Quit(message)) => assert!(message.contains("aborted by user")),
        _ => panic!("the run must end with Quit"),
    }
}

#[test]
fn a_ctrl_c_session_reads_and_writes_the_scope_it_was_opened_with() {
    // The session is opened with the scope that was live where the interrupt
    // landed, so reads and sets reach it. The loop check hands over the loop's
    // own scope for exactly this reason: that is what lets a loop which will
    // not finish be inspected and corrected where it stopped, rather than from
    // outside. Driven through the entry point directly, because a pending
    // interrupt set before a program starts is consumed by its first node, long
    // before any loop exists.
    let env = new_env(None);
    let setup = |env: &EnvRef, source: &str| {
        let program = Parser {
            ts: lex(source).unwrap(),
            i: 0,
        }
        .program()
        .unwrap();
        for form in &program {
            if eval(form, env, 0).is_err() {
                panic!("setup failed");
            }
        }
    };
    let read = |env: &EnvRef| {
        let program = Parser {
            ts: lex("n").unwrap(),
            i: 0,
        }
        .program()
        .unwrap();
        match eval(&program[0], env, 0) {
            Ok(Value::Int(n)) => n,
            _ => panic!("n must stay an integer"),
        }
    };
    setup(&env, "(let n 5)");
    assert_eq!(read(&env), 5);

    capture_start();
    feed(&["(set n 9)", "n", ":c"]);
    if interrupt_into_repl(&env, 0, Span { start: 0, end: 0 }).is_err() {
        panic!("the session must resume with :c, not stop the run");
    }
    REPL_INPUT.with(|queue| *queue.borrow_mut() = None);
    assert_eq!(
        read(&env),
        9,
        "the session set the binding it was opened with"
    );

    // The set is not echoed, and the read shows the value the session wrote.
    let out = capture_take();
    assert!(
        out.contains("repl> 9\n"),
        "the session read it back:\n{out}"
    );
}

#[test]
fn a_ctrl_c_before_a_session_starts_leaves_no_interrupt_behind() {
    // The flag is consumed before the session opens, so `:c` cannot come straight
    // back into another session. The run continues instead of looping on itself.
    feed(&[":c"]);
    INTERRUPT_PENDING.with(|flag| flag.set(true));
    let value = run("(add 1 1)").unwrap();
    INTERRUPT_PENDING.with(|flag| flag.set(false));
    REPL_INPUT.with(|queue| *queue.borrow_mut() = None);
    assert!(matches!(value, Value::Int(x) if x == 2));
    assert!(
        !INTERRUPT_PENDING.with(|flag| flag.get()),
        "the pending flag must be consumed"
    );
    assert!(
        !INTERRUPTED.load(Ordering::Relaxed),
        "and so must INTERRUPTED"
    );
}

#[test]
fn a_native_that_notices_a_ctrl_c_hands_it_to_the_evaluator() {
    // A native function is not given the environment, so it cannot open a
    // session itself. It records the interrupt and the evaluator picks it up at
    // its next check, rather than the run ending there and discarding whatever
    // the user typed after the cancel.
    assert!(!take_interrupt(), "nothing is pending to begin with");
    INTERRUPT_PENDING.set(true);
    assert!(take_interrupt(), "the evaluator sees a pending interrupt");
    assert!(!take_interrupt(), "and consumes it, so it fires once");
}

#[test]
fn a_source_span_resolves_against_the_source_that_contains_it() {
    // The context on top of the stack is not always the right one: a program
    // running inside a session line has the line on top, and an offset into the
    // program can be past the end of it. The stack is searched for a context
    // long enough to hold the span, so the prompt names the right file and line
    // even then.
    let program = "(let a 1)\n(let b 2)\n(repl)";
    push_source(SourceCtx {
        label: "prog.lisp".into(),
        source: program.to_owned(),
        line_offset: 0,
    });
    push_source(SourceCtx {
        label: "<repl>".into(),
        source: "(spin)".to_owned(),
        line_offset: 0,
    });
    // An offset into the program, past the end of the session line on top.
    let deep = source_for_span(Span { start: 20, end: 20 });
    // An offset inside the session line, so the top of the stack is right.
    let shallow = source_for_span(Span { start: 2, end: 2 });
    pop_source();
    pop_source();
    let deep_label = deep.as_ref().map(|ctx| ctx.label.clone());
    let deep_source = deep.map(|ctx| ctx.source);
    let shallow_label = shallow.map(|ctx| ctx.label);
    assert_eq!(deep_label.as_deref(), Some("prog.lisp"));
    assert_eq!(deep_source, Some(program.to_owned()));
    assert_eq!(shallow_label.as_deref(), Some("<repl>"));
}
