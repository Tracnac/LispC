use std::rc::Rc;

use super::super::{Error, NativeFunction, Value};

pub fn module() -> Value {
    Value::Struct(Rc::new(vec![
        (
            "upper".to_owned(),
            descriptor("str.upper", upper, "Convert a string to uppercase."),
        ),
        (
            "lower".to_owned(),
            descriptor("str.lower", lower, "Convert a string to lowercase."),
        ),
    ]))
}

fn descriptor(
    name: &'static str,
    call: fn(Vec<Value>) -> Result<Value, Error>,
    documentation: &'static str,
) -> Value {
    let types = Value::Array(Rc::new(vec![Value::Str("string".to_owned())]));
    let returns = Value::Array(Rc::new(vec![Value::Str("string".to_owned())]));
    let spec = Value::Struct(Rc::new(vec![
        (
            "documentation".to_owned(),
            Value::Str(documentation.to_owned()),
        ),
        ("arity".to_owned(), Value::Int(1)),
        ("type".to_owned(), types),
        ("return".to_owned(), returns),
    ]));
    let native = Value::NativeFunction(Rc::new(NativeFunction { name, call }));

    Value::Struct(Rc::new(vec![
        ("_".to_owned(), native),
        ("spec".to_owned(), spec),
    ]))
}

fn string_argument(args: Vec<Value>) -> Result<String, Error> {
    match args.into_iter().next() {
        Some(Value::Str(value)) => Ok(value),
        _ => Err(Error::Type("expected string".into())),
    }
}

fn upper(args: Vec<Value>) -> Result<Value, Error> {
    Ok(Value::Str(string_argument(args)?.to_uppercase()))
}

fn lower(args: Vec<Value>) -> Result<Value, Error> {
    Ok(Value::Str(string_argument(args)?.to_lowercase()))
}
