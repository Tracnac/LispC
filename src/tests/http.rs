use super::{http_fixture, run};
use crate::*;

#[test]
fn http_module_returns_text_and_json() {
    let (url, handle) = http_fixture(
        "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 5\r\n\r\nhello",
        None,
    );
    assert!(matches!(
        run(&format!("(use \"http\") (http.get \"{url}\")")),
        Ok(Value::Str(value)) if value == "hello"
    ));
    handle.join().unwrap();

    let (url, handle) = http_fixture(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json; charset=utf-8\r\nContent-Length: 13\r\n\r\n{\"answer\":42}",
            None,
        );
    let value = run(&format!("(use \"http\") (http.get \"{url}\")")).unwrap();
    assert_eq!(debug_render(&value), "Struct({answer: Int(42)})");
    handle.join().unwrap();
}

#[test]
fn http_post_serializes_strings_and_structs_as_json() {
    let (url, handle) = http_fixture(
        "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 2\r\n\r\nok",
        Some("\"hello\""),
    );
    assert!(matches!(
        run(&format!("(use \"http\") (http.post \"{url}\" \"hello\")")),
        Ok(Value::Str(value)) if value == "ok"
    ));
    handle.join().unwrap();

    let (url, handle) = http_fixture(
        "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 2\r\n\r\nok",
        Some("{\"x\":1}"),
    );
    assert!(matches!(
        run(&format!("(use \"http\") (http.post \"{url}\" {{x:1}})")),
        Ok(Value::Str(value)) if value == "ok"
    ));
    handle.join().unwrap();
}

#[test]
fn http_head_returns_empty_string() {
    let (url, handle) = http_fixture(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 13\r\n\r\nnot-json-body",
            None,
        );
    assert!(matches!(
        run(&format!("(use \"http\") (http.head \"{url}\")")),
        Ok(Value::Str(value)) if value.is_empty()
    ));
    handle.join().unwrap();
}

#[test]
fn http_reports_status_and_json_errors() {
    let (url, handle) = http_fixture("HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n", None);
    let error = match run(&format!("(use \"http\") (http.get \"{url}\")")) {
        Err(error) => error,
        Ok(_) => panic!("expected an HTTP error"),
    };
    assert!(matches!(&error, Error::Http(message) if message.contains("status 404")));
    assert!(error.to_string().starts_with("HTTPError"));
    handle.join().unwrap();

    let (url, handle) = http_fixture(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 8\r\n\r\nnot-json",
        None,
    );
    let error = match run(&format!("(use \"http\") (http.get \"{url}\")")) {
        Err(error) => error,
        Ok(_) => panic!("expected an HTTP error"),
    };
    assert!(matches!(&error, Error::Http(message) if message.contains("invalid JSON response")));
    assert!(error.to_string().starts_with("HTTPError"));
    handle.join().unwrap();
}

#[test]
fn http_decodes_json_suffix_media_types() {
    // RFC 6839 structured syntax suffix: application/problem+json is JSON.
    let (url, handle) = http_fixture(
        "HTTP/1.1 200 OK\r\nContent-Type: application/problem+json; charset=utf-8\r\nContent-Length: 18\r\n\r\n{\"status\":\"error\"}",
        None,
    );
    let value = run(&format!("(use \"http\") (http.get \"{url}\")")).unwrap();
    assert_eq!(debug_render(&value), "Struct({status: Str(\"error\")})");
    handle.join().unwrap();

    let (url, handle) = http_fixture(
        "HTTP/1.1 200 OK\r\nContent-Type: Application/HAL+JSON\r\nContent-Length: 18\r\n\r\n{\"status\":\"error\"}",
        None,
    );
    let value = run(&format!("(use \"http\") (http.get \"{url}\")")).unwrap();
    assert_eq!(debug_render(&value), "Struct({status: Str(\"error\")})");
    handle.join().unwrap();

    // A non-JSON media type still comes back as a raw string.
    let (url, handle) = http_fixture(
        "HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nContent-Length: 5\r\n\r\nhello",
        None,
    );
    assert!(matches!(
        run(&format!("(use \"http\") (http.get \"{url}\")")),
        Ok(Value::Str(value)) if value == "hello"
    ));
    handle.join().unwrap();
}

#[test]
fn http_descriptors_are_introspectable_and_enforce_arity_and_types() {
    let value = run(r#"(use "http")
        (expect http.get.spec.arity 1)
        (expect http.get.spec.type ["string"])
        (expect http.post.spec.arity 2)
        (expect http.post.spec.type ["string" "any"])
        (expect http.post.spec.return _)
        (expect http.head.spec.documentation "Perform a synchronous HTTP HEAD request and return an empty string.")
        t"#)
    .unwrap();
    assert!(matches!(value, Value::Bool(true)));

    assert!(matches!(
        run(r#"(use "http") (http.get)"#),
        Err(Error::Arity(message)) if message.contains("expects 1 arguments")
    ));
    assert!(matches!(
        run(r#"(use "http") (http.get 10)"#),
        Err(Error::Type(message)) if message.contains("argument 1 expects string")
    ));
    assert!(matches!(
        run(r#"(use "http") (http.post "url")"#),
        Err(Error::Arity(message)) if message.contains("expects 2 arguments")
    ));
}
