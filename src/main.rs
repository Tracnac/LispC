use regex::Regex;
use std::{
    cell::RefCell,
    collections::HashSet,
    env, fmt,
    fs::{self},
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
    DuplicateBinding(String),
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
            DuplicateBinding(s) => write!(f, "DuplicateBindingError: {s}"),
            Expect(s) => write!(f, "ExpectationError: {s}"),
            Match => write!(f, "MatchError: no predicate matched"),
            ContinueOutsideLoop => write!(f, "ContinueOutsideLoop"),
            BreakOutside => write!(f, "BreakOutsideLoop"),
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

pub(crate) fn check_interrupted() -> Result<(), Error> {
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
        let magnitude = u64::from_str_radix(d, base)
            .map_err(|_| Error::Parse(format!("invalid number `{s}`")))?;
        // -2^63 is representable even though its magnitude overflows i64,
        // mirroring the decimal `-9223372036854775808`.
        let value = if sign < 0 && magnitude == 1u64 << 63 {
            i64::MIN
        } else if sign < 0 {
            i64::try_from(magnitude)
                .ok()
                .and_then(|m| m.checked_mul(sign))
                .ok_or_else(|| Error::Parse("integer out of range".into()))?
        } else {
            i64::try_from(magnitude)
                .ok()
                .ok_or_else(|| Error::Parse("integer out of range".into()))?
        };
        return Ok(Some(TokKind::Int(value)));
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
                    "+NaN" => ExprKind::Lit(Literal::Float(f64::NAN)),
                    "Inf" => ExprKind::Lit(Literal::Float(f64::INFINITY)),
                    "+Inf" => ExprKind::Lit(Literal::Float(f64::INFINITY)),
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
    fn visit(cell: &Cell, target: &Cell, visited: &mut HashSet<usize>) -> bool {
        let identity = Rc::as_ptr(cell) as usize;
        if !visited.insert(identity) {
            return false;
        }
        if Rc::ptr_eq(cell, target) {
            return true;
        }
        match &*cell.borrow() {
            Value::Ref(next) => visit(next, target, visited),
            Value::Array(values) => {
                let cells: Vec<Cell> = values.borrow().iter().cloned().collect();
                cells.iter().any(|cell| visit(cell, target, visited))
            }
            Value::Struct(fields) => {
                let cells: Vec<Cell> = fields.borrow().iter().map(|(_, c)| c.clone()).collect();
                cells.iter().any(|cell| visit(cell, target, visited))
            }
            _ => false,
        }
    }
    let mut visited = HashSet::new();
    match value {
        Value::Ref(cell) => visit(cell, target, &mut visited),
        Value::Array(values) => {
            let cells: Vec<Cell> = values.borrow().iter().cloned().collect();
            cells.iter().any(|cell| visit(cell, target, &mut visited))
        }
        Value::Struct(fields) => {
            let cells: Vec<Cell> = fields.borrow().iter().map(|(_, c)| c.clone()).collect();
            cells.iter().any(|cell| visit(cell, target, &mut visited))
        }
        _ => false,
    }
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
    if env.borrow().values.iter().any(|(k, _)| k == &name) {
        return Err(Error::DuplicateBinding(name).into());
    }
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
                if l == 0 {
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
                        Ok(v) if truth(&v) => return eval(&p[1], env, l, m + 1),
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
        let Value::Struct(descriptor) = cell.borrow().clone() else {
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
    let callable = fields
        .borrow()
        .iter()
        .find(|(key, _)| key == "_")
        .map(|(_, cell)| cell.borrow().clone());
    match callable {
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
    matches!(value, Value::Function(_) | Value::NativeFunction(_))
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
            if s != 'b' && s != 'h' {
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
                    out.push_str(&format_hex(*n, width)?)
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
                Value::Int(n) => out.push_str(&n.to_string()),
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
fn format_hex(value: i64, width: Option<usize>) -> Result<String, Error> {
    match width {
        None => Ok(format!("{value:x}")),
        Some(width @ (8 | 16 | 32 | 64)) => {
            let digits = if width == 64 {
                value as u64
            } else {
                (value as u64) & ((1u64 << width) - 1)
            };
            Ok(format!("{digits:0width$x}", width = width / 4))
        }
        Some(width) => Err(Error::Format(format!(
            "FormatTypeError: %{width}h supports widths 8, 16, 32, or 64"
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
                let (left_int, left_float) = numeric(pair[0].clone()).map_err(flow_err)?;
                let (right_int, right_float) = numeric(pair[1].clone()).map_err(flow_err)?;
                let result = match (left_int, right_int) {
                    (Some(left), Some(right)) => match name {
                        "lt" => left < right,
                        "gt" => left > right,
                        "le" => left <= right,
                        _ => left >= right,
                    },
                    _ => match name {
                        "lt" => left_float < right_float,
                        "gt" => left_float > right_float,
                        "le" => left_float <= right_float,
                        _ => left_float >= right_float,
                    },
                };
                Ok::<_, Error>(result)
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
/// A shebang line (`#!...`) is an interpreter directive for the kernel, not
/// part of the program. When a script starts with one, only the first line is
/// skipped — anything else keeps the source unchanged.
fn strip_shebang(source: &str) -> &str {
    match source.strip_prefix("#!") {
        Some(rest) => rest.find('\n').map_or("", |offset| &rest[offset + 1..]),
        None => source,
    }
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
    let src = src.map(|source| strip_shebang(&source).to_owned());
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
mod tests;
