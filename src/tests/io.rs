use super::{http_fixture, run};
use crate::*;

#[test]
fn http_builtin_returns_text_and_json() {
    let (url, handle) = http_fixture(
        "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 5\r\n\r\nhello",
        None,
    );
    assert!(
        matches!(run(&format!("(@ \"{url}\" \"GET\")")), Ok(Value::Str(value)) if value == "hello")
    );
    handle.join().unwrap();

    let (url, handle) = http_fixture(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json; charset=utf-8\r\nContent-Length: 13\r\n\r\n{\"answer\":42}",
            None,
        );
    let value = run(&format!("(@ \"{url}\" \"GET\")")).unwrap();
    assert_eq!(debug_render(&value), "Struct({answer: Int(42)})");
    handle.join().unwrap();
}

#[test]
fn http_builtin_serializes_lisp_strings_as_json() {
    let (url, handle) = http_fixture(
        "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 2\r\n\r\nok",
        Some("\"hello\""),
    );
    assert!(matches!(
        run(&format!("(@ \"{url}\" \"POST\" \"hello\")")),
        Ok(Value::Str(value)) if value == "ok"
    ));
    handle.join().unwrap();

    let (url, handle) = http_fixture(
        "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 2\r\n\r\nok",
        Some("{\"x\":1}"),
    );
    assert!(matches!(
        run(&format!("(@ \"{url}\" \"POST\" {{x:1}})")),
        Ok(Value::Str(value)) if value == "ok"
    ));
    handle.join().unwrap();
}

#[test]
fn http_builtin_handles_head_without_decoding() {
    let (url, handle) = http_fixture(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 13\r\n\r\nnot-json-body",
            None,
        );
    assert!(matches!(
        run(&format!("(@ \"{url}\" \"HEAD\")")),
        Ok(Value::Str(value)) if value.is_empty()
    ));
    handle.join().unwrap();
}

#[test]
fn http_builtin_reports_method_status_and_json_errors() {
    assert!(matches!(
        run("(@ \"http://127.0.0.1:1\" \"OPTIONS\")"),
        Err(Error::Type(message)) if message.contains("unsupported HTTP method")
    ));

    let (url, handle) = http_fixture("HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n", None);
    assert!(matches!(
        run(&format!("(@ \"{url}\" \"GET\")")),
        Err(Error::Io(message)) if message.contains("status 404")
    ));
    handle.join().unwrap();

    let (url, handle) = http_fixture(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 8\r\n\r\nnot-json",
        None,
    );
    assert!(matches!(
        run(&format!("(@ \"{url}\" \"GET\")")),
        Err(Error::Io(message)) if message.contains("invalid JSON response")
    ));
    handle.join().unwrap();
}

#[test]
fn file_descriptor_io_reads_lines_and_tracks_close_status() {
    let path = env::temp_dir().join(format!("small_lisp_io_{}.txt", std::process::id()));
    let source = format!(
        r#"
                (use "io")
                (let #fd (io.open "file:{}?mode=w"))
                (io.write #fd "first\nsecond")
                (io.close #fd)
                (set #fd (io.open "file:{}?mode=r"))
                (let first (io.read #fd))
                (io.close #fd)
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
                (let #fd (io.open "file:{}?mode=r"))
                (io.close #fd)
                (io.close #fd)
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
        r#"(use "io") (let #fd (io.open "file:{path}?mode=r")) (let line (io.read #fd)) (io.close #fd) line"#
    ))
    .unwrap();
    assert!(matches!(value, Value::Str(line) if line == "initial"));

    let value = run(&format!(
        r#"(use "io") (let #fd (io.open "file:{path}?mode=w")) (io.close #fd)"#
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
            r#"(use "io") (let #fd (io.open "file:{path}?mode={mode}")) (io.write #fd "{text}") (io.close #fd)"#
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
