use super::run;
use crate::*;

#[test]
fn file_descriptor_io_reads_lines_and_tracks_close_status() {
    let path = env::temp_dir().join(format!("small_lisp_io_{}.txt", std::process::id()));
    let source = format!(
        r#"
                (use "io")
                (let fd (io.open "file:{}?mode=w"))
                (io.write fd "first\nsecond")
                (io.close fd)
                (set fd (io.open "file:{}?mode=r"))
                (let first (io.read fd))
                (io.close fd)
                first
            "#,
        path.display(),
        path.display(),
    );
    let value = run(&source).unwrap();
    assert!(matches!(value, Value::Str(line) if line == "first"));

    let close_status = run(&format!(
        r#"
                (use "io")
                (let fd (io.open "file:{}?mode=r"))
                (io.close fd)
                (io.close fd)
            "#,
        path.display(),
    ))
    .unwrap();
    assert!(matches!(close_status, Value::Bool(false)));
    fs::remove_file(path).unwrap();
}

#[test]
fn native_io_module_is_loaded_and_introspectable() {
    let value = run(r#"(use "io")
           (expect io.open.spec.arity 1)
           (expect io.open.spec.type ["string"])
           (expect io.open.spec.return ["int"])
           ($ "%s" io.open.spec.documentation)"#)
    .unwrap();
    assert!(matches!(
        value,
        Value::Str(text) if text == "Open a file URI using its mode query parameter."
    ));
}

#[test]
fn file_open_modes_have_their_declared_semantics() {
    let path = env::temp_dir().join(format!("small_lisp_io_modes_{}.txt", std::process::id()));
    fs::write(&path, "initial").unwrap();
    let path = path.display().to_string();

    let value = run(&format!(
        r#"(use "io") (let fd (io.open "file:{path}?mode=r")) (let line (io.read fd)) (io.close fd) line"#
    ))
    .unwrap();
    assert!(matches!(value, Value::Str(line) if line == "initial"));

    let value = run(&format!(
        r#"(use "io") (let fd (io.open "file:{path}?mode=w")) (io.close fd)"#
    ))
    .unwrap();
    assert!(matches!(value, Value::Bool(true)));
    assert_eq!(fs::read_to_string(&path).unwrap(), "");

    fs::write(&path, "initial").unwrap();
    for (mode, text, expected) in [
        ("r+", "R", "Rnitial"),
        ("w+", "W", "W"),
        ("a+", "A", "initialA"),
        ("a", " appended", "initial appended"),
    ] {
        fs::write(&path, "initial").unwrap();
        run(&format!(
            r#"(use "io") (let fd (io.open "file:{path}?mode={mode}")) (io.write fd "{text}") (io.close fd)"#
        ))
        .unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), expected, "mode {mode}");
    }

    fs::remove_file(path).unwrap();
}

#[test]
fn file_open_rejects_missing_invalid_and_malformed_uris() {
    for uri in [
        "file:/tmp/small-lisp-missing-mode",
        "file:/tmp/small-lisp-invalid-mode?mode=x",
        "file:/tmp/small-lisp-empty-mode?mode=",
        "http:/tmp/small-lisp-non-file?mode=r",
        "file:/tmp/small-lisp-multiple?foo=bar",
    ] {
        assert!(
            matches!(
                run(&format!(r#"(use "io") (io.open "{uri}")"#)),
                Err(Error::Io(_))
            ),
            "{uri}"
        );
    }
}

#[test]
fn standard_file_descriptors_are_available() {
    modules::io::FILES.with(|files| {
        let files = files.borrow();
        assert!(matches!(
            files.files.get(&0),
            Some(modules::io::FileHandle::Stdin)
        ));
        assert!(matches!(
            files.files.get(&1),
            Some(modules::io::FileHandle::Stdout)
        ));
        assert!(matches!(
            files.files.get(&2),
            Some(modules::io::FileHandle::Stderr)
        ));
    });
    assert!(matches!(
        run(r#"(use "io") (io.write 1 "")"#),
        Ok(Value::Int(0))
    ));
    assert!(matches!(
        run(r#"(use "io") (io.write 2 "")"#),
        Ok(Value::Int(0))
    ));
}

#[test]
fn io_read_is_buffered_across_repeated_calls_and_multiple_lines() {
    let path = env::temp_dir().join(format!("small_lisp_io_buffered_{}.txt", std::process::id()));
    // LF-terminated lines, an empty line, and a final line without a
    // terminator, read back through five repeated io.read calls.
    fs::write(&path, "alpha\nbeta\n\ngamma").unwrap();
    let source = format!(
        r#"
            (use "io")
            (let fd (io.open "file:{}?mode=r"))
            (let a (io.read fd))
            (let b (io.read fd))
            (let c (io.read fd))
            (let d (io.read fd))
            (let e (io.read fd))
            (io.close fd)
            [a b c d e]
        "#,
        path.display(),
    );
    let value = run(&source).unwrap();
    // The final read at EOF returns `_`.
    assert_eq!(
        debug_render(&value),
        r#"Array([Str("alpha"), Str("beta"), Str(""), Str("gamma"), Null])"#
    );
    fs::remove_file(path).unwrap();
}

#[test]
fn read_write_mode_r_plus_reads_then_writes_through_one_handle() {
    let path = env::temp_dir().join(format!("small_lisp_io_rplus_{}.txt", std::process::id()));
    fs::write(&path, "initial").unwrap();
    let source = format!(
        r#"(use "io")
            (let fd (io.open "file:{}?mode=r+"))
            (let line (io.read fd))
            (io.write fd "!")
            (io.close fd)
            line"#,
        path.display(),
    );
    let value = run(&source).unwrap();
    assert!(matches!(value, Value::Str(line) if line == "initial"));
    // The write lands at the read position (EOF), extending the file: reads
    // and writes share one stream position on the same underlying file.
    assert_eq!(fs::read_to_string(&path).unwrap(), "initial!");
    fs::remove_file(path).unwrap();
}

#[test]
fn read_write_mode_w_plus_truncates_and_reads_at_the_stream_position() {
    let path = env::temp_dir().join(format!("small_lisp_io_wplus_{}.txt", std::process::id()));
    fs::write(&path, "old").unwrap();
    let source = format!(
        r#"(use "io")
            (let fd (io.open "file:{}?mode=w+"))
            (let before (io.read fd))
            (io.write fd "one\ntwo\n")
            (let after (io.read fd))
            (io.close fd)
            [before after]"#,
        path.display(),
    );
    let value = run(&source).unwrap();
    // w+ truncated the file, and the single shared stream position is past
    // the written bytes when the second read happens, so both reads see EOF.
    assert_eq!(debug_render(&value), "Array([Null, Null])");
    assert_eq!(fs::read_to_string(&path).unwrap(), "one\ntwo\n");
    fs::remove_file(path).unwrap();
}

#[test]
fn read_write_mode_a_plus_reads_from_the_start_and_appends() {
    let path = env::temp_dir().join(format!("small_lisp_io_aplus_{}.txt", std::process::id()));
    fs::write(&path, "one\ntwo\n").unwrap();
    let source = format!(
        r#"(use "io")
            (let fd (io.open "file:{}?mode=a+"))
            (let a (io.read fd))
            (let b (io.read fd))
            (io.write fd "X")
            (io.close fd)
            [a b]"#,
        path.display(),
    );
    let value = run(&source).unwrap();
    assert_eq!(debug_render(&value), r#"Array([Str("one"), Str("two")])"#);
    // Appends always land at the end of the file, even after reads.
    assert_eq!(fs::read_to_string(&path).unwrap(), "one\ntwo\nX");
    fs::remove_file(path).unwrap();
}

#[test]
fn io_read_strips_crlf_and_lone_lf_terminators() {
    let path = env::temp_dir().join(format!("small_lisp_io_crlf_{}.txt", std::process::id()));
    fs::write(&path, "one\r\ntwo\nthree\r\n").unwrap();
    let source = format!(
        r#"(use "io")
            (let fd (io.open "file:{}?mode=r"))
            (let a (io.read fd))
            (let b (io.read fd))
            (let c (io.read fd))
            (let d (io.read fd))
            [a b c d]"#,
        path.display(),
    );
    let value = run(&source).unwrap();
    assert_eq!(
        debug_render(&value),
        r#"Array([Str("one"), Str("two"), Str("three"), Null])"#
    );
    fs::remove_file(path).unwrap();
}

#[test]
fn io_read_rejects_invalid_utf8() {
    let path = env::temp_dir().join(format!("small_lisp_io_utf8_{}.txt", std::process::id()));
    fs::write(&path, b"ok\n\xff\xfe\n").unwrap();
    let source = format!(
        r#"(use "io") (let fd (io.open "file:{}?mode=r")) (io.read fd) (io.read fd)"#,
        path.display(),
    );
    // The first line reads fine; the second line is not valid UTF-8.
    assert!(matches!(
        run(&source),
        Err(Error::Io(message)) if message == "input is not valid UTF-8"
    ));
    fs::remove_file(path).unwrap();
}

#[test]
fn io_read_and_write_reject_invalid_and_incapable_descriptors() {
    assert!(matches!(
        run(r#"(use "io") (io.read 42)"#),
        Err(Error::Io(message)) if message == "invalid file descriptor 42"
    ));
    assert!(matches!(
        run(r#"(use "io") (io.write 42 "x")"#),
        Err(Error::Io(message)) if message == "invalid file descriptor 42"
    ));
    // stdout and stderr are never readable.
    assert!(matches!(
        run(r#"(use "io") (io.read 1)"#),
        Err(Error::Io(message)) if message == "file descriptor 1 is not readable"
    ));
    assert!(matches!(
        run(r#"(use "io") (io.read 2)"#),
        Err(Error::Io(message)) if message == "file descriptor 2 is not readable"
    ));

    let path = env::temp_dir().join(format!("small_lisp_io_caps_{}.txt", std::process::id()));
    fs::write(&path, "data").unwrap();
    // A write-only handle cannot be read; a read-only handle cannot be written.
    assert!(matches!(
        run(&format!(
            r#"(use "io") (let fd (io.open "file:{}?mode=w")) (io.read fd)"#,
            path.display()
        )),
        Err(Error::Io(message)) if message.contains("is not readable")
    ));
    assert!(matches!(
        run(&format!(
            r#"(use "io") (let fd (io.open "file:{}?mode=r")) (io.write fd "x")"#,
            path.display()
        )),
        Err(Error::Io(message)) if message.contains("is not writable")
    ));
    fs::remove_file(path).unwrap();
}
