use std::{
    cell::RefCell,
    collections::HashMap,
    fs::{File, OpenOptions},
    io::{self, BufRead, BufReader, Write},
    rc::Rc,
};

use super::super::{check_interrupted, Error, NativeFunction, Value};

pub(crate) struct FileTable {
    pub(crate) next_fd: i64,
    pub(crate) files: HashMap<i64, FileHandle>,
}

/// A file opened for reading. The buffered reader is owned by the handle for
/// its whole lifetime, so data prefetched by one read stays available to the
/// next read on the same descriptor (and to future read-byte/read-line-style
/// operations).
pub(crate) struct ReadableStream {
    reader: BufReader<File>,
    writable: bool,
}

pub(crate) enum FileHandle {
    /// File opened for reading (`r`, `r+`, `w+`, `a+`); `writable` is true for
    /// the read/write modes so `io.write` reaches the same underlying file
    /// below the reader's buffer instead of through a second wrapper.
    Read(ReadableStream),
    /// File opened for writing only (`w`, `a`).
    Write(File),
    Stdin,
    Stdout,
    Stderr,
}

thread_local! {
    pub(crate) static FILES: RefCell<FileTable> = RefCell::new(FileTable {
        next_fd: 3,
        files: HashMap::from([
            (0, FileHandle::Stdin),
            (1, FileHandle::Stdout),
            (2, FileHandle::Stderr),
        ]),
    });
}

pub fn module() -> Value {
    Value::Struct(Rc::new(RefCell::new(vec![
        (
            "open".to_owned(),
            Rc::new(RefCell::new(descriptor(
                "io.open",
                open,
                "Open a file URI using its mode query parameter.",
                1,
                &["string"],
                &["int"],
            ))),
        ),
        (
            "read".to_owned(),
            Rc::new(RefCell::new(descriptor(
                "io.read",
                read,
                "Read one line from a file descriptor.",
                1,
                &["int"],
                &["string"],
            ))),
        ),
        (
            "write".to_owned(),
            Rc::new(RefCell::new(descriptor(
                "io.write",
                write,
                "Write a string to a file descriptor.",
                2,
                &["int", "string"],
                &["int"],
            ))),
        ),
        (
            "close".to_owned(),
            Rc::new(RefCell::new(descriptor(
                "io.close",
                close,
                "Close a file descriptor.",
                1,
                &["int"],
                &["bool"],
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
    returns: &[&str],
) -> Value {
    let values = |items: &[&str]| {
        Value::Array(Rc::new(RefCell::new(
            items
                .iter()
                .map(|item| Rc::new(RefCell::new(Value::Str((*item).to_owned()))))
                .collect(),
        )))
    };
    let spec = Value::Struct(Rc::new(RefCell::new(vec![
        (
            "documentation".to_owned(),
            Rc::new(RefCell::new(Value::Str(documentation.to_owned()))),
        ),
        ("arity".to_owned(), Rc::new(RefCell::new(Value::Int(arity)))),
        ("type".to_owned(), Rc::new(RefCell::new(values(types)))),
        ("return".to_owned(), Rc::new(RefCell::new(values(returns)))),
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

fn open(args: Vec<Value>) -> Result<Value, Error> {
    let [Value::Str(uri)] = args.as_slice() else {
        return Err(Error::Type("io.open expects a string URI".into()));
    };
    let Some(path_and_query) = uri.strip_prefix("file:") else {
        return Err(Error::Io("io.open expects a file URI".into()));
    };
    let Some((path, query)) = path_and_query.split_once('?') else {
        return Err(Error::Io("file URI is missing mode".into()));
    };
    if path.is_empty() || query.is_empty() {
        return Err(Error::Io("malformed file URI".into()));
    }
    let mode = query
        .split('&')
        .find_map(|parameter| parameter.strip_prefix("mode="))
        .ok_or_else(|| Error::Io("file URI is missing mode".into()))?;
    let mut options = OpenOptions::new();
    let readable = match mode {
        "r" => {
            options.read(true);
            true
        }
        "r+" => {
            options.read(true).write(true);
            true
        }
        "w" => {
            options.write(true).create(true).truncate(true);
            false
        }
        "w+" => {
            options.read(true).write(true).create(true).truncate(true);
            true
        }
        "a" => {
            options.append(true).create(true);
            false
        }
        "a+" => {
            options.read(true).append(true).create(true);
            true
        }
        _ => return Err(Error::Io(format!("unsupported open mode `{mode}`"))),
    };
    let file = options.open(path).map_err(|e| Error::Io(e.to_string()))?;
    let fd = FILES.with(|files| {
        let mut files = files.borrow_mut();
        let fd = files.next_fd;
        files.next_fd += 1;
        let handle = if readable {
            FileHandle::Read(ReadableStream {
                reader: BufReader::new(file),
                // Only `r` lacks write access among the readable modes.
                writable: mode != "r",
            })
        } else {
            FileHandle::Write(file)
        };
        files.files.insert(fd, handle);
        fd
    });
    Ok(Value::Int(fd))
}

fn close(args: Vec<Value>) -> Result<Value, Error> {
    let [Value::Int(fd)] = args.as_slice() else {
        return Err(Error::Type("io.close expects an integer descriptor".into()));
    };
    Ok(Value::Bool(FILES.with(|files| {
        files.borrow_mut().files.remove(fd).is_some()
    })))
}

fn read(args: Vec<Value>) -> Result<Value, Error> {
    let [Value::Int(fd)] = args.as_slice() else {
        return Err(Error::Type("io.read expects an integer descriptor".into()));
    };
    let line = FILES.with(|files| {
        let mut files = files.borrow_mut();
        let handle = files
            .files
            .get_mut(fd)
            .ok_or_else(|| Error::Io(format!("invalid file descriptor {fd}")))?;
        match handle {
            FileHandle::Read(stream) => read_line(&mut stream.reader),
            FileHandle::Write(_) => Err(Error::Io(format!("file descriptor {fd} is not readable"))),
            FileHandle::Stdin => read_line(&mut io::stdin().lock()),
            FileHandle::Stdout => Err(Error::Io("file descriptor 1 is not readable".into())),
            FileHandle::Stderr => Err(Error::Io("file descriptor 2 is not readable".into())),
        }
    })?;
    Ok(line.map_or(Value::Null, Value::Str))
}

/// Read one line through the handle's persistent buffered reader: `None` at
/// EOF, otherwise the line without its terminator (LF, or CRLF as a unit).
/// `read_line` keeps the underlying buffer filled, so the next read on the same
/// descriptor continues without losing prefetched data.
fn read_line<R: BufRead>(reader: &mut R) -> Result<Option<String>, Error> {
    check_interrupted()?;
    let mut line = String::new();
    let bytes = reader
        .read_line(&mut line)
        .map_err(|error| match error.kind() {
            io::ErrorKind::InvalidData => Error::Io("input is not valid UTF-8".into()),
            _ => Error::Io(error.to_string()),
        })?;
    check_interrupted()?;
    if bytes == 0 {
        return Ok(None);
    }
    if line.ends_with('\n') {
        line.pop();
    }
    if line.ends_with('\r') {
        line.pop();
    }
    Ok(Some(line))
}

fn write(args: Vec<Value>) -> Result<Value, Error> {
    let [Value::Int(fd), Value::Str(text)] = args.as_slice() else {
        return Err(Error::Type(
            "io.write expects an integer descriptor and string".into(),
        ));
    };
    FILES.with(|files| {
        let mut files = files.borrow_mut();
        let handle = files
            .files
            .get_mut(fd)
            .ok_or_else(|| Error::Io(format!("invalid file descriptor {fd}")))?;
        match handle {
            FileHandle::Read(stream) if stream.writable => {
                stream.reader.get_mut().write_all(text.as_bytes())
            }
            FileHandle::Read(_) => {
                return Err(Error::Io(format!("file descriptor {fd} is not writable")));
            }
            FileHandle::Write(file) => file.write_all(text.as_bytes()),
            FileHandle::Stdin => return Err(Error::Io("file descriptor 0 is not writable".into())),
            FileHandle::Stdout => io::stdout()
                .write_all(text.as_bytes())
                .and_then(|_| io::stdout().flush()),
            FileHandle::Stderr => io::stderr()
                .write_all(text.as_bytes())
                .and_then(|_| io::stderr().flush()),
        }
        .map_err(|e| Error::Io(e.to_string()))
    })?;
    Ok(Value::Int(text.len() as i64))
}
