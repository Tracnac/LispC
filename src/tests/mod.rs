use super::*;
use std::{
    io::{Read, Write},
    net::TcpListener,
    thread,
};

fn run(src: &str) -> Result<Value, Error> {
    LAST_TRACE.with(|trace| trace.borrow_mut().clear());
    let tokens = lex(src)?;
    let program = Parser { ts: tokens, i: 0 }.program()?;
    let env = new_env(None);
    let mut result = Value::Null;
    for form in program {
        result = eval(&form, &env, 0, 0).map_err(flow_err)?;
    }
    Ok(result)
}

fn check(src: &str, expected: &str) {
    let actual = run(src).expect("left-hand side of check failed to run");
    let expected = run(expected).expect("right-hand side of check failed to run");
    assert!(equals(&actual, &expected));
}

fn http_fixture(
    response: &'static str,
    expected_request: Option<&'static str>,
) -> (String, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = Vec::new();
        let mut buffer = [0; 1024];
        let header_end = loop {
            let count = stream.read(&mut buffer).unwrap();
            assert!(count > 0);
            request.extend_from_slice(&buffer[..count]);
            if let Some(end) = request.windows(4).position(|window| window == b"\r\n\r\n") {
                break end + 4;
            }
        };
        let headers = String::from_utf8_lossy(&request[..header_end]);
        let content_length = headers
            .lines()
            .find_map(|line| line.strip_prefix("Content-Length: "))
            .and_then(|value| value.trim().parse::<usize>().ok())
            .unwrap_or(0);
        while request.len() < header_end + content_length {
            let count = stream.read(&mut buffer).unwrap();
            assert!(count > 0);
            request.extend_from_slice(&buffer[..count]);
        }
        if let Some(expected) = expected_request {
            let request = String::from_utf8_lossy(&request);
            assert!(request[header_end..].contains(expected));
        }
        stream.write_all(response.as_bytes()).unwrap();
    });
    (url, handle)
}

mod arithmetic;
mod collections;
mod comparisons;
mod control_flow;
mod core;
mod errors;
mod http;
mod io;
mod modules;
mod repl;
mod strings;
mod types;
