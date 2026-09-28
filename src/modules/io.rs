use std::{
    cell::RefCell,
    collections::HashMap,
    fs::{File, OpenOptions},
    io::{self, BufRead, BufReader, Seek, SeekFrom, Write},
    rc::Rc,
};

use super::super::{check_interrupted, Error, NativeFunction, Value};

pub(crate) struct FileTable {
    pub(crate) next_fd: i64,
    pub(crate) files: HashMap<i64, FileHandle>,
}

/// Where `io.write` puts its bytes on a handle that is also readable.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum WriteTarget {
    /// The mode is write-only or read-only, so `io.write` refuses the handle.
    Forbidden,
    /// `r+` and `w+`: the write goes at the current file position, and the
    /// position moves past it.
    Current,
    /// `a+`: the write goes at the end of the file.
    End,
}

/// A file opened for reading. The buffered reader belongs to the handle, so
/// data prefetched by one read stays available to the next read on the same
/// descriptor (and to future read-byte/read-line-style operations).
///
/// A handle that is also writable cannot keep that buffer across a write.
/// Prefetch has already moved the operating system's cursor past the logical
/// read position, so the position is tracked here and the file repositioned
/// before every write; and a write overwrites the bytes the reader prefetched
/// but has not returned yet, so the buffer is dropped and rebuilt around it.
pub(crate) struct ReadableStream {
    /// `None` only inside `write`, for as long as the buffer is taken out to
    /// be discarded.
    reader: Option<BufReader<File>>,
    writes: WriteTarget,
    /// Offset of the first byte the next read returns. The buffer's prefetch
    /// has already moved the file cursor past it, so the file cannot be asked
    /// where the reads got to and the count is kept here instead. A write to a
    /// `Current` target moves it; a write to an `End` target does not.
    position: u64,
}

pub(crate) enum FileHandle {
    /// File opened for reading (`r`, `r+`, `w+`, `a+`). Whether a write is
    /// allowed, and where it lands, is `ReadableStream::writes`.
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
    Value::Struct(Rc::new(vec![
        (
            "open".to_owned(),
            descriptor(
                "io.open",
                open,
                "Open a file URI using its mode query parameter.",
                1,
                &["string"],
                &["int"],
            ),
        ),
        (
            "read".to_owned(),
            descriptor(
                "io.read",
                read,
                "Read one line from a file descriptor.",
                1,
                &["int"],
                &["string"],
            ),
        ),
        (
            "write".to_owned(),
            descriptor(
                "io.write",
                write,
                "Write a string to a file descriptor.",
                2,
                &["int", "string"],
                &["int"],
            ),
        ),
        (
            "close".to_owned(),
            descriptor(
                "io.close",
                close,
                "Close a file descriptor.",
                1,
                &["int"],
                &["bool"],
            ),
        ),
    ]))
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
        Value::Array(Rc::new(
            items
                .iter()
                .map(|item| Value::Str((*item).to_owned()))
                .collect(),
        ))
    };
    let spec = Value::Struct(Rc::new(vec![
        (
            "documentation".to_owned(),
            Value::Str(documentation.to_owned()),
        ),
        ("arity".to_owned(), Value::Int(arity)),
        ("type".to_owned(), values(types)),
        ("return".to_owned(), values(returns)),
    ]));
    Value::Struct(Rc::new(vec![
        (
            "_".to_owned(),
            Value::NativeFunction(Rc::new(NativeFunction { name, call })),
        ),
        ("spec".to_owned(), spec),
    ]))
}

impl ReadableStream {
    fn new(file: File, writes: WriteTarget) -> Self {
        ReadableStream {
            reader: Some(BufReader::new(file)),
            writes,
            position: 0,
        }
    }

    /// Read one line and move the shared position past the bytes it consumed.
    fn read_line(&mut self) -> Result<Option<String>, Error> {
        let reader = self
            .reader
            .as_mut()
            .expect("the read buffer is only taken out for the length of a write");
        let (line, consumed) = read_line(reader)?;
        self.position += consumed;
        Ok(line)
    }

    /// Write `text` at this handle's target.
    fn write(&mut self, text: &str) -> io::Result<()> {
        let reader = self
            .reader
            .take()
            .expect("the read buffer is only taken out for the length of a write");
        let mut file = reader.into_inner();
        let outcome = self.write_through(&mut file, text);
        // A write disturbs the bytes the reader prefetched but has not
        // returned, so they are no longer what a read should see and the buffer
        // is rebuilt from the file. It is rebuilt either way, and rebuilding
        // after a failed write is no worse than the buffer the write disturbed.
        self.reader = Some(BufReader::new(file));
        outcome
    }

    /// Write `text` where this handle's mode says a write belongs, and leave
    /// the read position consistent with what the write did.
    fn write_through(&mut self, file: &mut File, text: &str) -> io::Result<()> {
        match self.writes {
            // `r+` and `w+` share one position with the reads. The seek is the
            // whole point: the buffer's prefetch has already left the file
            // cursor at the end of the prefetched block, which is where the
            // write would otherwise land.
            WriteTarget::Current => {
                file.seek(SeekFrom::Start(self.position))?;
                file.write_all(text.as_bytes())?;
                self.position += text.len() as u64;
            }
            // `a+` is opened in append mode, so the operating system puts the
            // bytes at the end however the file is positioned, and the read
            // position is left where the reads left it. The seek afterwards is
            // what makes that work: the append has just moved the file cursor
            // to the new end, and the rebuilt read buffer refills from the
            // cursor, so without it the reads would resume at the end of the
            // file and silently skip everything the buffer had not reached.
            WriteTarget::End => {
                file.write_all(text.as_bytes())?;
                file.seek(SeekFrom::Start(self.position))?;
            }
            WriteTarget::Forbidden => {
                return Err(io::Error::other("handle is not writable"));
            }
        }
        Ok(())
    }
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
    let (readable, writes) = match mode {
        "r" => {
            options.read(true);
            (true, WriteTarget::Forbidden)
        }
        "r+" => {
            options.read(true).write(true);
            (true, WriteTarget::Current)
        }
        "w" => {
            options.write(true).create(true).truncate(true);
            (false, WriteTarget::Forbidden)
        }
        "w+" => {
            options.read(true).write(true).create(true).truncate(true);
            (true, WriteTarget::Current)
        }
        "a" => {
            options.append(true).create(true);
            (false, WriteTarget::Forbidden)
        }
        "a+" => {
            options.read(true).append(true).create(true);
            (true, WriteTarget::End)
        }
        _ => return Err(Error::Io(format!("unsupported open mode `{mode}`"))),
    };
    let file = options.open(path).map_err(|e| Error::Io(e.to_string()))?;
    let fd = FILES.with(|files| {
        let mut files = files.borrow_mut();
        let fd = files.next_fd;
        files.next_fd += 1;
        let handle = if readable {
            FileHandle::Read(ReadableStream::new(file, writes))
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
            FileHandle::Read(stream) => stream.read_line(),
            FileHandle::Write(_) => Err(Error::Io(format!("file descriptor {fd} is not readable"))),
            FileHandle::Stdin => read_line(&mut io::stdin().lock()).map(|(line, _)| line),
            FileHandle::Stdout => Err(Error::Io("file descriptor 1 is not readable".into())),
            FileHandle::Stderr => Err(Error::Io("file descriptor 2 is not readable".into())),
        }
    })?;
    Ok(line.map_or(Value::Null, Value::Str))
}

/// Read one line through the handle's persistent buffered reader: `None` at
/// EOF, otherwise the line without its terminator (LF, or CRLF as a unit).
/// The second value is the number of bytes the read consumed, terminator
/// included, so a caller tracking a stream position can account for the
/// terminator the returned string has had removed. Reading the bytes first and
/// decoding them second keeps that count known even when the line turns out
/// not to be valid UTF-8, and `read_until` keeps the underlying buffer filled,
/// so the next read on the same descriptor continues without losing
/// prefetched data.
fn read_line<R: BufRead>(reader: &mut R) -> Result<(Option<String>, u64), Error> {
    check_interrupted();
    let mut raw = Vec::new();
    let consumed = reader
        .read_until(b'\n', &mut raw)
        .map_err(|error| Error::Io(error.to_string()))?;
    check_interrupted();
    if consumed == 0 {
        return Ok((None, 0));
    }
    let mut line =
        String::from_utf8(raw).map_err(|_| Error::Io("input is not valid UTF-8".into()))?;
    if line.ends_with('\n') {
        line.pop();
    }
    if line.ends_with('\r') {
        line.pop();
    }
    Ok((Some(line), consumed as u64))
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
            FileHandle::Read(stream) if stream.writes != WriteTarget::Forbidden => {
                stream.write(text)
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
