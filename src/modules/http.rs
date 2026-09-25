use std::{cell::RefCell, rc::Rc};

use super::super::{json_render, Error, NativeFunction, Value};

pub fn module() -> Value {
    Value::Struct(Rc::new(RefCell::new(vec![
        (
            "get".to_owned(),
            Rc::new(RefCell::new(descriptor(
                "http.get",
                get,
                "Perform a synchronous HTTP GET request and return the response.",
                1,
                &["string"],
            ))),
        ),
        (
            "head".to_owned(),
            Rc::new(RefCell::new(descriptor(
                "http.head",
                head,
                "Perform a synchronous HTTP HEAD request and return an empty string.",
                1,
                &["string"],
            ))),
        ),
        (
            "delete".to_owned(),
            Rc::new(RefCell::new(descriptor(
                "http.delete",
                delete,
                "Perform a synchronous HTTP DELETE request and return the response.",
                1,
                &["string"],
            ))),
        ),
        (
            "post".to_owned(),
            Rc::new(RefCell::new(descriptor(
                "http.post",
                post,
                "Perform a synchronous HTTP POST request with a JSON body.",
                2,
                &["string", "any"],
            ))),
        ),
        (
            "put".to_owned(),
            Rc::new(RefCell::new(descriptor(
                "http.put",
                put,
                "Perform a synchronous HTTP PUT request with a JSON body.",
                2,
                &["string", "any"],
            ))),
        ),
        (
            "patch".to_owned(),
            Rc::new(RefCell::new(descriptor(
                "http.patch",
                patch,
                "Perform a synchronous HTTP PATCH request with a JSON body.",
                2,
                &["string", "any"],
            ))),
        ),
    ])))
}

fn descriptor(
    name: &'static str,
    call: fn(Vec<Value>) -> Result<Value, Error>,
    documentation: &'static str,
    arity: i64,
    types: &[&str],
) -> Value {
    let values = |items: &[&str]| {
        Value::Array(Rc::new(RefCell::new(
            items
                .iter()
                .map(|item| Rc::new(RefCell::new(Value::Str((*item).to_owned()))))
                .collect(),
        )))
    };
    // The response type is dynamic (string, struct, array, or null depending on
    // Content-Type), so the spec declares no single return type (`_`).
    let spec = Value::Struct(Rc::new(RefCell::new(vec![
        (
            "documentation".to_owned(),
            Rc::new(RefCell::new(Value::Str(documentation.to_owned()))),
        ),
        ("arity".to_owned(), Rc::new(RefCell::new(Value::Int(arity)))),
        ("type".to_owned(), Rc::new(RefCell::new(values(types)))),
        ("return".to_owned(), Rc::new(RefCell::new(Value::Null))),
    ])));
    Value::Struct(Rc::new(RefCell::new(vec![
        (
            "_".to_owned(),
            Rc::new(RefCell::new(Value::NativeFunction(Rc::new(
                NativeFunction { name, call },
            )))),
        ),
        ("spec".to_owned(), Rc::new(RefCell::new(spec))),
    ])))
}

fn get(args: Vec<Value>) -> Result<Value, Error> {
    let [Value::Str(url)] = args.as_slice() else {
        return Err(Error::Type("http.get expects a string url".into()));
    };
    request("GET", url, None)
}

fn head(args: Vec<Value>) -> Result<Value, Error> {
    let [Value::Str(url)] = args.as_slice() else {
        return Err(Error::Type("http.head expects a string url".into()));
    };
    request("HEAD", url, None)
}

fn delete(args: Vec<Value>) -> Result<Value, Error> {
    let [Value::Str(url)] = args.as_slice() else {
        return Err(Error::Type("http.delete expects a string url".into()));
    };
    request("DELETE", url, None)
}

fn post(args: Vec<Value>) -> Result<Value, Error> {
    let [Value::Str(url), body] = args.as_slice() else {
        return Err(Error::Type(
            "http.post expects a string url and a body".into(),
        ));
    };
    request("POST", url, Some(body.clone()))
}

fn put(args: Vec<Value>) -> Result<Value, Error> {
    let [Value::Str(url), body] = args.as_slice() else {
        return Err(Error::Type(
            "http.put expects a string url and a body".into(),
        ));
    };
    request("PUT", url, Some(body.clone()))
}

fn patch(args: Vec<Value>) -> Result<Value, Error> {
    let [Value::Str(url), body] = args.as_slice() else {
        return Err(Error::Type(
            "http.patch expects a string url and a body".into(),
        ));
    };
    request("PATCH", url, Some(body.clone()))
}

fn request(method: &str, url: &str, body: Option<Value>) -> Result<Value, Error> {
    let body = body.map(|value| json_render(&value)).transpose()?;
    let request = ureq::request(method, url).set("Accept", "application/json");
    let response = match body {
        Some(body) => request
            .set("Content-Type", "application/json")
            .send_string(&body),
        None => request.call(),
    }
    .map_err(|error| match error {
        ureq::Error::Status(code, _) => {
            Error::Http(format!("HTTP request failed with status {code}"))
        }
        ureq::Error::Transport(error) => Error::Http(format!("HTTP request failed: {error}")),
    })?;

    if method == "HEAD" {
        return Ok(Value::Str(String::new()));
    }
    let content_type = response
        .header("Content-Type")
        .unwrap_or_default()
        .to_owned();
    let text = response
        .into_string()
        .map_err(|error| Error::Http(format!("failed to read HTTP response: {error}")))?;
    if content_type
        .split(';')
        .next()
        .is_some_and(is_json_media_type)
    {
        let json: serde_json::Value = serde_json::from_str(&text)
            .map_err(|error| Error::Http(format!("invalid JSON response: {error}")))?;
        json_to_value(json)
    } else {
        Ok(Value::Str(text))
    }
}

/// True when the (parameter-free) media type is JSON: exactly `application/json`
/// or any type whose subtype ends in `+json` (RFC 6839 structured syntax suffix,
/// e.g. `application/problem+json`, `application/hal+json`, `application/vnd.api+json`).
fn is_json_media_type(media_type: &str) -> bool {
    let media_type = media_type.trim();
    if media_type.eq_ignore_ascii_case("application/json") {
        return true;
    }
    let Some((_level, subtype)) = media_type.split_once('/') else {
        return false;
    };
    subtype.trim().to_ascii_lowercase().ends_with("+json")
}

fn json_to_value(value: serde_json::Value) -> Result<Value, Error> {
    match value {
        serde_json::Value::Null => Ok(Value::Null),
        serde_json::Value::Bool(value) => Ok(Value::Bool(value)),
        serde_json::Value::Number(value) => {
            if let Some(value) = value.as_i64() {
                Ok(Value::Int(value))
            } else if let Some(value) = value.as_f64() {
                Ok(Value::Float(value))
            } else {
                Err(Error::Http("JSON number cannot be represented".into()))
            }
        }
        serde_json::Value::String(value) => Ok(Value::Str(value)),
        serde_json::Value::Array(values) => Ok(Value::Array(Rc::new(RefCell::new(
            values
                .into_iter()
                .map(|value| json_to_value(value).map(|value| Rc::new(RefCell::new(value))))
                .collect::<Result<Vec<_>, _>>()?,
        )))),
        serde_json::Value::Object(fields) => Ok(Value::Struct(Rc::new(RefCell::new(
            fields
                .into_iter()
                .map(|(key, value)| Ok((key, Rc::new(RefCell::new(json_to_value(value)?)))))
                .collect::<Result<Vec<_>, Error>>()?,
        )))),
    }
}
