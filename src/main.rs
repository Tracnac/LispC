use regex::Regex;
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    env, fmt,
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    rc::Rc,
    sync::atomic::{AtomicBool, Ordering},
};
use unicode_segmentation::UnicodeSegmentation;

mod modules;

type Cell = Rc<RefCell<Value>>;
type EnvRef = Rc<RefCell<Env>>;

#[derive(Clone)]
enum Value {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    Array(Rc<RefCell<Vec<Cell>>>),
    Struct(Rc<RefCell<Vec<(String, Cell)>>>),
    Function(Rc<Function>),
    NativeFunction(Rc<NativeFunction>),
    Ref(Cell),
}
#[derive(Clone, Debug)]
enum IndexSpec {
    Selector(Box<Expr>),
    Range(Option<Box<Expr>>, Option<Box<Expr>>),
}
#[derive(Clone)]
struct Function {
    params: Vec<String>,
    body: Expr,
    env: EnvRef,
    name: Option<String>,
}
struct NativeFunction {
    name: &'static str,
    call: fn(Vec<Value>) -> Result<Value, Error>,
}
struct Env {
    values: Vec<(String, Cell)>,
    parent: Option<EnvRef>,
}

struct FileTable {
    next_fd: i64,
    files: HashMap<i64, FileHandle>,
}
enum FileHandle {
    File(File),
    Stdin,
    Stdout,
    Stderr,
}

thread_local! {
    static FILES: RefCell<FileTable> = RefCell::new(FileTable {
        next_fd: 3,
        files: HashMap::from([
            (0, FileHandle::Stdin),
            (1, FileHandle::Stdout),
            (2, FileHandle::Stderr),
        ]),
    });
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Span {
    start: usize,
    end: usize,
}
#[derive(Clone, Debug)]
struct Expr {
    kind: ExprKind,
    span: Span,
}
#[derive(Clone, Debug)]
enum ExprKind {
    Lit(Literal),
    Symbol(String),
    Field(Box<Expr>, String),
    Index(Box<Expr>, IndexSpec),
    Ref(Box<Expr>),
    Array(Vec<Expr>),
    Struct(Vec<StructField>),
    Call(Box<Expr>, Vec<Expr>),
    Block(Vec<Expr>),
}
#[derive(Clone, Debug)]
struct StructField {
    key: String,
    value: Expr,
    span: Span,
}
#[derive(Clone, Debug)]
enum Literal {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
}

#[derive(Clone, Debug)]
enum Error {
    Parse(String),
    Name(String),
    Type(String),
    Arity(String),
    Math(String),
    Io(String),
    Format(String),
    Regex(String),
    DuplicateKey(String),
    Expect(String),
    Match,
    ContinueOutsideLoop,
    BreakOutside,
    Interrupted,
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use Error::*;
        match self {
            Parse(s) => write!(f, "ParseError: {s}"),
            Name(s) => write!(f, "NameError: {s}"),
            Type(s) => write!(f, "TypeError: {s}"),
            Arity(s) => write!(f, "ArityError: {s}"),
            Math(s) => write!(f, "MathError: {s}"),
            Io(s) => write!(f, "IOError: {s}"),
            Format(s) => write!(f, "FormatError: {s}"),
            Regex(s) => write!(f, "InvalidRegex: {s}"),
            DuplicateKey(s) => write!(f, "DuplicateKeyError: {s}"),
            Expect(s) => write!(f, "ExpectationError: {s}"),
            Match => write!(f, "MatchError: no predicate matched"),
            ContinueOutsideLoop => write!(f, "ContinueOutsideLoop"),
            BreakOutside => write!(f, "BreakOutsideLoopOrMatch"),
            Interrupted => write!(f, "Interrupted"),
        }
    }
}
type EResult = Result<Value, Flow>;
enum Flow {
    Error(Error),
    Break(Value),
    Continue,
}

thread_local! {
    static PARSE_ERROR_SPAN: RefCell<Option<Span>> = const { RefCell::new(None) };
    static CALL_TRACE: RefCell<Vec<(String, Span)>> = const { RefCell::new(Vec::new()) };
    static LAST_TRACE: RefCell<Vec<(String, Span)>> = const { RefCell::new(Vec::new()) };
    static LAST_ERROR_SPAN: RefCell<Option<Span>> = const { RefCell::new(None) };
}
static INTERRUPTED: AtomicBool = AtomicBool::new(false);

fn install_sigint_handler() -> Result<(), Error> {
    ctrlc::set_handler(|| {
        INTERRUPTED.store(true, Ordering::Relaxed);
    })
    .map_err(|error| Error::Io(format!("failed to install Ctrl-C handler: {error}")))
}

fn check_interrupted() -> Result<(), Error> {
    if INTERRUPTED.load(Ordering::Relaxed) {
        Err(Error::Interrupted)
    } else {
        Ok(())
    }
}

impl From<Error> for Flow {
    fn from(e: Error) -> Self {
        Flow::Error(e)
    }
}

#[derive(Clone, Debug, PartialEq)]
enum TokKind {
    LParen,
    RParen,
    LBrack,
    RBrack,
    LBrace,
    RBrace,
    Colon,
    DotDot,
    Dot,
    Caret,
    Symbol(String),
    Str(String),
    Int(i64),
    Float(f64),
}
#[derive(Clone, Debug, PartialEq)]
struct Tok {
    kind: TokKind,
    span: Span,
}

fn lex(src: &str) -> Result<Vec<Tok>, Error> {
    let mut out = Vec::new();
    let cs: Vec<char> = src.chars().collect();
    let mut i = 0;
    while i < cs.len() {
        let c = cs[i];
        let token_start = i;
        if c.is_whitespace() || c == ',' {
            i += 1;
            continue;
        }
        if c == ';' {
            while i < cs.len() && cs[i] != '\n' {
                i += 1
            }
            continue;
        }
        let one = match c {
            '(' => Some(TokKind::LParen),
            ')' => Some(TokKind::RParen),
            '[' => Some(TokKind::LBrack),
            ']' => Some(TokKind::RBrack),
            '{' => Some(TokKind::LBrace),
            '}' => Some(TokKind::RBrace),
            ':' => Some(TokKind::Colon),
            '.' if cs.get(i + 1) == Some(&'.') => {
                i += 1;
                Some(TokKind::DotDot)
            }
            '.' => Some(TokKind::Dot),
            '^' => Some(TokKind::Caret),
            _ => None,
        };
        if let Some(t) = one {
            out.push(Tok {
                kind: t,
                span: Span {
                    start: token_start,
                    end: i + 1,
                },
            });
            i += 1;
            continue;
        }
        if matches!(c, '<' | '>' | '$' | '~') {
            out.push(Tok {
                kind: TokKind::Symbol(c.to_string()),
                span: Span {
                    start: token_start,
                    end: i + 1,
                },
            });
            i += 1;
            continue;
        }
        if c == '"' || c == '\'' {
            let quote = c;
            let raw = quote == '\'';
            i += 1;
            let mut s = String::new();
            let mut closed = false;
            while i < cs.len() {
                let x = cs[i];
                i += 1;
                if x == quote {
                    closed = true;
                    break;
                }
                if x == '\\' && !raw {
                    if i >= cs.len() {
                        break;
                    }
                    let e = cs[i];
                    i += 1;
                    s.push(match e {
                        'n' => '\n',
                        't' => '\t',
                        'r' => '\r',
                        '\\' => '\\',
                        '"' => '"',
                        '\'' => '\'',
                        other => other,
                    });
                } else {
                    s.push(x)
                }
            }
            if !closed {
                return Err(Error::Parse("unterminated string".into()));
            }
            out.push(Tok {
                kind: TokKind::Str(s),
                span: Span {
                    start: token_start,
                    end: i,
                },
            });
            continue;
        }
        // A dot between digits belongs to a float; other dots remain field
        // separators, so `1.0` and `profile.name` can coexist.
        if c.is_ascii_digit()
            || matches!(c, '+' | '-') && cs.get(i + 1).is_some_and(|next| next.is_ascii_digit())
        {
            let start = i;
            if matches!(cs[i], '+' | '-') {
                i += 1;
            }
            while i < cs.len() && cs[i].is_ascii_digit() {
                i += 1;
            }
            if i + 1 < cs.len() && cs[i] == '.' && cs[i + 1].is_ascii_digit() {
                i += 1;
                while i < cs.len() && cs[i].is_ascii_digit() {
                    i += 1;
                }
                let s: String = cs[start..i].iter().collect();
                let kind = number(&s)?.expect("decimal float is a number");
                out.push(Tok {
                    kind,
                    span: Span {
                        start: token_start,
                        end: i,
                    },
                });
                continue;
            }
            i = start;
        }
        let start = i;
        while i < cs.len() && !cs[i].is_whitespace() && !"()[]{}:,.^\"';".contains(cs[i]) {
            i += 1
        }
        if start == i {
            return Err(Error::Parse(format!("unexpected `{c}`")));
        }
        let s: String = cs[start..i].iter().collect();
        if let Some(t) = number(&s)? {
            out.push(Tok {
                kind: t,
                span: Span {
                    start: token_start,
                    end: i,
                },
            })
        } else {
            out.push(Tok {
                kind: TokKind::Symbol(s),
                span: Span {
                    start: token_start,
                    end: i,
                },
            })
        }
    }
    let byte_offsets: Vec<usize> = src
        .char_indices()
        .map(|(offset, _)| offset)
        .chain(std::iter::once(src.len()))
        .collect();
    for token in &mut out {
        token.span.start = byte_offsets[token.span.start];
        token.span.end = byte_offsets[token.span.end];
    }
    Ok(out)
}
fn number(s: &str) -> Result<Option<TokKind>, Error> {
    let (sign, rest) = if let Some(x) = s.strip_prefix('-') {
        (-1i64, x)
    } else if let Some(x) = s.strip_prefix('+') {
        (1, x)
    } else {
        (1, s)
    };
    if rest.starts_with("0x") || rest.starts_with("0b") || rest.starts_with("0o") {
        let (base, d) = match &rest[..2] {
            "0x" => (16, &rest[2..]),
            "0b" => (2, &rest[2..]),
            _ => (8, &rest[2..]),
        };
        if d.is_empty() {
            return Ok(None);
        }
        let v = i64::from_str_radix(d, base)
            .map_err(|_| Error::Parse(format!("invalid number `{s}`")))?;
        return Ok(Some(TokKind::Int(
            v.checked_mul(sign)
                .ok_or_else(|| Error::Parse("integer out of range".into()))?,
        )));
    }
    if rest.chars().all(|c| c.is_ascii_digit()) && !rest.is_empty() {
        return s
            .parse::<i64>()
            .map(|v| Some(TokKind::Int(v)))
            .map_err(|_| Error::Parse(format!("integer out of range `{s}`")));
    }
    if rest.contains('.') {
        return s
            .parse::<f64>()
            .map(|v| Some(TokKind::Float(v)))
            .map_err(|_| Error::Parse(format!("invalid float `{s}`")));
    }
    Ok(None)
}
struct Parser {
    ts: Vec<Tok>,
    i: usize,
}
impl Parser {
    fn peek(&self) -> Option<&Tok> {
        self.ts.get(self.i)
    }
    fn take(&mut self) -> Option<Tok> {
        let x = self.ts.get(self.i).cloned();
        if let Some(token) = &x {
            PARSE_ERROR_SPAN.with(|span| *span.borrow_mut() = Some(token.span));
        }
        self.i += 1;
        x
    }
    fn program(&mut self) -> Result<Vec<Expr>, Error> {
        let mut x = vec![];
        while self.peek().is_some() {
            x.push(self.form()?)
        }
        Ok(x)
    }
    fn form(&mut self) -> Result<Expr, Error> {
        let token = self
            .take()
            .ok_or_else(|| Error::Parse("unexpected end".into()))?;
        let start = token.span.start;
        let expr = match token.kind {
            TokKind::LParen => self.paren(start)?,
            TokKind::LBrack => {
                let mut v = vec![];
                while self.peek().map(|t| &t.kind) != Some(&TokKind::RBrack) {
                    if self.peek().is_none() {
                        return Err(Error::Parse("unclosed array".into()));
                    }
                    v.push(self.form()?)
                }
                let end = self.take().unwrap().span.end;
                Expr {
                    kind: ExprKind::Array(v),
                    span: Span { start, end },
                }
            }
            TokKind::LBrace => self.struct_(start)?,
            TokKind::Caret => {
                let x = self.atom_field()?;
                Expr {
                    span: Span {
                        start,
                        end: x.span.end,
                    },
                    kind: ExprKind::Ref(Box::new(x)),
                }
            }
            TokKind::Str(s) => Expr {
                kind: ExprKind::Lit(Literal::Str(s)),
                span: token.span,
            },
            TokKind::Int(n) => Expr {
                kind: ExprKind::Lit(Literal::Int(n)),
                span: token.span,
            },
            TokKind::Float(n) => Expr {
                kind: ExprKind::Lit(Literal::Float(n)),
                span: token.span,
            },
            TokKind::Symbol(s) => {
                let e = match s.as_str() {
                    "t" => ExprKind::Lit(Literal::Bool(true)),
                    "f" => ExprKind::Lit(Literal::Bool(false)),
                    "_" => ExprKind::Lit(Literal::Null),
                    "NaN" => ExprKind::Lit(Literal::Float(f64::NAN)),
                    "Inf" => ExprKind::Lit(Literal::Float(f64::INFINITY)),
                    "-Inf" => ExprKind::Lit(Literal::Float(f64::NEG_INFINITY)),
                    _ => ExprKind::Symbol(s),
                };
                Expr {
                    kind: e,
                    span: token.span,
                }
            }
            x => return Err(Error::Parse(format!("unexpected token {x:?}"))),
        };
        self.postfix_tail(expr)
    }
    fn atom_field(&mut self) -> Result<Expr, Error> {
        self.form()
    }
    fn postfix_tail(&mut self, mut e: Expr) -> Result<Expr, Error> {
        loop {
            match self.peek().map(|t| &t.kind) {
                Some(TokKind::Dot)
                    if self
                        .peek()
                        .is_some_and(|token| token.span.start == e.span.end) =>
                {
                    self.take();
                    let key_token = self.take();
                    let (key, end) = match key_token {
                        Some(Tok {
                            kind: TokKind::Symbol(x),
                            span,
                        }) => (x, span.end),
                        _ => return Err(Error::Parse("expected struct field after dot".into())),
                    };
                    e = Expr {
                        span: Span {
                            start: e.span.start,
                            end,
                        },
                        kind: ExprKind::Field(Box::new(e), key),
                    };
                }
                Some(TokKind::LBrack)
                    if self
                        .peek()
                        .is_some_and(|token| token.span.start == e.span.end) =>
                {
                    self.take();
                    let spec = self.index_spec()?;
                    let end = self
                        .take()
                        .ok_or_else(|| Error::Parse("unclosed index selector".into()))?;
                    if end.kind != TokKind::RBrack {
                        return Err(Error::Parse("expected ] after index selector".into()));
                    }
                    e = Expr {
                        span: Span {
                            start: e.span.start,
                            end: end.span.end,
                        },
                        kind: ExprKind::Index(Box::new(e), spec),
                    };
                }
                _ => break,
            }
        }
        Ok(e)
    }
    fn index_spec(&mut self) -> Result<IndexSpec, Error> {
        if self.peek().map(|t| &t.kind) == Some(&TokKind::DotDot) {
            self.take();
            let end = if self.peek().map(|t| &t.kind) == Some(&TokKind::RBrack) {
                None
            } else {
                Some(Box::new(self.form()?))
            };
            return Ok(IndexSpec::Range(None, end));
        }
        let first = self.form()?;
        if self.peek().map(|t| &t.kind) == Some(&TokKind::DotDot) {
            self.take();
            let end = if self.peek().map(|t| &t.kind) == Some(&TokKind::RBrack) {
                None
            } else {
                Some(Box::new(self.form()?))
            };
            Ok(IndexSpec::Range(Some(Box::new(first)), end))
        } else {
            Ok(IndexSpec::Selector(Box::new(first)))
        }
    }
    fn paren(&mut self, start: usize) -> Result<Expr, Error> {
        if self.peek().map(|t| &t.kind) == Some(&TokKind::RParen) {
            let end = self.take().unwrap().span.end;
            return Ok(Expr {
                kind: ExprKind::Block(vec![]),
                span: Span { start, end },
            });
        }
        let first = self.form()?;
        let mut rest = vec![];
        while self.peek().map(|t| &t.kind) != Some(&TokKind::RParen) {
            if self.peek().is_none() {
                return Err(Error::Parse("unclosed parenthesis".into()));
            }
            rest.push(self.form()?)
        }
        let end = self.take().unwrap().span.end;
        match first.kind {
            ExprKind::Symbol(_) | ExprKind::Field(_, _) => Ok(Expr {
                kind: ExprKind::Call(Box::new(first), rest),
                span: Span { start, end },
            }),
            _ => {
                let mut all = vec![first];
                all.extend(rest);
                Ok(Expr {
                    kind: ExprKind::Block(all),
                    span: Span { start, end },
                })
            }
        }
    }
    fn struct_(&mut self, start: usize) -> Result<Expr, Error> {
        let mut v = vec![];
        while self.peek().map(|t| &t.kind) != Some(&TokKind::RBrace) {
            let key = match self.take() {
                Some(Tok {
                    kind: TokKind::Symbol(x),
                    span,
                }) => (x, span),
                _ => return Err(Error::Parse("struct key must be an identifier".into())),
            };
            if self.take().map(|t| t.kind) != Some(TokKind::Colon) {
                return Err(Error::Parse("expected : after struct key".into()));
            }
            let val = self.form()?;
            v.push(StructField {
                key: key.0,
                value: val,
                span: key.1,
            });
        }
        let end = self.take().unwrap().span.end;
        Ok(Expr {
            kind: ExprKind::Struct(v),
            span: Span { start, end },
        })
    }
}

fn new_env(parent: Option<EnvRef>) -> EnvRef {
    Rc::new(RefCell::new(Env {
        values: vec![],
        parent,
    }))
}
fn lookup(env: &EnvRef, n: &str) -> Option<Cell> {
    let e = env.borrow();
    if let Some((_, v)) = e.values.iter().rev().find(|(k, _)| k == n) {
        return Some(v.clone());
    }
    e.parent.clone().and_then(|p| lookup(&p, n))
}
fn bind_value(name: &str, value: Value, env: &EnvRef) -> Result<(), Error> {
    if let Some(cell) = lookup(env, name) {
        let cell = follow(cell)?;
        if value_references_cell(&value, &cell) {
            return Err(Error::Type("cyclic reference".into()));
        }
        *cell.borrow_mut() = value;
    } else {
        env.borrow_mut()
            .values
            .push((name.to_owned(), Rc::new(RefCell::new(value))));
    }
    Ok(())
}
fn follow(mut c: Cell) -> Result<Cell, Error> {
    let mut visited = HashSet::new();
    loop {
        let identity = Rc::as_ptr(&c) as usize;
        if !visited.insert(identity) {
            return Err(Error::Type("cyclic reference".into()));
        }
        let v = c.borrow().clone();
        if let Value::Ref(next) = v {
            c = next
        } else {
            return Ok(c);
        }
    }
}
fn value_references_cell(value: &Value, target: &Cell) -> bool {
    fn visit(
        value: &Value,
        target: &Cell,
        cells: &mut HashSet<usize>,
        arrays: &mut HashSet<usize>,
        structs: &mut HashSet<usize>,
    ) -> bool {
        match value {
            Value::Ref(cell) => {
                if Rc::ptr_eq(cell, target) {
                    return true;
                }
                let identity = Rc::as_ptr(cell) as usize;
                cells.insert(identity) && visit(&cell.borrow(), target, cells, arrays, structs)
            }
            Value::Array(values) => {
                let identity = Rc::as_ptr(values) as usize;
                arrays.insert(identity)
                    && values
                        .borrow()
                        .iter()
                        .any(|cell| visit(&cell.borrow(), target, cells, arrays, structs))
            }
            Value::Struct(fields) => {
                let identity = Rc::as_ptr(fields) as usize;
                structs.insert(identity)
                    && fields
                        .borrow()
                        .iter()
                        .any(|(_, cell)| visit(&cell.borrow(), target, cells, arrays, structs))
            }
            _ => false,
        }
    }

    visit(
        value,
        target,
        &mut HashSet::new(),
        &mut HashSet::new(),
        &mut HashSet::new(),
    )
}
fn copy(v: &Value) -> Value {
    match v {
        Value::Ref(c) => copy(&c.borrow()),
        Value::Array(a) => Value::Array(Rc::new(RefCell::new(
            a.borrow()
                .iter()
                .map(|c| Rc::new(RefCell::new(copy(&c.borrow()))))
                .collect(),
        ))),
        Value::Struct(s) => Value::Struct(Rc::new(RefCell::new(
            s.borrow()
                .iter()
                .map(|(k, c)| (k.clone(), Rc::new(RefCell::new(copy(&c.borrow())))))
                .collect(),
        ))),
        x => x.clone(),
    }
}
fn truth(v: &Value) -> bool {
    match v {
        Value::Null | Value::Bool(false) => false,
        Value::Int(0) => false,
        Value::Float(x) if *x == 0.0 => false,
        _ => true,
    }
}
fn render(v: &Value) -> String {
    match v {
        Value::Null => String::new(),
        Value::Bool(x) => x.to_string(),
        Value::Int(x) => x.to_string(),
        Value::Float(x) => float_render(*x),
        Value::Str(x) => x.clone(),
        Value::Ref(c) => render(&c.borrow()),
        Value::Array(a) => format!(
            "[{}]",
            a.borrow()
                .iter()
                .map(|x| render_nested(&x.borrow()))
                .collect::<Vec<_>>()
                .join(" ")
        ),
        Value::Struct(s) => format!(
            "{{{}}}",
            s.borrow()
                .iter()
                .map(|(k, v)| format!("{k}:{}", render_nested(&v.borrow())))
                .collect::<Vec<_>>()
                .join(" ")
        ),
        Value::Function(_) => "<fn>".into(),
        Value::NativeFunction(_) => "<native fn>".into(),
    }
}
fn render_nested(v: &Value) -> String {
    match v {
        Value::Str(value) => json_string(value),
        Value::Ref(cell) => render_nested(&cell.borrow()),
        Value::Array(_) | Value::Struct(_) => render(v),
        _ => render(v),
    }
}
fn location(e: &Expr, env: &EnvRef) -> Result<Cell, Error> {
    match &e.kind {
        ExprKind::Symbol(n) => lookup(env, n).ok_or_else(|| Error::Name(n.clone())),
        ExprKind::Field(base, key) => {
            let c = follow(location(base, env)?)?;
            let result = match &*c.borrow() {
                Value::Struct(s) => s
                    .borrow()
                    .iter()
                    .find(|(k, _)| k == key)
                    .map(|(_, v)| v.clone())
                    .ok_or_else(|| Error::Name(format!("field {key}"))),
                _ => Err(Error::Type("struct field access requires a struct".into())),
            };
            result
        }
        ExprKind::Index(base, IndexSpec::Selector(selector)) => {
            let c = follow(location(base, env)?)?;
            let index = match eval(selector, env, 0, 0).map_err(flow_err)? {
                Value::Int(index) => index,
                Value::Array(_) => {
                    return Err(Error::Type(
                        "a multi-index selector cannot be a reference target".into(),
                    ))
                }
                _ => return Err(Error::Type("array index must be an integer".into())),
            };
            let result = match &*c.borrow() {
                Value::Array(array) => {
                    let array = array.borrow();
                    array
                        .get(collection_position(
                            &Value::Int(index),
                            array.len(),
                            "array",
                        )?)
                        .cloned()
                        .ok_or_else(|| Error::Name(format!("array index {index} out of bounds")))
                }
                _ => Err(Error::Type("indexing requires an array".into())),
            };
            result
        }
        ExprKind::Index(_, IndexSpec::Range(_, _)) => {
            Err(Error::Type("a slice cannot be a reference target".into()))
        }
        _ => Err(Error::Type(
            "reference target must be a variable or field".into(),
        )),
    }
}

fn eval(e: &Expr, env: &EnvRef, loop_depth: usize, match_depth: usize) -> EResult {
    if let Err(error) = check_interrupted() {
        LAST_ERROR_SPAN.with(|span| *span.borrow_mut() = Some(e.span));
        return Err(error.into());
    }
    LAST_ERROR_SPAN.with(|span| *span.borrow_mut() = Some(e.span));
    match &e.kind {
        ExprKind::Lit(x) => Ok(match x {
            Literal::Null => Value::Null,
            Literal::Bool(b) => Value::Bool(*b),
            Literal::Int(n) => Value::Int(*n),
            Literal::Float(n) => Value::Float(*n),
            Literal::Str(s) => Value::Str(s.clone()),
        }),
        ExprKind::Symbol(n) => lookup(env, n)
            .map(|c| copy(&c.borrow()))
            .ok_or_else(|| Flow::Error(Error::Name(n.clone()))),
        ExprKind::Field(_, _) => location(e, env)
            .map(|c| copy(&c.borrow()))
            .map_err(Into::into),
        ExprKind::Index(target, spec) => {
            let value = eval(target, env, loop_depth, match_depth)?;
            apply_index(value, spec, env, loop_depth, match_depth)
        }
        ExprKind::Ref(x) => location(x, env).map(Value::Ref).map_err(Into::into),
        ExprKind::Array(xs) => {
            let mut v = vec![];
            for x in xs {
                v.push(Rc::new(RefCell::new(eval(
                    x,
                    env,
                    loop_depth,
                    match_depth,
                )?)))
            }
            Ok(Value::Array(Rc::new(RefCell::new(v))))
        }
        ExprKind::Struct(xs) => {
            let mut seen = HashSet::new();
            let mut v = vec![];
            for field in xs {
                if !seen.insert(&field.key) {
                    LAST_ERROR_SPAN.with(|span| *span.borrow_mut() = Some(field.span));
                    return Err(Error::DuplicateKey(field.key.clone()).into());
                }
                v.push((
                    field.key.clone(),
                    Rc::new(RefCell::new(eval(
                        &field.value,
                        env,
                        loop_depth,
                        match_depth,
                    )?)),
                ))
            }
            Ok(Value::Struct(Rc::new(RefCell::new(v))))
        }
        ExprKind::Block(xs) => {
            let child = new_env(Some(env.clone()));
            let mut r = Value::Null;
            for x in xs {
                r = eval(x, &child, loop_depth, match_depth)?
            }
            Ok(r)
        }
        ExprKind::Call(head, args) => call(head, args, env, loop_depth, match_depth, e.span),
    }
}
fn apply_index(
    value: Value,
    spec: &IndexSpec,
    env: &EnvRef,
    loop_depth: usize,
    match_depth: usize,
) -> EResult {
    let selector = match spec {
        IndexSpec::Selector(expr) => eval(expr, env, loop_depth, match_depth)?,
        IndexSpec::Range(_, _) => Value::Null,
    };
    match value {
        Value::Array(array) => {
            let values = array.borrow();
            match spec {
                IndexSpec::Selector(_) => {
                    if let Value::Array(indices) = selector {
                        let indices = indices.borrow();
                        let mut selected = Vec::with_capacity(indices.len());
                        for index in indices.iter() {
                            let position =
                                collection_position(&index.borrow(), values.len(), "array")?;
                            selected.push(Rc::new(RefCell::new(copy(&values[position].borrow()))));
                        }
                        Ok(Value::Array(Rc::new(RefCell::new(selected))))
                    } else {
                        let position = collection_position(&selector, values.len(), "array")?;
                        Ok(copy(&values[position].borrow()))
                    }
                }
                IndexSpec::Range(start, end) => {
                    let Some((start, end)) = range_positions(
                        start,
                        end,
                        env,
                        loop_depth,
                        match_depth,
                        values.len(),
                        "array",
                    )?
                    else {
                        return Ok(Value::Array(Rc::new(RefCell::new(Vec::new()))));
                    };
                    let selected = values[start..=end]
                        .iter()
                        .map(|cell| Rc::new(RefCell::new(copy(&cell.borrow()))))
                        .collect();
                    Ok(Value::Array(Rc::new(RefCell::new(selected))))
                }
            }
        }
        Value::Str(string) => {
            let graphemes: Vec<&str> = string.graphemes(true).collect();
            match spec {
                IndexSpec::Selector(_) => {
                    if let Value::Array(indices) = selector {
                        let indices = indices.borrow();
                        let mut selected = Vec::with_capacity(indices.len());
                        for index in indices.iter() {
                            let position =
                                collection_position(&index.borrow(), graphemes.len(), "string")?;
                            selected.push(Rc::new(RefCell::new(Value::Str(
                                graphemes[position].to_owned(),
                            ))));
                        }
                        Ok(Value::Array(Rc::new(RefCell::new(selected))))
                    } else {
                        let position = collection_position(&selector, graphemes.len(), "string")?;
                        Ok(Value::Str(graphemes[position].to_owned()))
                    }
                }
                IndexSpec::Range(start, end) => {
                    let Some((start, end)) = range_positions(
                        start,
                        end,
                        env,
                        loop_depth,
                        match_depth,
                        graphemes.len(),
                        "string",
                    )?
                    else {
                        return Ok(Value::Str(String::new()));
                    };
                    Ok(Value::Str(graphemes[start..=end].concat()))
                }
            }
        }
        _ => Err(Error::Type("indexing requires an array".into()).into()),
    }
}
fn collection_position(value: &Value, len: usize, collection: &str) -> Result<usize, Error> {
    let index = match value {
        Value::Int(index) => *index,
        _ => {
            return Err(Error::Type(format!(
                "{collection} index must be an integer"
            )))
        }
    };
    if index == 0 {
        return Err(Error::Type(format!("{collection} indices are 1-based")));
    }
    let position = if index > 0 {
        usize::try_from(index - 1)
            .map_err(|_| Error::Name(format!("{collection} index out of bounds")))?
    } else {
        let magnitude = usize::try_from(index.unsigned_abs())
            .map_err(|_| Error::Name(format!("{collection} index out of bounds")))?;
        len.checked_sub(magnitude)
            .ok_or_else(|| Error::Name(format!("{collection} index {index} out of bounds")))?
    };
    if position >= len {
        Err(Error::Name(format!(
            "{collection} index {index} out of bounds"
        )))
    } else {
        Ok(position)
    }
}
fn range_positions(
    start: &Option<Box<Expr>>,
    end: &Option<Box<Expr>>,
    env: &EnvRef,
    loop_depth: usize,
    match_depth: usize,
    len: usize,
    collection: &str,
) -> Result<Option<(usize, usize)>, Flow> {
    if len == 0 && start.is_none() && end.is_none() {
        return Ok(None);
    }
    let start = match start {
        Some(expr) => eval(expr, env, loop_depth, match_depth)?,
        None => Value::Int(1),
    };
    let end = match end {
        Some(expr) => eval(expr, env, loop_depth, match_depth)?,
        None => Value::Int(len as i64),
    };
    let start = collection_position(&start, len, collection)?;
    let end = collection_position(&end, len, collection)?;
    if start > end {
        return Err(Error::Type(format!("{collection} slice start must not exceed end")).into());
    }
    Ok(Some((start, end)))
}
fn values(args: &[Expr], env: &EnvRef, l: usize, m: usize) -> Result<Vec<Value>, Flow> {
    args.iter().map(|x| eval(x, env, l, m)).collect()
}
fn need(args: &[Expr], n: usize, name: &str) -> Result<(), Flow> {
    if args.len() == n {
        Ok(())
    } else {
        Err(Error::Arity(format!("{name} expects {n} arguments, got {}", args.len())).into())
    }
}
fn define_let(args: &[Expr], env: &EnvRef, l: usize, m: usize) -> Result<(String, Value), Flow> {
    need(args, 2, "let")?;
    let name = if let ExprKind::Symbol(name) = &args[0].kind {
        name.clone()
    } else {
        return Err(Error::Type("let name must be an identifier".into()).into());
    };
    let mut value = eval(&args[1], env, l, m)?;
    if let Value::Function(function) = &mut value {
        if let Some(function) = Rc::get_mut(function) {
            function.name = Some(name.clone());
        }
    }
    env.borrow_mut()
        .values
        .push((name.clone(), Rc::new(RefCell::new(value.clone()))));
    Ok((name, value))
}
fn call(head: &Expr, args: &[Expr], env: &EnvRef, l: usize, m: usize, call_span: Span) -> EResult {
    if let ExprKind::Symbol(name) = &head.kind {
        match name.as_str() {
            "let" => {
                define_let(args, env, l, m)?;
                return Ok(Value::Null);
            }
            "set" => {
                need(args, 2, "set")?;
                let c = location(&args[0], env).map_err(Flow::Error)?;
                let v = eval(&args[1], env, l, m)?;
                let c = follow(c).map_err(Flow::Error)?;
                if value_references_cell(&v, &c) {
                    return Err(Error::Type("cyclic reference".into()).into());
                }
                *c.borrow_mut() = v;
                return Ok(Value::Null);
            }
            "if" => {
                if args.len() != 2 && args.len() != 3 {
                    return Err(Error::Arity("if expects 2 or 3 arguments".into()).into());
                }
                if truth(&eval(&args[0], env, l, m)?) {
                    return eval(&args[1], env, l, m);
                }
                return if args.len() == 3 {
                    eval(&args[2], env, l, m)
                } else {
                    Ok(Value::Null)
                };
            }
            "fn" => {
                if args.len() < 2 {
                    return Err(Error::Arity("fn expects parameters and body".into()).into());
                }
                let ps = match &args[0].kind {
                    ExprKind::Block(xs) => xs
                        .iter()
                        .map(|x| {
                            if let ExprKind::Symbol(s) = &x.kind {
                                Ok(s.clone())
                            } else {
                                Err(Error::Type("function parameter must be identifier".into()))
                            }
                        })
                        .collect::<Result<Vec<_>, _>>()
                        .map_err(Flow::Error)?,
                    ExprKind::Call(h, xs) => {
                        let mut all = vec![*h.clone()];
                        all.extend(xs.clone());
                        all.iter()
                            .map(|x| {
                                if let ExprKind::Symbol(s) = &x.kind {
                                    Ok(s.clone())
                                } else {
                                    Err(Error::Type("function parameter must be identifier".into()))
                                }
                            })
                            .collect::<Result<Vec<_>, _>>()
                            .map_err(Flow::Error)?
                    }
                    _ => return Err(Error::Type("fn parameters must use ()".into()).into()),
                };
                let fun = Value::Function(Rc::new(Function {
                    params: ps,
                    body: args[1].clone(),
                    env: env.clone(),
                    name: None,
                }));
                if args.len() == 2 {
                    return Ok(fun);
                }
                return invoke(fun, values(&args[2..], env, l, m)?, call_span);
            }
            "loop" => {
                let local = new_env(Some(env.clone()));
                loop {
                    check_interrupted().map_err(Flow::Error)?;
                    for a in args {
                        match eval(a, &local, l + 1, m) {
                            Ok(_) => {}
                            Err(Flow::Continue) => break,
                            Err(Flow::Break(v)) => return Ok(v),
                            Err(e) => return Err(e),
                        }
                    }
                }
            }
            "break" => {
                if l == 0 && m == 0 {
                    return Err(Error::BreakOutside.into());
                }
                if args.len() > 1 {
                    return Err(Error::Arity("break expects zero or one argument".into()).into());
                }
                return Err(Flow::Break(if args.is_empty() {
                    Value::Null
                } else {
                    eval(&args[0], env, l, m)?
                }));
            }
            "continue" => {
                need(args, 0, "continue")?;
                return if l == 0 {
                    Err(Error::ContinueOutsideLoop.into())
                } else {
                    Err(Flow::Continue)
                };
            }
            "match" => {
                if !args.len().is_multiple_of(2) {
                    return Err(
                        Error::Arity("match expects predicate/expression pairs".into()).into(),
                    );
                }
                for p in args.chunks(2) {
                    match eval(&p[0], env, l, m + 1) {
                        Ok(v) if truth(&v) => {
                            return match eval(&p[1], env, l, m + 1) {
                                Err(Flow::Break(v)) => Ok(v),
                                x => x,
                            }
                        }
                        Ok(_) => {}
                        Err(x) => return Err(x),
                    }
                }
                return Err(Error::Match.into());
            }
            "and" => {
                let mut r = Value::Bool(true);
                for a in args {
                    r = eval(a, env, l, m)?;
                    if !truth(&r) {
                        return Ok(r);
                    }
                }
                return Ok(r);
            }
            "or" => {
                let mut r = Value::Bool(false);
                for a in args {
                    r = eval(a, env, l, m)?;
                    if truth(&r) {
                        return Ok(r);
                    }
                }
                return Ok(r);
            }
            "not" => {
                need(args, 1, "not")?;
                return Ok(Value::Bool(!truth(&eval(&args[0], env, l, m)?)));
            }
            "expect" => {
                if args.len() != 2 && args.len() != 3 {
                    return Err(Error::Arity(format!(
                        "expect expects 2 or 3 arguments, got {}",
                        args.len()
                    ))
                    .into());
                }
                let actual = eval(&args[0], env, l, m)?;
                let expected = eval(&args[1], env, l, m)?;
                if equals(&actual, &expected) {
                    return Ok(Value::Bool(true));
                }
                let comment = if args.len() == 3 {
                    as_str(eval(&args[2], env, l, m)?)?
                } else {
                    "expectation failed".into()
                };
                return Err(Error::Expect(format!(
                    "{comment}: expected {}, got {}",
                    debug_render(&expected),
                    debug_render(&actual)
                ))
                .into());
            }
            "use" => {
                need(args, 1, "use")?;
                let (name, module) = match &args[0].kind {
                    ExprKind::Call(let_head, let_args) if matches!(&let_head.kind, ExprKind::Symbol(name) if name == "let") => {
                        define_let(let_args, env, l, m)?
                    }
                    ExprKind::Lit(Literal::Str(name)) => (name.clone(), native_module(name)?),
                    _ => {
                        let name = as_str(eval(&args[0], env, l, m)?)?;
                        let module = native_module(&name)?;
                        (name, module)
                    }
                };
                validate_module(&module).map_err(Flow::Error)?;
                bind_value(&name, module.clone(), env).map_err(Flow::Error)?;
                return Ok(module);
            }
            "io/open" => return io_open(args, env, l, m),
            "io/close" => return io_close(args, env, l, m),
            "io/read" => return io_read(args, env, l, m),
            "io/write" => return io_write(args, env, l, m),
            "@" => return http_request(args, env, l, m),
            "$" => return format_value(args, env, l, m),
            "~" => {
                if args.len() != 3 {
                    return Err(
                        Error::Arity(format!("~ expects 3 arguments, got {}", args.len())).into(),
                    );
                }
                let name = if let ExprKind::Symbol(name) = &args[2].kind {
                    name
                } else {
                    return Err(Error::Type("regex binding must be an identifier".into()).into());
                };
                let pattern = as_str(eval(&args[0], env, l, m)?)?;
                let text = as_str(eval(&args[1], env, l, m)?)?;
                let regex = Regex::new(&pattern)
                    .map_err(|error| Error::Regex(format!("invalid regex: {error}")))?;
                let Some(captures) = regex.captures(&text) else {
                    return Ok(Value::Bool(false));
                };
                let result = Value::Array(Rc::new(RefCell::new(
                    captures
                        .iter()
                        .map(|capture| {
                            Rc::new(RefCell::new(match capture {
                                Some(value) => Value::Str(value.as_str().to_owned()),
                                None => Value::Null,
                            }))
                        })
                        .collect(),
                )));
                bind_value(name, result.clone(), env).map_err(Flow::Error)?;
                return Ok(result);
            }
            _ => {
                if [
                    "add", "sub", "mul", "div", "mod", "pow", "eq", "ne", "lt", "gt", "le", "ge",
                    "bit-and", "bit-or", "bit-xor", "bit-not", "bit-shl", "bit-shr",
                ]
                .contains(&name.as_str())
                {
                    return builtin(name, values(args, env, l, m)?).map_err(Into::into);
                }
            }
        }
    }
    let value = eval(head, env, l, m)?;
    let arguments = values(args, env, l, m)?;
    invoke_operator(value, arguments, call_span)
}
fn invoke_operator(value: Value, vals: Vec<Value>, call_span: Span) -> EResult {
    if let Value::Struct(fields) = &value {
        if fields.borrow().iter().any(|(key, _)| key == "_") {
            if let Err(error) = validate_descriptor(fields, &vals) {
                LAST_ERROR_SPAN.with(|span| *span.borrow_mut() = Some(call_span));
                return Err(error.into());
            }
        }
    }
    invoke(operator_value(value), vals, call_span)
}
fn validate_module(value: &Value) -> Result<(), Error> {
    let Value::Struct(fields) = value else {
        return Err(Error::Type("module must be a struct".into()));
    };
    for (_, cell) in fields.borrow().iter() {
        let Value::Struct(descriptor) = copy(&cell.borrow()) else {
            return Err(Error::Type(
                "module members must be callable descriptors".into(),
            ));
        };
        validate_descriptor_spec(&descriptor)?;
    }
    Ok(())
}
fn validate_descriptor_spec(
    fields: &Rc<RefCell<Vec<(String, Cell)>>>,
) -> Result<(usize, Vec<String>), Error> {
    let field = |name: &str| {
        fields
            .borrow()
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, cell)| copy(&cell.borrow()))
    };
    match field("_") {
        Some(value) if is_callable(&value) => {}
        Some(_) => return Err(Error::Type("module descriptor _ must be callable".into())),
        None => return Err(Error::Type("module descriptor must contain _".into())),
    }
    let spec = match field("spec") {
        Some(Value::Struct(spec)) => spec,
        Some(_) => {
            return Err(Error::Type(
                "module descriptor spec must be a struct".into(),
            ))
        }
        None => {
            return Err(Error::Type(
                "module descriptor must contain a spec field".into(),
            ))
        }
    };
    let spec_field = |name: &str| {
        spec.borrow()
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, cell)| copy(&cell.borrow()))
    };
    if !matches!(spec_field("documentation"), Some(Value::Str(_))) {
        return Err(Error::Type(
            "module descriptor spec.documentation must be a string".into(),
        ));
    }
    let arity = match spec_field("arity") {
        Some(Value::Int(arity)) if arity >= 0 => arity as usize,
        _ => {
            return Err(Error::Type(
                "module descriptor spec.arity must be an integer".into(),
            ))
        }
    };
    let types = match spec_field("type") {
        Some(Value::Null) if arity == 0 => Vec::new(),
        Some(Value::Array(types)) if arity > 0 => types
            .borrow()
            .iter()
            .map(|cell| match &*cell.borrow() {
                Value::Str(value) => Ok(value.clone()),
                _ => Err(Error::Type(
                    "module descriptor spec.type entries must be strings".into(),
                )),
            })
            .collect::<Result<Vec<_>, _>>()?,
        _ => {
            return Err(Error::Type(
                "module descriptor spec.type must be an array, or _ for arity 0".into(),
            ))
        }
    };
    if types.len() != arity {
        return Err(Error::Type(
            "module descriptor spec.type length must match spec.arity".into(),
        ));
    }
    if !matches!(spec_field("return"), Some(Value::Null | Value::Array(_))) {
        return Err(Error::Type(
            "module descriptor spec.return must be an array or null".into(),
        ));
    }
    Ok((arity, types))
}
fn validate_descriptor(
    fields: &Rc<RefCell<Vec<(String, Cell)>>>,
    vals: &[Value],
) -> Result<(), Error> {
    let (arity, types) = validate_descriptor_spec(fields)?;
    if vals.len() != arity {
        return Err(Error::Arity(format!(
            "module function expects {arity} arguments, got {}",
            vals.len()
        )));
    }
    for (index, (expected, actual)) in types.iter().zip(vals).enumerate() {
        let actual_type = value_type(actual);
        if expected != actual_type {
            return Err(Error::Type(format!(
                "module function argument {} expects {expected}, got {actual_type}",
                index + 1
            )));
        }
    }
    Ok(())
}
fn is_callable(value: &Value) -> bool {
    match value {
        Value::Function(_) | Value::NativeFunction(_) => true,
        Value::Ref(cell) => is_callable(&cell.borrow()),
        _ => false,
    }
}
fn native_module(name: &str) -> Result<Value, Error> {
    let factory = modules::registry()
        .remove(name)
        .ok_or_else(|| Error::Name(format!("unknown native module {name}")))?;
    Ok(factory())
}
fn operator_value(value: Value) -> Value {
    match value {
        Value::Struct(fields) => fields
            .borrow()
            .iter()
            .find(|(key, _)| key == "_")
            .map(|(_, value)| value.clone())
            .map_or(Value::Struct(fields.clone()), |value| {
                operator_value(copy(&value.borrow()))
            }),
        value => value,
    }
}
fn invoke(f: Value, vals: Vec<Value>, call_span: Span) -> EResult {
    let f = match f {
        Value::Function(f) => f,
        Value::NativeFunction(f) => {
            return (f.call)(vals).map_err(Into::into);
        }
        Value::Ref(c) => return invoke(copy(&c.borrow()), vals, call_span),
        _ => return Err(Error::Type("value is not callable".into()).into()),
    };
    if f.params.len() != vals.len() {
        return Err(Error::Arity(format!(
            "function expects {}, got {}",
            f.params.len(),
            vals.len()
        ))
        .into());
    }
    let e = new_env(Some(f.env.clone()));
    for (n, v) in f.params.iter().zip(vals) {
        e.borrow_mut()
            .values
            .push((n.clone(), Rc::new(RefCell::new(v))));
    }
    let name = f
        .name
        .clone()
        .unwrap_or_else(|| "<anonymous function>".into());
    CALL_TRACE.with(|trace| trace.borrow_mut().push((name, call_span)));
    let result = eval(&f.body, &e, 0, 0);
    if result.is_err() {
        CALL_TRACE.with(|trace| {
            LAST_TRACE.with(|last| {
                let trace = trace.borrow();
                if trace.len() > last.borrow().len() {
                    *last.borrow_mut() = trace.clone();
                }
            });
        });
    }
    CALL_TRACE.with(|trace| {
        trace.borrow_mut().pop();
    });
    result
}
fn as_int(v: Value) -> Result<i64, Flow> {
    if let Value::Int(x) = v {
        Ok(x)
    } else {
        Err(Error::Type("expected integer".into()).into())
    }
}
fn as_str(v: Value) -> Result<String, Flow> {
    if let Value::Str(x) = v {
        Ok(x)
    } else {
        Err(Error::Type("expected string".into()).into())
    }
}
fn http_request(args: &[Expr], env: &EnvRef, l: usize, m: usize) -> EResult {
    if args.len() != 2 && args.len() != 3 {
        return Err(Error::Arity(format!("@ expects 2 or 3 arguments, got {}", args.len())).into());
    }
    let url = as_str(eval(&args[0], env, l, m)?)?;
    let method = as_str(eval(&args[1], env, l, m)?)?.to_ascii_uppercase();
    if !matches!(
        method.as_str(),
        "GET" | "POST" | "PUT" | "PATCH" | "DELETE" | "HEAD"
    ) {
        return Err(Error::Type(format!("@ unsupported HTTP method: {method}")).into());
    }
    let body = if args.len() == 3 {
        Some(json_render(&eval(&args[2], env, l, m)?).map_err(Flow::Error)?)
    } else {
        None
    };
    let request = ureq::request(&method, &url).set("Accept", "application/json");
    let response = match body {
        Some(body) => request
            .set("Content-Type", "application/json")
            .send_string(&body),
        None => request.call(),
    }
    .map_err(|error| match error {
        ureq::Error::Status(code, _) => {
            Error::Io(format!("HTTP request failed with status {code}"))
        }
        ureq::Error::Transport(error) => Error::Io(format!("HTTP request failed: {error}")),
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
        .map_err(|error| Error::Io(format!("failed to read HTTP response: {error}")))?;
    if content_type
        .split(';')
        .next()
        .is_some_and(|media_type| media_type.trim().eq_ignore_ascii_case("application/json"))
    {
        let json: serde_json::Value = serde_json::from_str(&text)
            .map_err(|error| Error::Io(format!("invalid JSON response: {error}")))?;
        json_to_value(json).map_err(Flow::Error)
    } else {
        Ok(Value::Str(text))
    }
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
                Err(Error::Io("JSON number cannot be represented".into()))
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
fn io_open(args: &[Expr], env: &EnvRef, l: usize, m: usize) -> EResult {
    need(args, 2, "io/open")?;
    let mode = as_str(eval(&args[0], env, l, m)?)?;
    let path = as_str(eval(&args[1], env, l, m)?)?;
    let mut options = OpenOptions::new();
    match mode.as_str() {
        "r" => {
            options.read(true);
        }
        "r+" => {
            options.read(true).write(true);
        }
        "w" => {
            options.write(true).create(true).truncate(true);
        }
        "w+" => {
            options.read(true).write(true).create(true).truncate(true);
        }
        "a" => {
            options.append(true).create(true);
        }
        "a+" => {
            options.read(true).append(true).create(true);
        }
        _ => return Err(Error::Io(format!("unsupported open mode `{mode}`")).into()),
    }
    let file = options
        .open(path)
        .map_err(|e| Flow::Error(Error::Io(e.to_string())))?;
    let fd = FILES.with(|files| {
        let mut files = files.borrow_mut();
        let fd = files.next_fd;
        files.next_fd += 1;
        files.files.insert(fd, FileHandle::File(file));
        fd
    });
    Ok(Value::Int(fd))
}
fn io_close(args: &[Expr], env: &EnvRef, l: usize, m: usize) -> EResult {
    need(args, 1, "io/close")?;
    let fd = as_int(eval(&args[0], env, l, m)?)?;
    Ok(Value::Bool(FILES.with(|files| {
        files.borrow_mut().files.remove(&fd).is_some()
    })))
}
fn io_read(args: &[Expr], env: &EnvRef, l: usize, m: usize) -> EResult {
    need(args, 1, "io/read")?;
    let fd = as_int(eval(&args[0], env, l, m)?)?;
    let line = FILES.with(|files| {
        let mut files = files.borrow_mut();
        let handle = files
            .files
            .get_mut(&fd)
            .ok_or_else(|| Error::Io(format!("invalid file descriptor {fd}")))?;
        match handle {
            FileHandle::File(file) => read_line(file),
            FileHandle::Stdin => read_stdin_line(),
            FileHandle::Stdout => Err(Error::Io("file descriptor 1 is not readable".into())),
            FileHandle::Stderr => Err(Error::Io("file descriptor 2 is not readable".into())),
        }
    });
    Ok(match line.map_err(Flow::Error)? {
        Some(line) => Value::Str(line),
        None => Value::Null,
    })
}
fn read_line(file: &mut File) -> Result<Option<String>, Error> {
    check_interrupted()?;
    let mut bytes = Vec::new();
    let mut byte = [0; 1];
    loop {
        check_interrupted()?;
        match file.read(&mut byte).map_err(|e| Error::Io(e.to_string()))? {
            0 if bytes.is_empty() => return Ok(None),
            0 => break,
            1 if byte[0] == b'\n' => break,
            1 => bytes.push(byte[0]),
            _ => unreachable!(),
        }
    }
    if bytes.last() == Some(&b'\r') {
        bytes.pop();
    }
    String::from_utf8(bytes)
        .map(Some)
        .map_err(|_| Error::Io("input is not valid UTF-8".into()))
}
fn read_stdin_line() -> Result<Option<String>, Error> {
    check_interrupted()?;
    let mut line = String::new();
    let bytes = io::stdin()
        .read_line(&mut line)
        .map_err(|e| Error::Io(e.to_string()))?;
    check_interrupted()?;
    if bytes == 0 {
        Ok(None)
    } else {
        Ok(Some(line.trim_end_matches(['\n', '\r']).to_string()))
    }
}
fn io_write(args: &[Expr], env: &EnvRef, l: usize, m: usize) -> EResult {
    need(args, 2, "io/write")?;
    let fd = as_int(eval(&args[0], env, l, m)?)?;
    let text = as_str(eval(&args[1], env, l, m)?)?;
    FILES
        .with(|files| {
            let mut files = files.borrow_mut();
            let handle = files
                .files
                .get_mut(&fd)
                .ok_or_else(|| Error::Io(format!("invalid file descriptor {fd}")))?;
            match handle {
                FileHandle::File(file) => file.write_all(text.as_bytes()),
                FileHandle::Stdin => {
                    return Err(Error::Io("file descriptor 0 is not writable".into()))
                }
                FileHandle::Stdout => {
                    let mut stdout = io::stdout();
                    stdout
                        .write_all(text.as_bytes())
                        .and_then(|_| stdout.flush())
                }
                FileHandle::Stderr => {
                    let mut stderr = io::stderr();
                    stderr
                        .write_all(text.as_bytes())
                        .and_then(|_| stderr.flush())
                }
            }
            .map_err(|e| Error::Io(e.to_string()))
        })
        .map_err(Flow::Error)?;
    Ok(Value::Int(text.len() as i64))
}
fn format_value(args: &[Expr], env: &EnvRef, l: usize, m: usize) -> EResult {
    if args.is_empty() {
        return Err(Error::Arity("$ expects format string".into()).into());
    }
    let fmt = as_str(eval(&args[0], env, l, m)?)?;
    let vs = values(&args[1..], env, l, m)?;
    let mut out = String::new();
    let mut it = fmt.chars().peekable();
    let mut i = 0;
    while let Some(c) = it.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        let mut s = it
            .next()
            .ok_or_else(|| Flow::Error(Error::Format("trailing %".into())))?;
        let width = if s.is_ascii_digit() {
            let mut digits = s.to_string();
            while it.peek().is_some_and(|next| next.is_ascii_digit()) {
                digits.push(it.next().expect("peeked digit must be available"));
            }
            s = it
                .next()
                .ok_or_else(|| Flow::Error(Error::Format("trailing format width".into())))?;
            if s != 'b' {
                return Err(Error::Format(format!("unknown specifier %{digits}{s}")).into());
            }
            Some(
                digits
                    .parse::<usize>()
                    .map_err(|_| Flow::Error(Error::Format("invalid binary width".into())))?,
            )
        } else {
            None
        };
        if s == '%' {
            if width.is_some() {
                return Err(Error::Format("unknown specifier".into()).into());
            }
            out.push('%');
            continue;
        }
        let v = vs
            .get(i)
            .ok_or_else(|| Flow::Error(Error::Format("FormatArityError".into())))?;
        i += 1;
        match s {
            's' => out.push_str(&render(v)),
            'd' => {
                if let Value::Int(n) = v {
                    out.push_str(&n.to_string())
                } else {
                    return Err(Error::Format("FormatTypeError: %d expects integer".into()).into());
                }
            }
            'b' => {
                if let Value::Int(n) = v {
                    out.push_str(&format_binary(*n, width)?)
                } else {
                    return Err(Error::Format("FormatTypeError: %b expects integer".into()).into());
                }
            }
            'h' => {
                if let Value::Int(n) = v {
                    out.push_str(&format!("{n:x}"))
                } else {
                    return Err(Error::Format("FormatTypeError: %h expects integer".into()).into());
                }
            }
            'o' => {
                if let Value::Int(n) = v {
                    out.push_str(&format!("{n:o}"))
                } else {
                    return Err(Error::Format("FormatTypeError: %o expects integer".into()).into());
                }
            }
            'f' => match v {
                Value::Float(n) => out.push_str(&float_render(*n)),
                Value::Int(n) => out.push_str(&(*n as f64).to_string()),
                _ => return Err(Error::Format("FormatTypeError: %f expects number".into()).into()),
            },
            'j' => out.push_str(&json_render(v)?),
            't' => out.push_str(value_type(v)),
            'v' => out.push_str(&debug_render(v)),
            _ => return Err(Error::Format(format!("unknown specifier %{s}")).into()),
        }
    }
    if i != vs.len() {
        return Err(Error::Format("FormatArityError".into()).into());
    }
    Ok(Value::Str(out))
}
fn format_binary(value: i64, width: Option<usize>) -> Result<String, Error> {
    match width {
        None => Ok(format!("{value:b}")),
        Some(width @ (8 | 16 | 32 | 64)) => {
            let bits = if width == 64 {
                value as u64
            } else {
                (value as u64) & ((1u64 << width) - 1)
            };
            Ok(format!("{bits:0width$b}"))
        }
        Some(width) => Err(Error::Format(format!(
            "FormatTypeError: %{width}b supports widths 8, 16, 32, or 64"
        ))),
    }
}
fn value_type(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Int(_) => "int",
        Value::Float(_) => "float",
        Value::Str(_) => "string",
        Value::Array(_) => "array",
        Value::Struct(_) => "struct",
        Value::Function(_) => "function",
        Value::NativeFunction(_) => "function",
        Value::Ref(_) => "ref",
    }
}
fn float_render(value: f64) -> String {
    if value.is_nan() {
        "NaN".into()
    } else if value == f64::INFINITY {
        "Inf".into()
    } else if value == f64::NEG_INFINITY {
        "-Inf".into()
    } else {
        value.to_string()
    }
}
fn json_render(v: &Value) -> Result<String, Error> {
    match v {
        Value::Null => Ok("null".into()),
        Value::Bool(value) => Ok(value.to_string()),
        Value::Int(value) => Ok(value.to_string()),
        Value::Float(value) if value.is_finite() => Ok(value.to_string()),
        Value::Float(_) => Err(Error::Format(
            "FormatTypeError: %j cannot encode non-finite float".into(),
        )),
        Value::Str(value) => Ok(json_string(value)),
        Value::Ref(cell) => json_render(&cell.borrow()),
        Value::Array(values) => values
            .borrow()
            .iter()
            .map(|value| json_render(&value.borrow()))
            .collect::<Result<Vec<_>, _>>()
            .map(|values| format!("[{}]", values.join(","))),
        Value::Struct(fields) => fields
            .borrow()
            .iter()
            .map(|(key, value)| {
                Ok(format!(
                    "{}:{}",
                    json_string(key),
                    json_render(&value.borrow())?
                ))
            })
            .collect::<Result<Vec<_>, Error>>()
            .map(|fields| format!("{{{}}}", fields.join(","))),
        Value::Function(_) => Err(Error::Format(
            "FormatTypeError: %j cannot encode function".into(),
        )),
        Value::NativeFunction(_) => Err(Error::Format(
            "FormatTypeError: %j cannot encode function".into(),
        )),
    }
}
fn json_string(value: &str) -> String {
    let mut escaped = String::from("\"");
    for c in value.chars() {
        match c {
            '\"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            c if c <= '\u{1f}' => escaped.push_str(&format!("\\u{:04x}", c as u32)),
            c => escaped.push(c),
        }
    }
    escaped.push('\"');
    escaped
}
fn debug_render(v: &Value) -> String {
    match v {
        Value::Null => "Null".into(),
        Value::Bool(value) => format!("Bool({value})"),
        Value::Int(value) => format!("Int({value})"),
        Value::Float(value) => format!("Float({value:?})"),
        Value::Str(value) => format!("Str({value:?})"),
        Value::Ref(cell) => format!("Ref({})", debug_render(&cell.borrow())),
        Value::Array(values) => format!(
            "Array([{}])",
            values
                .borrow()
                .iter()
                .map(|value| debug_render(&value.borrow()))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Value::Struct(fields) => format!(
            "Struct({{{}}})",
            fields
                .borrow()
                .iter()
                .map(|(key, value)| format!("{key}: {}", debug_render(&value.borrow())))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Value::Function(function) => format!("Function({})", function.params.join(", ")),
        Value::NativeFunction(function) => format!("NativeFunction({})", function.name),
    }
}
fn numeric(v: Value) -> Result<(Option<i64>, f64), Flow> {
    match v {
        Value::Int(n) => Ok((Some(n), n as f64)),
        Value::Float(n) => Ok((None, n)),
        _ => Err(Error::Type("expected number".into()).into()),
    }
}
fn builtin(name: &str, vs: Vec<Value>) -> Result<Value, Error> {
    match name {
        "add" => fold_numeric(name, &vs, Value::Int(0), |x, y| x + y),
        "mul" => fold_numeric(name, &vs, Value::Int(1), |x, y| x * y),
        "sub" => {
            require_at_least(name, vs.len(), 1)?;
            if vs.len() == 1 {
                return negate_numeric(vs[0].clone());
            }
            fold_numeric(name, &vs[1..], vs[0].clone(), |x, y| x - y)
        }
        "div" => {
            require_at_least(name, vs.len(), 1)?;
            if vs.len() == 1 {
                let (_, value) = numeric(vs[0].clone()).map_err(flow_err)?;
                if value == 0.0 {
                    return Err(Error::Math("DivisionByZero".into()));
                }
                return Ok(Value::Float(1.0 / value));
            }
            fold_numeric(name, &vs[1..], vs[0].clone(), |x, y| x / y)
        }
        "mod" | "pow" => {
            require_exact(name, vs.len(), 2)?;
            binary_numeric(name, vs[0].clone(), vs[1].clone())
        }
        "eq" | "ne" => {
            let equal = vs.windows(2).all(|pair| equals(&pair[0], &pair[1]));
            let distinct = (0..vs.len())
                .all(|index| ((index + 1)..vs.len()).all(|other| !equals(&vs[index], &vs[other])));
            Ok(Value::Bool(if name == "eq" { equal } else { distinct }))
        }
        "lt" | "gt" | "le" | "ge" => {
            let ordered = vs.windows(2).try_fold(true, |_, pair| {
                let (_, left) = numeric(pair[0].clone()).map_err(flow_err)?;
                let (_, right) = numeric(pair[1].clone()).map_err(flow_err)?;
                Ok::<_, Error>(match name {
                    "lt" => left < right,
                    "gt" => left > right,
                    "le" => left <= right,
                    _ => left >= right,
                })
            })?;
            Ok(Value::Bool(ordered))
        }
        "bit-and" | "bit-or" | "bit-xor" => {
            let identity = match name {
                "bit-and" => -1,
                _ => 0,
            };
            let result = vs.iter().try_fold(identity, |acc, value| {
                let value = integer_value(value.clone())?;
                Ok::<_, Error>(match name {
                    "bit-and" => acc & value,
                    "bit-or" => acc | value,
                    _ => acc ^ value,
                })
            })?;
            Ok(Value::Int(result))
        }
        "bit-not" => {
            require_exact(name, vs.len(), 1)?;
            Ok(Value::Int(!integer_value(vs[0].clone())?))
        }
        "bit-shl" | "bit-shr" => {
            require_exact(name, vs.len(), 2)?;
            let x = integer_value(vs[0].clone())?;
            let y = integer_value(vs[1].clone())?;
            if !(0..64).contains(&y) {
                return Err(Error::Math("InvalidShiftCount".into()));
            }
            let shift = y as u32;
            Ok(Value::Int(if name == "bit-shl" {
                let shifted = x
                    .checked_shl(shift)
                    .ok_or_else(|| Error::Math("IntegerOverflow".into()))?;
                if shifted.checked_shr(shift) != Some(x) {
                    return Err(Error::Math("IntegerOverflow".into()));
                }
                shifted
            } else {
                x.checked_shr(shift)
                    .ok_or_else(|| Error::Math("IntegerOverflow".into()))?
            }))
        }
        _ => Err(Error::Name(format!("unknown builtin {name}"))),
    }
}

fn require_exact(name: &str, actual: usize, expected: usize) -> Result<(), Error> {
    if actual == expected {
        Ok(())
    } else {
        Err(Error::Arity(format!(
            "{name} expects {expected} arguments, got {actual}"
        )))
    }
}

fn require_at_least(name: &str, actual: usize, minimum: usize) -> Result<(), Error> {
    if actual >= minimum {
        Ok(())
    } else {
        Err(Error::Arity(format!(
            "{name} expects at least {minimum} argument{}, got {actual}",
            if minimum == 1 { "" } else { "s" }
        )))
    }
}

fn negate_numeric(value: Value) -> Result<Value, Error> {
    match value {
        Value::Int(value) => value
            .checked_neg()
            .map(Value::Int)
            .ok_or_else(|| Error::Math("IntegerOverflow".into())),
        Value::Float(value) => Ok(Value::Float(-value)),
        _ => Err(Error::Type("expected number".into())),
    }
}

fn fold_numeric(
    name: &str,
    values: &[Value],
    initial: Value,
    operation: fn(f64, f64) -> f64,
) -> Result<Value, Error> {
    let mut result = initial;
    for value in values {
        result = binary_numeric_with_operation(name, result, value.clone(), operation)?;
    }
    Ok(result)
}

fn binary_numeric(name: &str, left: Value, right: Value) -> Result<Value, Error> {
    binary_numeric_with_operation(
        name,
        left,
        right,
        match name {
            "mod" => |x, y| x % y,
            "pow" => |x, y| x.powf(y),
            _ => unreachable!(),
        },
    )
}

fn binary_numeric_with_operation(
    name: &str,
    left: Value,
    right: Value,
    operation: fn(f64, f64) -> f64,
) -> Result<Value, Error> {
    let (left_int, left_float) = numeric(left).map_err(flow_err)?;
    let (right_int, right_float) = numeric(right).map_err(flow_err)?;
    if name == "div" && right_float == 0.0 {
        return Err(Error::Math("DivisionByZero".into()));
    }
    if name == "mod" {
        let (Some(left), Some(right)) = (left_int, right_int) else {
            return Err(Error::Type("mod requires integers".into()));
        };
        if right == 0 {
            return Err(Error::Math("DivisionByZero".into()));
        }
        return left
            .checked_rem(right)
            .map(Value::Int)
            .ok_or_else(|| Error::Math("IntegerOverflow".into()));
    }
    if name == "pow" && left_int.is_some() && right_int.is_some() {
        return right_int
            .and_then(|right| u32::try_from(right).ok())
            .and_then(|right| left_int.unwrap().checked_pow(right))
            .map(Value::Int)
            .ok_or_else(|| Error::Math("IntegerOverflow".into()));
    }
    match (left_int, right_int) {
        (Some(left), Some(right)) if name == "div" => {
            return left
                .checked_div(right)
                .map(Value::Int)
                .ok_or_else(|| Error::Math("IntegerOverflow".into()));
        }
        _ => {}
    }
    match (left_int, right_int) {
        (Some(left), Some(right)) if matches!(name, "add" | "sub" | "mul") => {
            let result = match name {
                "add" => left.checked_add(right),
                "sub" => left.checked_sub(right),
                "mul" => left.checked_mul(right),
                _ => unreachable!(),
            };
            return result
                .map(Value::Int)
                .ok_or_else(|| Error::Math("IntegerOverflow".into()));
        }
        _ => {}
    }
    Ok(Value::Float(operation(left_float, right_float)))
}
fn integer_value(v: Value) -> Result<i64, Error> {
    if let Value::Int(n) = v {
        Ok(n)
    } else {
        Err(Error::Type("bitwise operations require integers".into()))
    }
}
fn flow_err(f: Flow) -> Error {
    match f {
        Flow::Error(e) => e,
        _ => Error::Type("unexpected control flow".into()),
    }
}
fn equals(a: &Value, b: &Value) -> bool {
    let a = if let Value::Ref(c) = a {
        let x = c.borrow();
        return equals(&x, b);
    } else {
        a
    };
    let b = if let Value::Ref(c) = b {
        let x = c.borrow();
        return equals(a, &x);
    } else {
        b
    };
    match (a, b) {
        (Value::Null, Value::Null) => true,
        (Value::Bool(x), Value::Bool(y)) => x == y,
        (Value::Int(x), Value::Int(y)) => x == y,
        (Value::Float(x), Value::Float(y)) => x == y,
        (Value::Int(x), Value::Float(y)) | (Value::Float(y), Value::Int(x)) => (*x as f64) == *y,
        (Value::Str(x), Value::Str(y)) => x == y,
        (Value::Function(x), Value::Function(y)) => Rc::ptr_eq(x, y),
        (Value::NativeFunction(x), Value::NativeFunction(y)) => Rc::ptr_eq(x, y),
        (Value::Array(x), Value::Array(y)) => {
            let x = x.borrow();
            let y = y.borrow();
            x.len() == y.len()
                && x.iter()
                    .zip(y.iter())
                    .all(|(a, b)| equals(&a.borrow(), &b.borrow()))
        }
        (Value::Struct(x), Value::Struct(y)) => {
            let x = x.borrow();
            let y = y.borrow();
            x.len() == y.len()
                && x.iter().all(|(k, v)| {
                    y.iter()
                        .find(|(q, _)| q == k)
                        .is_some_and(|(_, w)| equals(&v.borrow(), &w.borrow()))
                })
        }
        _ => false,
    }
}
fn diagnostic(error: &Error, source: &str, file: &str, span: Option<Span>) -> String {
    let span = span.unwrap_or(Span { start: 0, end: 0 });
    let offset = span.start.min(source.len());
    let line = source[..offset].bytes().filter(|b| *b == b'\n').count() + 1;
    let line_start = source[..offset].rfind('\n').map_or(0, |p| p + 1);
    let line_end = source[offset..]
        .find('\n')
        .map_or(source.len(), |p| offset + p);
    let column = source[line_start..offset].chars().count() + 1;
    let excerpt = &source[line_start..line_end];
    let caret = format!("{}^", " ".repeat(column.saturating_sub(1)));
    let mut out = format!("{file}:{line}:{column}: {error}\n{excerpt}\n{caret}");
    let trace = LAST_TRACE.with(|last| last.borrow().clone());
    if !trace.is_empty() {
        out.push_str("\ncall trace:");
        for (name, frame) in trace.iter().rev() {
            let line = source[..frame.start.min(source.len())]
                .bytes()
                .filter(|b| *b == b'\n')
                .count()
                + 1;
            let line_start = source[..frame.start.min(source.len())]
                .rfind('\n')
                .map_or(0, |p| p + 1);
            let column = source[line_start..frame.start.min(source.len())]
                .chars()
                .count()
                + 1;
            out.push_str(&format!("\n  {name} at {file}:{line}:{column}"));
        }
    }
    out
}
fn main() {
    if let Err(error) = install_sigint_handler() {
        let _ = writeln!(io::stderr(), "{error}");
        std::process::exit(1);
    }
    let args: Vec<String> = env::args().collect();
    let file = args.get(1).map_or("<stdin>", String::as_str);
    let src = if args.len() > 1 {
        fs::read_to_string(&args[1]).map_err(|e| Error::Io(e.to_string()))
    } else {
        let mut s = String::new();
        io::stdin()
            .read_to_string(&mut s)
            .map(|_| s)
            .map_err(|e| Error::Io(e.to_string()))
    };
    let result = (|| -> Result<(), (Error, bool)> {
        let source = src.clone().map_err(|e| (e, false))?;
        let ts = lex(&source).map_err(|e| (e, true))?;
        let p = Parser { ts, i: 0 }.program().map_err(|e| (e, true))?;
        let e = new_env(None);
        for x in p {
            eval(&x, &e, 0, 0).map_err(|e| (flow_err(e), false))?;
        }
        Ok(())
    })();
    if let Err((e, parse)) = result {
        let source = match &src {
            Ok(source) => source,
            Err(_) => "",
        };
        let span = if parse {
            PARSE_ERROR_SPAN.with(|span| *span.borrow())
        } else {
            LAST_ERROR_SPAN.with(|span| *span.borrow())
        };
        let _ = writeln!(io::stderr(), "{}", diagnostic(&e, source, file, span));
        std::process::exit(1)
    }
}

#[cfg(test)]
mod tests {
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

        let (url, handle) =
            http_fixture("HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n", None);
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
    fn closure_recursion_and_integer_division_work() {
        let value = run(
            "(let fact (fn (n) (if (eq n 0) 1 (mul n (fact (sub n 1)))))) (add (fact 5) (div 7 2))",
        )
        .unwrap();
        assert!(matches!(value, Value::Int(123)));
    }

    #[test]
    fn variadic_builtins_follow_their_declared_arities() {
        let value = run("(expect (add) 0)
             (expect (add 1 2 3 4) 10)
             (expect (mul) 1)
             (expect (mul 2 3 4) 24)
             (expect (sub 10) -10)
             (expect (sub 10 3 2) 5)
             (expect (div 20) 0.05)
             (expect (div 20 2 2) 5)
             (expect (eq) t)
             (expect (eq 1) t)
             (expect (ne) t)
             (expect (ne 1) t)
             (expect (ne 1 2 1) f)
             (expect (ne 1 2 3) t)
             (expect (lt 1 2 3) t)
             (expect (ge 3 2 2) t)
             (expect (bit-and) -1)
             (expect (bit-and 15 7 3) 3)
             (expect (bit-or) 0)
             (expect (bit-or 1 2 4) 7)
             (expect (bit-xor 7 3 1) 5)")
        .unwrap();
        assert!(matches!(value, Value::Bool(true)));
    }

    #[test]
    fn fixed_builtin_and_user_function_arities_remain_strict() {
        for source in ["(mod 4)", "(mod 4 2 1)", "(pow 2)", "(bit-not 1 2)"] {
            assert!(matches!(run(source), Err(Error::Arity(_))));
        }
        assert!(matches!(
            run("(let sum (fn (x y) (add x y))) (sum 1)"),
            Err(Error::Arity(_))
        ));
        assert!(matches!(
            run("(let sum (fn (x y) (add x y))) (sum 1 2 3)"),
            Err(Error::Arity(_))
        ));
    }

    #[test]
    fn variadic_folds_preserve_numeric_errors() {
        assert!(matches!(
            run("(add 1 2 9223372036854775807)"),
            Err(Error::Math(message)) if message == "IntegerOverflow"
        ));
        assert!(matches!(
            run("(div 20 2 0)"),
            Err(Error::Math(message)) if message == "DivisionByZero"
        ));
        assert!(matches!(
            run("(bit-or 1 2.0)"),
            Err(Error::Type(message)) if message == "bitwise operations require integers"
        ));
    }

    #[test]
    fn integer_builtins_turn_operator_overflows_into_lisp_errors() {
        for source in [
            "(div -9223372036854775808 -1)",
            "(mod -9223372036854775808 -1)",
            "(bit-shl 4611686018427387904 2)",
        ] {
            assert!(matches!(
                run(source),
                Err(Error::Math(message)) if message == "IntegerOverflow"
            ));
        }
    }

    #[test]
    fn references_mutate_the_original_location() {
        let value =
            run("(let a [1 2]) (let setzero (fn (x) (set x[1] 0))) (setzero ^a) a[1]").unwrap();
        assert!(matches!(value, Value::Int(0)));
    }

    #[test]
    fn cyclic_references_are_reported_as_lisp_errors() {
        for source in [
            "(let x _) (set x ^x)",
            "(let x _) (let y _) (set x ^y) (set y ^x)",
            "(let x _) (set x [^x])",
            "(let x _) (set x {self: ^x})",
        ] {
            assert!(matches!(
                run(source),
                Err(Error::Type(message)) if message == "cyclic reference"
            ));
        }
    }

    #[test]
    fn arrays_support_bracket_indexing_and_inclusive_slices() {
        let value = run(r#"(let value [10 20 30 40 50])
               (expect value[1] 10)
               (expect value[-1] 50)
               (expect value[2..4] [20 30 40])
               (expect value[..2] [10 20])
               (expect value[4..] [40 50])
               (expect value[[-1 1]] [50 10])"#)
        .unwrap();
        assert!(matches!(value, Value::Bool(true)));

        let value = run(r#"(let value {score:[23 [1 2 3] 66]})
               (expect value.score[2][3] 3)
               (expect value.score[2][[1 3]] [1 3])"#)
        .unwrap();
        assert!(matches!(value, Value::Bool(true)));
    }

    #[test]
    fn fully_open_slice_of_empty_array_is_empty() {
        let value = run("(let a []) (let b a[..]) b").unwrap();
        assert!(matches!(value, Value::Array(array) if array.borrow().is_empty()));
    }

    #[test]
    fn array_dot_access_is_rejected() {
        assert!(matches!(
            run("(let value [1]) value.1"),
            Err(Error::Parse(message)) if message.contains("struct field")
        ));
    }

    #[test]
    fn strings_support_grapheme_indexing_and_string_slices() {
        let value = run(r#"(let s "😀abc")
               (expect s[1] "😀")
               (expect s[2] "a")
               (expect s[-1] "c")
               (expect s[1..2] "😀a")
               (expect s[..2] "😀a")
               (expect s[3..] "bc")
               (expect s[-2..-1] "bc")
               (expect s[[1 3]] ["😀" "b"])
               (expect "é👩‍🚀"[1] "é")
               (expect ""[..] "")"#)
        .unwrap();
        assert!(matches!(value, Value::Bool(true)));
    }

    #[test]
    fn string_index_errors_match_collection_rules() {
        assert!(matches!(
            run(r#""abc"[0]"#),
            Err(Error::Type(message)) if message.contains("1-based")
        ));
        assert!(matches!(
            run(r#""abc"[4]"#),
            Err(Error::Name(message)) if message.contains("out of bounds")
        ));
        assert!(matches!(
            run(r#""abc"[1.0]"#),
            Err(Error::Type(message)) if message.contains("integer")
        ));
    }

    #[test]
    fn duplicate_struct_key_is_an_error() {
        assert!(matches!(run("{x: 1 x: 2}"), Err(Error::DuplicateKey(_))));
        assert!(matches!(run("{_: 1 _: 2}"), Err(Error::DuplicateKey(key)) if key == "_"));
    }

    #[test]
    fn duplicate_struct_key_diagnostic_points_at_duplicate_key() {
        let source = "(let value {\n  x: 1\n  x: 2\n})";
        let error = match run(source) {
            Err(error) => error,
            Ok(_) => panic!("expected duplicate key error"),
        };
        let span = LAST_ERROR_SPAN.with(|span| *span.borrow());
        let rendered = diagnostic(&error, source, "sample.lisp", span);
        assert!(rendered.starts_with("sample.lisp:3:3:"));
        assert!(rendered.contains("  x: 2"));
    }

    #[test]
    fn struct_keys_use_identifier_syntax() {
        let value = run(r#"(let value {name:"Yvan" visits:2}) value.name"#).unwrap();
        assert!(matches!(value, Value::Str(name) if name == "Yvan"));
        assert!(matches!(
            run(r#"{"name":"Yvan"}"#),
            Err(Error::Parse(message)) if message == "struct key must be an identifier"
        ));
    }

    #[test]
    fn structs_unwrap_the_underscore_field_only_in_operator_position() {
        let source = r#"
            (let module {
              open: {
                _: (fn () 42)
                spec: {documentation: "open" arity: 0 type: _ return: []}
              }
            })
            (module.open)
        "#;
        let value = run(source).unwrap();
        assert!(matches!(value, Value::Int(42)));

        let value = run(r#"
            (let module {open: {_: (fn () 42) spec: {documentation:"open" arity: 0 type: _ return: []}}})
            ($ "%s" module.open)
        "#)
        .unwrap();
        assert!(matches!(
            value,
            Value::Str(text) if text == r#"{_:<fn> spec:{documentation:"open" arity:0 type: return:[]}}"#
        ));
    }

    #[test]
    fn native_str_module_is_loaded_and_introspectable() {
        let value = run(r#"(use "str")
               (expect (str.upper "hello") "HELLO")
               (expect (str.lower "HELLO") "hello")
               (expect str.upper.spec.documentation "Convert a string to uppercase.")
               (expect str.upper.spec.arity 1)
               (expect str.upper.spec.type ["string"])
               (expect str.upper.spec.return ["string"])
               ($ "%s" str.upper)"#)
        .unwrap();
        assert!(matches!(
            value,
            Value::Str(text) if text == r#"{_:<native fn> spec:{documentation:"Convert a string to uppercase." arity:1 type:["string"] return:["string"]}}"#
        ));
    }

    #[test]
    fn use_accepts_string_names_and_rejects_unknown_modules() {
        let value = run(r#"(use "str") (str.upper "hello")"#).unwrap();
        assert!(matches!(value, Value::Str(text) if text == "HELLO"));
        let value = run(r#"(let module-name "str") (use module-name) (str.lower "ABC")"#).unwrap();
        assert!(matches!(value, Value::Str(text) if text == "abc"));
        let value = run(r#"(let str "str") (use str) (str.upper "hello")"#).unwrap();
        assert!(matches!(value, Value::Str(text) if text == "HELLO"));
        assert!(matches!(
            run("(use str)"),
            Err(Error::Name(message)) if message == "str"
        ));
        assert!(matches!(
            run("(use \"missing\")"),
            Err(Error::Name(message)) if message == "unknown native module missing"
        ));
    }

    #[test]
    fn lisp_modules_are_loaded_from_named_bindings_and_validate_descriptors() {
        let value = run(r#"(use (let mymodule {
                 add: {
                   _: (fn (x y) x)
                   spec: {
                     documentation: "Return the first string."
                     arity: 2
                     type: ["string" "string"]
                     return: []
                   }
                 }
               }))
               (expect mymodule.add.spec.documentation "Return the first string.")
               (expect mymodule.add.spec.arity 2)
               (expect mymodule.add.spec.type ["string" "string"])
               (mymodule.add "a" "b")"#)
        .unwrap();
        assert!(matches!(value, Value::Str(text) if text == "a"));

        assert!(matches!(
            run(r#"(use (let mymodule {
                add: {_: (fn (x y) x) spec: {documentation: "add" arity: 2 type: ["string" "string"] return: []}}
            })) (mymodule.add "a")"#),
            Err(Error::Arity(message)) if message.contains("expects 2 arguments")
        ));
        assert!(matches!(
            run(r#"(use (let mymodule {
                add: {_: (fn (x y) x) spec: {documentation: "add" arity: 2 type: ["string" "string"] return: []}}
            })) (mymodule.add 10 "b")"#),
            Err(Error::Type(message)) if message.contains("argument 1 expects string")
        ));
    }

    #[test]
    fn native_descriptors_use_the_same_validation_path() {
        assert!(matches!(
            run("(use \"str\") (str.upper)"),
            Err(Error::Arity(message)) if message.contains("expects 1 arguments")
        ));
        assert!(matches!(
            run("(use \"str\") (str.upper 10)"),
            Err(Error::Type(message)) if message.contains("argument 1 expects string")
        ));
    }

    #[test]
    fn module_argument_validation_reports_the_call_site_after_nested_evaluation() {
        let source = r#"
(use (let module {
    open: {
        _: (fn (x y) (io/write 1 ($ "Called open with args %s and %s\n" x y)))
        spec: {
            documentation: "open"
            arity: 2
            type: ["string" "string"]
            return: ["string"]
        }
    }
}))
(use "str")
(str.lower (module.open "args1" "args2"))
"#;
        let error = match run(source) {
            Err(error) => error,
            Ok(_) => panic!("expected a module argument type error"),
        };
        assert!(matches!(
            &error,
            Error::Type(message) if message.contains("argument 1 expects string, got int")
        ));
        let span = LAST_ERROR_SPAN.with(|span| *span.borrow());
        let rendered = diagnostic(&error, source, "sample.lisp", span);
        assert!(rendered.starts_with("sample.lisp:14:"));
        assert!(rendered.contains("(str.lower (module.open"));
    }

    #[test]
    fn descriptor_registration_requires_a_complete_callable_spec() {
        let valid = r#"(use (let module {
            foo: {_: (fn (x) x) spec: {
                documentation: "foo" arity: 1 type: ["string"] return: []
            }}
        }))"#;
        assert!(run(valid).is_ok());

        let valid_return = r#"(use (let module {
            foo: {_: (fn (x) x) spec: {
                documentation: "foo" arity: 1 type: ["string"] return: ["string"]
            }}
        }))"#;
        assert!(run(valid_return).is_ok());

        for spec in [
            r#"{arity: 1 type: ["string"] return: []}"#,
            r#"{documentation: "foo" type: ["string"] return: []}"#,
            r#"{documentation: "foo" arity: 1 return: []}"#,
            r#"{documentation: "foo" arity: 1 type: ["string"]}"#,
            r#"{documentation: 1 arity: 1 type: ["string"] return: []}"#,
            r#"{documentation: "foo" arity: "1" type: ["string"] return: []}"#,
            r#"{documentation: "foo" arity: 1 type: "string" return: []}"#,
            r#"{documentation: "foo" arity: 1 type: ["string"] return: 1}"#,
        ] {
            let source = format!("(use (let module {{foo: {{_: (fn (x) x) spec: {spec}}}}}))");
            assert!(matches!(run(&source), Err(Error::Type(_))), "{source}");
        }

        assert!(matches!(
            run(r#"(use (let module {
                foo: {spec: {documentation: "foo" arity: 1 type: ["string"] return: []}}
            }))"#),
            Err(Error::Type(message)) if message.contains("contain _")
        ));
        assert!(matches!(
            run(r#"(use (let module {
                foo: {_: 1 spec: {documentation: "foo" arity: 1 type: ["string"] return: []}}
            }))"#),
            Err(Error::Type(message)) if message.contains("_ must be callable")
        ));
        assert!(run(r#"(use (let module {
            foo: {_: (fn () 1) spec: {
                documentation: "foo" arity: 0 type: _ return: _
            }}
        })) (module.foo)"#)
        .is_ok());
        assert!(matches!(
            run(r#"(use (let module {
                foo: {_: (fn () 1) spec: {
                    documentation: "foo" arity: 0 type: [] return: _
                }}
            }))"#),
            Err(Error::Type(message)) if message.contains("spec.type")
        ));
    }

    #[test]
    fn regex_returns_and_binds_captures() {
        let value = run(r#"(let string "Hello the world")
               (~ "^Hello(.*)$" string match)
               (expect match ["Hello the world" " the world"])"#)
        .unwrap();
        assert!(matches!(value, Value::Bool(true)));
    }

    #[test]
    fn regex_preserves_multiple_and_optional_captures() {
        let value = run(r#"(let match _)
               (~ "^(a)(b)?(c)$" "ac" match)
               (expect match ["ac" "a" _ "c"])"#)
        .unwrap();
        assert!(matches!(value, Value::Bool(true)));
    }

    #[test]
    fn regex_failure_does_not_overwrite_binding() {
        let value = run(r#"(let match ["unchanged"])
               (expect (~ "^a+$" "bbb" match) f)
               (expect match ["unchanged"])"#)
        .unwrap();
        assert!(matches!(value, Value::Bool(true)));
    }

    #[test]
    fn regex_reports_invalid_patterns_and_requires_strings() {
        assert!(matches!(
            run(r#"(~ "(" "text" match)"#),
            Err(Error::Regex(message)) if message.contains("invalid regex")
        ));
        assert!(matches!(
            run(r#"(~ "^a$" 1 match)"#),
            Err(Error::Type(message)) if message == "expected string"
        ));
        assert!(matches!(
            run(r#"(~ "^a$" "a" [match])"#),
            Err(Error::Type(message)) if message.contains("binding")
        ));
    }

    #[test]
    fn regex_binding_follows_nested_scope_rules() {
        let value = run(r#"(let match ["outer"])
               (let result
                 ((~ "^(a)$" "a" match)
                  match))
               (expect result ["a" "a"])
               (expect match ["a" "a"])"#)
        .unwrap();
        assert!(matches!(value, Value::Bool(true)));
    }

    #[test]
    fn formatting_supports_integer_bases_json_and_debug_output() {
        let value = run("($ \"%b %h %o\" 10 255 8)").unwrap();
        assert!(matches!(value, Value::Str(text) if text == "1010 ff 10"));

        let value = run("($ \"%8b %16b %32b %64b\" 5 5 5 5)").unwrap();
        assert!(
            matches!(value, Value::Str(text) if text == "00000101 0000000000000101 00000000000000000000000000000101 0000000000000000000000000000000000000000000000000000000000000101")
        );

        let value = run("($ \"%t %t\" 1 [1])").unwrap();
        assert!(matches!(value, Value::Str(text) if text == "int array"));

        let value =
            run(r#"($ "%s" {name:"Yvan" contact:{gsm:"0102030405"} score:["ok" 2]})"#).unwrap();
        assert!(
            matches!(value, Value::Str(text) if text == r#"{name:"Yvan" contact:{gsm:"0102030405"} score:["ok" 2]}"#)
        );

        let value = run("($ \"%j\" {name: \"Ada\" values: [1 t _]})").unwrap();
        assert!(
            matches!(value, Value::Str(text) if text == r#"{"name":"Ada","values":[1,true,null]}"#)
        );

        let value = run("($ \"%v\" {name: \"Ada\" values: [1 t _]})").unwrap();
        assert!(
            matches!(value, Value::Str(text) if text == r#"Struct({name: Str("Ada"), values: Array([Int(1), Bool(true), Null])})"#)
        );

        let value = run("($ \"%v\" [1 \"x\"])").unwrap();
        assert!(matches!(value, Value::Str(text) if text == r#"Array([Int(1), Str("x")])"#));
    }

    #[test]
    fn expect_checks_values_and_reports_comments() {
        let value = run("(expect (eq 1 1) t \"integers compare equally\")").unwrap();
        assert!(matches!(value, Value::Bool(true)));

        let value = run("(expect (add 1 1) (sub 3 1))").unwrap();
        assert!(matches!(value, Value::Bool(true)));

        assert!(matches!(
            run("(expect (add 1 1) 3 \"addition regression\")"),
            Err(Error::Expect(message))
                if message.contains("addition regression")
                    && message.contains("expected Int(3), got Int(2)")
        ));
    }

    #[test]
    fn decimal_floats_do_not_conflict_with_field_access() {
        let value = run("(expect (add 1 0) 1.0 \"integer float test\")").unwrap();
        assert!(matches!(value, Value::Bool(true)));

        let value = run("(let values [10]) values[1]").unwrap();
        assert!(matches!(value, Value::Int(10)));
    }

    #[test]
    fn non_finite_float_literals_follow_numeric_semantics() {
        let value = run(r#"
                (expect ($ "%t:%s" NaN NaN) "float:NaN")
                (expect ($ "%t:%s" Inf Inf) "float:Inf")
                (expect ($ "%t:%s" -Inf -Inf) "float:-Inf")
                (expect (eq NaN NaN) f)
                (expect (eq Inf Inf) t)
                (expect (eq -Inf -Inf) t)
                (expect (eq 0.0 -0.0) t)
                (expect (eq 5 5.0) t)
                (expect (lt 1.0 Inf) t)
                (expect (lt -Inf 1.0) t)
                (expect (lt NaN 1.0) f)
                (expect (gt NaN 1.0) f)
            "#)
        .unwrap();
        assert!(matches!(value, Value::Bool(true)));
    }

    #[test]
    fn integer_bitwise_operations_work() {
        let value = run("(bit-or (bit-and 0b110 0b101) (bit-xor 0b110 0b101))").unwrap();
        assert!(matches!(value, Value::Int(0b111)));

        let value = run("(bit-shr (bit-shl 1 4) 2)").unwrap();
        assert!(matches!(value, Value::Int(4)));

        let value = run("(bit-not 0b101)").unwrap();
        assert!(matches!(value, Value::Int(-6)));
    }

    #[test]
    fn bitwise_operations_require_integers_and_valid_shift_counts() {
        for source in [
            "(bit-and 1.0 2)",
            "(bit-or 1 \"2\")",
            "(bit-xor 1 t)",
            "(bit-not _)",
            "(bit-shl 1 2.0)",
            "(bit-shr 4 f)",
        ] {
            assert!(matches!(
                run(source),
                Err(Error::Type(message)) if message == "bitwise operations require integers"
            ));
        }
        assert!(matches!(
            run("(bit-shl 1 64)"),
            Err(Error::Math(message)) if message == "InvalidShiftCount"
        ));
    }

    #[test]
    fn file_descriptor_io_reads_lines_and_tracks_close_status() {
        let path = env::temp_dir().join(format!("small_lisp_io_{}.txt", std::process::id()));
        let source = format!(
            r#"
                (let #fd (io/open "w" "{}"))
                (io/write #fd "first\nsecond")
                (io/close #fd)
                (let #fd (io/open "r" "{}"))
                (let first (io/read #fd))
                (io/close #fd)
                first
            "#,
            path.display(),
            path.display(),
        );
        let value = run(&source).unwrap();
        assert!(matches!(value, Value::Str(line) if line == "first"));

        let close_status = run(&format!(
            r#"
                (let #fd (io/open "r" "{}"))
                (io/close #fd)
                (io/close #fd)
            "#,
            path.display(),
        ))
        .unwrap();
        assert!(matches!(close_status, Value::Bool(false)));
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn standard_file_descriptors_are_available() {
        FILES.with(|files| {
            let files = files.borrow();
            assert!(matches!(files.files.get(&0), Some(FileHandle::Stdin)));
            assert!(matches!(files.files.get(&1), Some(FileHandle::Stdout)));
            assert!(matches!(files.files.get(&2), Some(FileHandle::Stderr)));
        });
        assert!(matches!(run("(io/write 1 \"\")"), Ok(Value::Int(0))));
        assert!(matches!(run("(io/write 2 \"\")"), Ok(Value::Int(0))));
    }

    #[test]
    fn diagnostics_include_source_location_and_excerpt() {
        let source = "(let x 1)\n(div x 0)";
        let error = match run(source) {
            Err(error) => error,
            Ok(_) => panic!("expected runtime error"),
        };
        let span = LAST_ERROR_SPAN.with(|span| *span.borrow());
        let rendered = diagnostic(&error, source, "sample.lisp", span);
        assert!(rendered.starts_with("sample.lisp:2:"));
        assert!(rendered.contains("(div x 0)"));
        assert!(rendered.contains("^"));
    }

    #[test]
    fn diagnostics_use_byte_offsets_after_unicode_source() {
        let source = "; section — variadic functions\n(let value 1)\n(div value 0)";
        let error = match run(source) {
            Err(error) => error,
            Ok(_) => panic!("expected division by zero"),
        };
        let span = LAST_ERROR_SPAN.with(|span| *span.borrow());
        let rendered = diagnostic(&error, source, "unicode.lisp", span);
        assert!(rendered.starts_with("unicode.lisp:3:"));
        assert!(rendered.contains("(div value 0)"));
        assert!(!rendered.contains("; section"));
    }

    #[test]
    fn nested_user_function_errors_include_call_trace() {
        let error = match run("(let inner (fn () (div 1 0))) (let outer (fn () (inner))) (outer)") {
            Err(error) => error,
            Ok(_) => panic!("expected runtime error"),
        };
        let span = LAST_ERROR_SPAN.with(|span| *span.borrow());
        let rendered = diagnostic(
            &error,
            "(let inner (fn () (div 1 0))) (let outer (fn () (inner))) (outer)",
            "x",
            span,
        );
        assert!(rendered.contains("call trace:"));
        assert!(rendered.contains("outer"));
        assert!(rendered.contains("inner"));
        assert!(rendered.contains("at x:1:"));
    }

    #[test]
    fn parse_diagnostics_point_at_the_failing_token() {
        let source = "(let x 1";
        let error = Parser {
            ts: lex(source).unwrap(),
            i: 0,
        }
        .program()
        .unwrap_err();
        let span = PARSE_ERROR_SPAN.with(|span| *span.borrow());
        let rendered = diagnostic(&error, source, "sample.lisp", span);
        assert!(rendered.starts_with("sample.lisp:1:"));
        assert!(rendered.contains("^"));
    }
}
