use regex::{Regex, RegexBuilder};
use std::{
    cell::RefCell,
    collections::{HashSet, VecDeque},
    env, fmt,
    fs::{self},
    io::{self, BufRead, BufReader, Read, Write},
    rc::Rc,
    sync::atomic::{AtomicBool, Ordering},
    thread,
};
use unicode_segmentation::UnicodeSegmentation;

mod modules;

type Cell = Rc<RefCell<Value>>;
type EnvRef = Rc<RefCell<Env>>;

/// One step of a location path: inside the root cell's value, descend to an
/// array element or a struct field.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum PathStep {
    Index(i64),
    Field(String),
}

/// A reference to a logical location: a root cell plus the path from that
/// cell's current value to the referenced position. A Ref is resolved against
/// the CURRENT value at read/write time — it never pins or holds a snapshot
/// of an intermediate node, and the root cell stays alive for the Ref's whole
/// lifetime so no dangling storage is ever exposed.
#[derive(Clone)]
struct RefLocation {
    root: Cell,
    path: Vec<PathStep>,
}

#[derive(Clone)]
enum Value {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    /// Immutable snapshot: no interior Value is ever mutated in place.
    /// Writes rebuild a fresh snapshot and replace the value in the owning
    /// (root/location) cell.
    Array(Rc<Vec<Value>>),
    Struct(Rc<Vec<(String, Value)>>),
    Function(Rc<Function>),
    NativeFunction(Rc<NativeFunction>),
    Ref(Rc<RefLocation>),
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
    Http(String),
    Format(String),
    Regex(String),
    DuplicateKey(String),
    DuplicateBinding(String),
    Expect(String),
    Recursion(String),
    Match,
    ContinueOutsideLoop,
    BreakOutside,
    Interrupted,
    Quit(String),
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
            Http(s) => write!(f, "HTTPError: {s}"),
            Format(s) => write!(f, "FormatError: {s}"),
            Regex(s) => write!(f, "InvalidRegex: {s}"),
            DuplicateKey(s) => write!(f, "DuplicateKeyError: {s}"),
            DuplicateBinding(s) => write!(f, "DuplicateBindingError: {s}"),
            Expect(s) => write!(f, "ExpectationError: {s}"),
            Recursion(s) => write!(f, "RecursionError: {s}"),
            Match => write!(f, "MatchError: no predicate matched"),
            ContinueOutsideLoop => write!(f, "ContinueOutsideLoop"),
            BreakOutside => write!(f, "BreakOutsideLoop"),
            Interrupted => write!(f, "Interrupted"),
            Quit(s) => write!(f, "Quit: {s}"),
        }
    }
}
type EResult = Result<Value, Flow>;
enum Flow {
    Error(Error),
    Break(Value),
    Continue,
}

/// The maximum number of nested Lisp function calls before the recursion
/// guard trips. This is a tree-walking interpreter: every Lisp call nests a
/// chain of native eval/call/invoke frames — tens of KB of native stack per
/// level in debug builds (measured: ~28 KB lean, ~44 KB for bodies with
/// several nested forms). `MAX_CALL_DEPTH` levels therefore need on the
/// order of 100 MB of native stack, so the interpreter runs on a thread
/// with a large explicit stack (INTERPRETER_STACK below). Passing the guard
/// is a normal RecursionError (with the call site and live call chain), not
/// a stack-overflow abort. See spec.txt §fn and main().
const MAX_CALL_DEPTH: usize = 2048;
/// Native stack reserved for the interpreter thread (see MAX_CALL_DEPTH and
/// main()). Only the pages actually touched are committed.
const INTERPRETER_STACK: usize = 256 * 1024 * 1024;

thread_local! {
    static PARSE_ERROR_SPAN: RefCell<Option<Span>> = const { RefCell::new(None) };
    static CALL_TRACE: RefCell<Vec<(String, Span)>> = const { RefCell::new(Vec::new()) };
    static LAST_TRACE: RefCell<Vec<(String, Span)>> = const { RefCell::new(Vec::new()) };
    static LAST_ERROR_SPAN: RefCell<Option<Span>> = const { RefCell::new(None) };
    // Sources currently being evaluated, innermost last: lets the REPL commands
    // :l/:i/:bt map a span (a byte offset) back to file line numbers and source
    // text. Pushed and popped at every evaluation boundary (program file,
    // (eval …), each REPL line).
    static EVAL_SOURCES: RefCell<Vec<SourceCtx>> = const { RefCell::new(Vec::new()) };
    // Test hook: when set, REPL lines are taken from this queue instead of stdin.
    static REPL_INPUT: RefCell<Option<VecDeque<String>>> = const { RefCell::new(None) };
    // Test hook: when set, REPL console output (prompts, notices, command
    // output, echoed results) is captured here instead of written to the
    // terminal.
    static REPL_OUTPUT: RefCell<Option<Vec<String>>> = const { RefCell::new(None) };
    // Test hook: a per-thread signal that the REPL session loop treats as
    // Ctrl-C, so tests can simulate an interrupt deterministically without
    // racing the process-wide INTERRUPTED flag.
    static REPL_INTERRUPT: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}
static INTERRUPTED: AtomicBool = AtomicBool::new(false);

/// A source text plus how to report positions inside it: its label (a file
/// path, `<stdin>`, `<test>`, `<eval>` or `<repl>`) and how many leading lines
/// were stripped before parsing (a shebang), so displayed line numbers match
/// the original text.
#[derive(Clone)]
struct SourceCtx {
    label: String,
    source: String,
    line_offset: usize,
}

fn push_source(ctx: SourceCtx) {
    EVAL_SOURCES.with(|sources| sources.borrow_mut().push(ctx));
}
fn pop_source() {
    EVAL_SOURCES.with(|sources| {
        sources.borrow_mut().pop();
    });
}

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
        if matches!(c, '$' | '~') {
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
        while i < cs.len() && !cs[i].is_whitespace() && !"()[]{}:,.^\"';#".contains(cs[i]) {
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
fn bind_module(name: &str, value: Value, env: &EnvRef) -> Result<(), Error> {
    if !is_valid_identity(name) {
        return Err(Error::Type(format!(
            "invalid binding name `{name}`: allowed characters are [A-Za-z0-9_-] (ASCII), §2"
        )));
    }
    if env.borrow().values.iter().any(|(k, _)| k == name) {
        return Err(Error::DuplicateBinding(name.to_owned()));
    }
    env.borrow_mut()
        .values
        .push((name.to_owned(), Rc::new(RefCell::new(value))));
    Ok(())
}
fn follow(c: Cell) -> Result<(Cell, Vec<PathStep>), Error> {
    // Only cells whose value is an alias (Value::Ref) participate in a chain,
    // so the visited set is allocated lazily, once an actual alias is seen.
    let (mut root, mut steps) = (c, Vec::new());
    let mut visited: Option<HashSet<usize>> = None;
    loop {
        if !matches!(&*root.borrow(), Value::Ref(_)) {
            return Ok((root, steps));
        }
        let identity = Rc::as_ptr(&root) as usize;
        let fresh = match visited.as_mut() {
            Some(set) => set.insert(identity),
            None => {
                let mut set = HashSet::new();
                let fresh = set.insert(identity);
                visited = Some(set);
                fresh
            }
        };
        if !fresh {
            return Err(Error::Type("cyclic reference".into()));
        }
        let value = root.borrow().clone();
        let Value::Ref(loc) = value else {
            return Ok((root, steps));
        };
        root = loc.root.clone();
        // The chain's own steps come before what we already accumulated.
        let mut combined = loc.path.clone();
        combined.append(&mut steps);
        steps = combined;
    }
}

/// Resolve a location to its terminal (root cell, path) plus the value
/// currently there — WITHOUT resolving away an alias that sits at the
/// landing. Cell-level chains and value-level aliases on intermediate steps
/// are fully followed, so the returned (root, path) is the canonical location
/// a deref would read. Fails deterministically when the path no longer exists
/// or an intermediate value has the wrong type.
fn deref_landing(root: &Cell, steps: &[PathStep]) -> Result<(Cell, Vec<PathStep>, Value), Error> {
    let (root, prefix) = follow(root.clone())?;
    let mut all = prefix;
    all.extend(steps.iter().cloned());
    let mut value = root.borrow().clone();
    for (i, step) in all.iter().enumerate() {
        value = step_into(&value, step)?;
        if let Value::Ref(loc) = value {
            // The remaining steps continue inside the aliased location.
            let mut remaining = loc.path.clone();
            remaining.extend(all.iter().skip(i + 1).cloned());
            return deref_landing(&loc.root, &remaining);
        }
    }
    Ok((root, all, value))
}

/// The value currently at a location (root cell + path), shallow-cloned.
fn deref(root: &Cell, steps: &[PathStep]) -> Result<Value, Error> {
    match deref_landing(root, steps)?.2 {
        Value::Ref(loc) => deref(&loc.root, &loc.path),
        other => Ok(other),
    }
}

/// Resolve a value that may be an alias to the value it denotes.
fn deref_value(value: Value) -> Result<Value, Error> {
    match value {
        Value::Ref(loc) => deref(&loc.root, &loc.path),
        other => Ok(other),
    }
}

/// Clone the child value at one path step, or fail with the documented
/// type/name error. The child may itself be an alias, which the caller
/// resolves.
fn step_into(value: &Value, step: &PathStep) -> Result<Value, Error> {
    match (value, step) {
        (Value::Array(items), PathStep::Index(i)) => {
            let position = collection_position(&Value::Int(*i), items.len(), "array")?;
            Ok(items[position].clone())
        }
        (Value::Struct(fields), PathStep::Field(name)) => fields
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, v)| v.clone())
            .ok_or_else(|| Error::Name(format!("field {name}"))),
        (Value::Array(_), PathStep::Field(_)) => {
            Err(Error::Type("struct field access requires a struct".into()))
        }
        (Value::Struct(_), PathStep::Index(_)) => {
            Err(Error::Type("indexing requires an array".into()))
        }
        (_, PathStep::Index(_)) => Err(Error::Type("indexing requires an array".into())),
        (_, PathStep::Field(_)) => Err(Error::Type("struct field access requires a struct".into())),
    }
}

/// Replace the child at the FINAL step of `steps` inside `value`, rebuilding
/// every immutable snapshot along the path and returning the new top-level
/// value. The final step's current child is replaced outright — an alias
/// sitting at that position is a normal value and is reassigned, not written
/// through — matching the documented location semantics. Intermediate steps
/// were validated by `terminal_location`, which also translated value-level
/// aliases into jumps, so this pass only rebuilds.
fn update_snapshot(value: Value, steps: &[PathStep], new: Value) -> Result<Value, Error> {
    if steps.len() == 1 {
        return set_last(value, steps[0].clone(), new);
    }
    match (&value, &steps[0]) {
        (Value::Array(items), PathStep::Index(i)) => {
            let position = collection_position(&Value::Int(*i), items.len(), "array")?;
            let mut updated = (**items).clone();
            updated[position] = update_snapshot(updated[position].clone(), &steps[1..], new)?;
            Ok(Value::Array(Rc::new(updated)))
        }
        (Value::Struct(fields), PathStep::Field(name)) => {
            let position = fields
                .iter()
                .position(|(key, _)| key == name)
                .ok_or_else(|| Error::Name(format!("field {name}")))?;
            let mut updated = (**fields).clone();
            updated[position].1 = update_snapshot(updated[position].1.clone(), &steps[1..], new)?;
            Ok(Value::Struct(Rc::new(updated)))
        }
        (Value::Array(_), PathStep::Field(_)) => {
            Err(Error::Type("struct field access requires a struct".into()))
        }
        (Value::Struct(_), PathStep::Index(_)) => {
            Err(Error::Type("indexing requires an array".into()))
        }
        (_, PathStep::Index(_)) => Err(Error::Type("indexing requires an array".into())),
        (_, PathStep::Field(_)) => Err(Error::Type("struct field access requires a struct".into())),
    }
}

/// Replace the child at a single final step inside `value`, returning a new
/// immutable snapshot.
fn set_last(value: Value, step: PathStep, new: Value) -> Result<Value, Error> {
    match (value, step) {
        (Value::Array(items), PathStep::Index(i)) => {
            let position = collection_position(&Value::Int(i), items.len(), "array")?;
            let mut updated = (*items).clone();
            updated[position] = new;
            Ok(Value::Array(Rc::new(updated)))
        }
        (Value::Struct(fields), PathStep::Field(name)) => {
            let position = fields
                .iter()
                .position(|(key, _)| key == &name)
                .ok_or_else(|| Error::Name(format!("field {name}")))?;
            let mut updated = (*fields).clone();
            updated[position].1 = new;
            Ok(Value::Struct(Rc::new(updated)))
        }
        (Value::Array(_), PathStep::Field(_)) => {
            Err(Error::Type("struct field access requires a struct".into()))
        }
        (Value::Struct(_), PathStep::Index(_)) => {
            Err(Error::Type("indexing requires an array".into()))
        }
        (_, PathStep::Index(_)) => Err(Error::Type("indexing requires an array".into())),
        (_, PathStep::Field(_)) => Err(Error::Type("struct field access requires a struct".into())),
    }
}

/// Resolve a location to its terminal write target: follow the root cell's
/// alias chain, then walk the path's intermediate steps (validating them),
/// translating any value-level alias encountered along the way into a jump —
/// the remaining steps continue inside that alias's location (written
/// through). Returns the terminal root cell, the full remaining path, and the
/// container value the final step applies to. The final step itself is
/// validated by the write. Fails deterministically when an intermediate step
/// no longer exists or has the wrong type.
fn terminal_location(
    root: Cell,
    steps: Vec<PathStep>,
) -> Result<(Cell, Vec<PathStep>, Value), Error> {
    let (root, chain) = follow(root)?;
    let mut all = chain;
    all.extend(steps);
    if all.is_empty() {
        let value = root.borrow().clone();
        return Ok((root, all, value));
    }
    let mut value = root.borrow().clone();
    for i in 0..all.len() - 1 {
        value = step_into(&value, &all[i])?;
        if let Value::Ref(loc) = value {
            let (r, chain) = follow(loc.root.clone())?;
            let mut rest = chain;
            rest.extend(loc.path.iter().cloned());
            rest.extend(all.iter().skip(i + 1).cloned());
            return terminal_location(r, rest);
        }
    }
    Ok((root, all, value))
}

/// Perform a logical-location write to an already-resolved terminal target:
/// rebuild the immutable snapshots along the whole path and replace the value
/// in the terminal cell. Snapshots are never mutated; the caller resolves the
/// location with `terminal_location` (which follows cell-level chains and
/// translates value-level aliases on intermediate steps into jumps), so a
/// cycle check can reuse the same result. Fails deterministically when the
/// path is invalid.
fn assign_to(root: Cell, steps: Vec<PathStep>, new: Value) -> Result<(), Error> {
    if steps.is_empty() {
        *root.borrow_mut() = new;
        return Ok(());
    }
    let value = root.borrow().clone();
    let updated = update_snapshot(value, &steps, new)?;
    *root.borrow_mut() = updated;
    Ok(())
}

/// Whether `value` contains an alias (Value::Ref) anywhere. Cycle detection
/// only needs to run when this is true: values without references can only
/// marshal freshly created snapshots, so no walk can reach an existing
/// location.
fn value_contains_ref(value: &Value) -> bool {
    match value {
        Value::Ref(_) => true,
        Value::Array(items) => items.iter().any(value_contains_ref),
        Value::Struct(fields) => fields.iter().any(|(_, v)| value_contains_ref(v)),
        _ => false,
    }
}

/// Whether writing `value` to a location (root cell + path) creates a cycle:
/// true when some alias reachable from `value` resolves back to that same
/// location (or passes through it), so dereferencing the written value would
/// loop forever.
fn creates_cycle(value: &Value, root: &Cell, path: &[PathStep]) -> bool {
    fn is_prefix(prefix: &[PathStep], full: &[PathStep]) -> bool {
        prefix.len() <= full.len() && prefix.iter().zip(full.iter()).all(|(a, b)| a == b)
    }
    fn hit(root: &Cell, path: &[PathStep], target: &Cell, target_path: &[PathStep]) -> bool {
        Rc::ptr_eq(root, target) && (is_prefix(path, target_path) || is_prefix(target_path, path))
    }
    fn reachable(
        value: &Value,
        target: &Cell,
        target_path: &[PathStep],
        seen: &mut HashSet<(usize, Vec<PathStep>)>,
    ) -> bool {
        match value {
            Value::Ref(loc) => {
                // A direct hit on the target location (same root, one path a
                // prefix of the other) is a cycle: reading it reaches the
                // written position.
                if hit(&loc.root, &loc.path, target, target_path) {
                    return true;
                }
                // Resolve to the canonical landing WITHOUT flattening away an
                // alias that sits at the end of the path, then walk the
                // landing: its location may pass through the target (e.g. two
                // elements referencing each other) even when the alias itself
                // did not.
                match deref_landing(&loc.root, &loc.path) {
                    Ok((landing_root, landing_path, landing)) => {
                        if hit(&landing_root, &landing_path, target, target_path) {
                            return true;
                        }
                        let key = (Rc::as_ptr(&landing_root) as usize, landing_path);
                        if !seen.insert(key) {
                            return false; // location already walked, cannot add a cycle
                        }
                        reachable(&landing, target, target_path, seen)
                    }
                    Err(_) => false, // invalid path: the read fails, cannot cycle
                }
            }
            Value::Array(items) => items
                .iter()
                .any(|item| reachable(item, target, target_path, seen)),
            Value::Struct(fields) => fields
                .iter()
                .any(|(_, v)| reachable(v, target, target_path, seen)),
            _ => false,
        }
    }
    let mut seen = HashSet::new();
    reachable(value, root, path, &mut seen)
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
        Value::Ref(loc) => match deref(&loc.root, &loc.path) {
            Ok(value) => render(&value),
            Err(_) => "<invalid reference>".into(),
        },
        Value::Array(a) => format!(
            "[{}]",
            a.iter().map(render_nested).collect::<Vec<_>>().join(" ")
        ),
        Value::Struct(s) => format!(
            "{{{}}}",
            s.iter()
                .map(|(k, v)| format!("{k}:{}", render_nested(v)))
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
        Value::Ref(loc) => match deref(&loc.root, &loc.path) {
            Ok(value) => render_nested(&value),
            Err(_) => "null".into(),
        },
        Value::Array(_) | Value::Struct(_) => render(v),
        _ => render(v),
    }
}
/// Whether an expression can be resolved to an assignment cell (a variable,
/// struct field, or array element). Used to skip evaluating location-shaped
/// index bases to a full value copy.
fn is_location(e: &Expr) -> bool {
    matches!(
        e.kind,
        ExprKind::Symbol(_) | ExprKind::Field(_, _) | ExprKind::Index(_, _)
    )
}

fn location(e: &Expr, env: &EnvRef) -> Result<(Cell, Vec<PathStep>), Error> {
    match &e.kind {
        ExprKind::Symbol(n) => Ok((
            lookup(env, n).ok_or_else(|| Error::Name(n.clone()))?,
            Vec::new(),
        )),
        ExprKind::Field(base, key) => {
            let (root, mut steps) = location(base, env)?;
            steps.push(PathStep::Field(key.clone()));
            Ok((root, steps))
        }
        ExprKind::Index(base, IndexSpec::Selector(selector)) => {
            let index = match eval(selector, env, 0, 0).map_err(flow_err)? {
                Value::Int(index) => index,
                Value::Array(_) => {
                    return Err(Error::Type(
                        "a multi-index selector cannot be a reference target".into(),
                    ))
                }
                _ => return Err(Error::Type("array index must be an integer".into())),
            };
            let (root, mut steps) = location(base, env)?;
            steps.push(PathStep::Index(index));
            Ok((root, steps))
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
        ExprKind::Symbol(n) => {
            let cell = lookup(env, n).ok_or_else(|| Flow::Error(Error::Name(n.clone())))?;
            deref(&cell, &[]).map_err(Into::into)
        }
        ExprKind::Field(_, _) => {
            let (root, path) = location(e, env).map_err(Flow::Error)?;
            deref(&root, &path).map_err(Flow::Error)
        }
        ExprKind::Index(target, spec) => {
            // Location-shaped bases (a variable, field, or nested index)
            // resolve straight to their location so the selector applies to
            // the value there without evaluating the base to a copy first.
            // Non-location bases (e.g. a call result) still evaluate to a
            // value. Either way the result is an O(1) shallow clone: Model P
            // snapshots are immutable, so reads never deep-copy.
            if matches!(spec, IndexSpec::Selector(_)) && is_location(target) {
                let (root, path) = location(target, env).map_err(Flow::Error)?;
                let value = deref(&root, &path).map_err(Flow::Error)?;
                apply_index(value, spec, env, loop_depth, match_depth)
            } else {
                let value = eval(target, env, loop_depth, match_depth)?;
                apply_index(value, spec, env, loop_depth, match_depth)
            }
        }
        ExprKind::Ref(x) => location(x, env)
            .map(|(root, path)| Value::Ref(Rc::new(RefLocation { root, path })))
            .map_err(Into::into),
        ExprKind::Array(xs) => {
            let mut v = Vec::with_capacity(xs.len());
            for x in xs {
                v.push(eval(x, env, loop_depth, match_depth)?);
            }
            Ok(Value::Array(Rc::new(v)))
        }
        ExprKind::Struct(xs) => {
            let mut seen = HashSet::new();
            let mut v = Vec::with_capacity(xs.len());
            for field in xs {
                if !seen.insert(&field.key) {
                    LAST_ERROR_SPAN.with(|span| *span.borrow_mut() = Some(field.span));
                    return Err(Error::DuplicateKey(field.key.clone()).into());
                }
                v.push((
                    field.key.clone(),
                    eval(&field.value, env, loop_depth, match_depth)?,
                ));
            }
            Ok(Value::Struct(Rc::new(v)))
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
        Value::Array(values) => match spec {
            IndexSpec::Selector(_) => {
                if let Value::Array(indices) = selector {
                    let mut selected = Vec::with_capacity(indices.len());
                    for index in indices.iter() {
                        let position = collection_position(index, values.len(), "array")?;
                        selected.push(deref_value(values[position].clone())?);
                    }
                    Ok(Value::Array(Rc::new(selected)))
                } else {
                    let position = collection_position(&selector, values.len(), "array")?;
                    deref_value(values[position].clone()).map_err(Into::into)
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
                    return Ok(Value::Array(Rc::new(Vec::new())));
                };
                let selected = values[start..=end]
                    .iter()
                    .map(|value| deref_value(value.clone()))
                    .collect::<Result<Vec<_>, Error>>()?;
                Ok(Value::Array(Rc::new(selected)))
            }
        },
        Value::Str(string) => {
            let graphemes: Vec<&str> = string.graphemes(true).collect();
            match spec {
                IndexSpec::Selector(_) => {
                    if let Value::Array(indices) = selector {
                        let mut selected = Vec::with_capacity(indices.len());
                        for index in indices.iter() {
                            let position = collection_position(index, graphemes.len(), "string")?;
                            selected.push(Value::Str(graphemes[position].to_owned()));
                        }
                        Ok(Value::Array(Rc::new(selected)))
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
const RESERVED_NAMES: &[&str] = &[
    "let", "set", "if", "fn", "loop", "break", "continue", "match", "and", "or", "not", "expect",
    "use", "eval", "$", "add", "sub", "mul", "div", "mod", "pow", "eq", "ne", "lt", "gt", "le",
    "ge", "bit-and", "bit-or", "bit-xor", "bit-not", "bit-shl", "bit-shr", "repl",
];

fn is_reserved_name(name: &str) -> bool {
    RESERVED_NAMES.contains(&name)
}

fn is_valid_identity(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

fn define_let(args: &[Expr], env: &EnvRef, l: usize, m: usize) -> Result<(String, Value), Flow> {
    need(args, 2, "let")?;
    let name = if let ExprKind::Symbol(name) = &args[0].kind {
        name.clone()
    } else {
        return Err(Error::Type("let name must be an identifier".into()).into());
    };
    if is_reserved_name(&name) {
        return Err(Error::Type(format!("reserved name cannot be bound: `{name}`")).into());
    }
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

/// REPL console. Real sessions are driven from the process's controlling
/// terminal (/dev/tty) so that program stdin is never consumed by the REPL;
/// tests inject lines through the REPL_INPUT thread-local hook instead.
enum ReplConsole {
    Queued(VecDeque<String>),
    Tty {
        reader: BufReader<fs::File>,
        writer: fs::File,
    },
}

fn open_repl_console() -> Result<ReplConsole, Error> {
    if let Some(lines) = REPL_INPUT.with(|queue| queue.borrow_mut().take()) {
        return Ok(ReplConsole::Queued(lines));
    }
    let tty = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/tty")
        .map_err(|error| {
            Error::Io(format!(
                "repl: cannot open the controlling terminal (/dev/tty): {error}"
            ))
        })?;
    let reader = BufReader::new(tty.try_clone().map_err(|error| {
        Error::Io(format!(
            "repl: cannot duplicate /dev/tty for reading: {error}"
        ))
    })?);
    Ok(ReplConsole::Tty {
        reader,
        writer: tty,
    })
}

impl ReplConsole {
    /// One REPL line: a queued test line, or from the controlling terminal.
    fn read_line(&mut self) -> Option<String> {
        match self {
            ReplConsole::Queued(lines) => lines.pop_front(),
            ReplConsole::Tty { reader, writer: _ } => {
                let mut line = String::new();
                match reader.read_line(&mut line) {
                    Ok(0) | Err(_) => None,
                    Ok(_) => Some(line),
                }
            }
        }
    }
    /// Echo an evaluated result: program stdout in test mode, the terminal otherwise.
    fn echo(&mut self, text: &str) {
        match self {
            ReplConsole::Queued(_) => {
                REPL_OUTPUT.with(|output| {
                    let mut capture = output.borrow_mut();
                    if let Some(lines) = capture.as_mut() {
                        lines.push(format!("{text}\n"));
                    } else {
                        let _ = writeln!(io::stdout(), "{text}");
                    }
                });
            }
            ReplConsole::Tty { writer, .. } => {
                let _ = writeln!(writer, "{text}");
                let _ = writer.flush();
            }
        }
    }
    /// Prompt, diagnostic and notice output: program stderr in test mode, the
    /// terminal otherwise.
    fn notify(&mut self, text: &str) {
        match self {
            ReplConsole::Queued(_) => {
                REPL_OUTPUT.with(|output| {
                    let mut capture = output.borrow_mut();
                    if let Some(lines) = capture.as_mut() {
                        lines.push(text.to_owned());
                    } else {
                        let _ = write!(io::stderr(), "{text}");
                        let _ = io::stderr().flush();
                    }
                });
            }
            ReplConsole::Tty { writer, .. } => {
                let _ = writer.write_all(text.as_bytes());
                let _ = writer.flush();
            }
        }
    }
}
fn repl_echo(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::Function(_) => "<fn>".into(),
        Value::NativeFunction(_) => "<native fn>".into(),
        other => lisp_source(other).unwrap_or_else(|_| debug_render(other)),
    }
}
fn repl_diagnostic(error: &Error, line: &str, span: Option<Span>) -> String {
    let span = span.unwrap_or(Span { start: 0, end: 0 });
    let offset = span.start.min(line.len());
    let column = line[..offset].chars().count() + 1;
    let caret = format!("{}^", " ".repeat(column.saturating_sub(1)));
    format!("<repl>:1:{column}: {error}\n{line}\n{caret}")
}

/// What a REPL command asked the session to do.
enum ReplCommand {
    /// Resume the program (`:c`/`:continue`).
    Resume,
    /// Abort the whole run (`:q`/`:quit`).
    Quit,
    /// Print and keep the session open.
    Stay,
}

/// Handle one `:`-prefixed REPL line. `command` is the trimmed text after the
/// colon; like the evaluator, `:c`/`:q`/`:h` keep their exact no-argument
/// behavior, while `:l`/`:i`/`:bt` accept arguments.
fn handle_repl_command(
    command: &str,
    session: &EnvRef,
    console: &mut ReplConsole,
    source: &Option<SourceCtx>,
    execution_span: Span,
) -> ReplCommand {
    let mut words = command.split_whitespace();
    let cmd = words.next().unwrap_or("");
    let args: Vec<&str> = words.collect();
    match cmd {
        "c" | "continue" if args.is_empty() => ReplCommand::Resume,
        "q" | "quit" if args.is_empty() => ReplCommand::Quit,
        "h" | "help" if args.is_empty() => {
            console.notify(
                "commands: :c/:continue resume, :q/:quit abort, \
                 :l/:list [N] show source around the execution point, \
                 :i/:inspect [name] show the bindings, \
                 :bt/:backtrace show the call stack, \
                 :h/:help this help; anything else is evaluated as Lisp\n",
            );
            ReplCommand::Stay
        }
        "l" | "list" => {
            let window = match args.len() {
                0 => Ok(3),
                1 => args[0].parse::<usize>().map_err(|_| args[0]),
                _ => {
                    console.notify(&format!("repl: :{cmd} expects zero or one count\n"));
                    return ReplCommand::Stay;
                }
            };
            match window {
                Ok(window) => console.notify(&repl_source_window(source, execution_span, window)),
                Err(bad) => console.notify(&format!(
                    "repl: :{cmd} expects a non-negative count, got `{bad}`\n"
                )),
            }
            ReplCommand::Stay
        }
        "i" | "inspect" => {
            match args.len() {
                0 => console.notify(&repl_binding_table(session)),
                1 => console.notify(&repl_inspect_binding(session, args[0], source)),
                _ => console.notify(&format!("repl: :{cmd} expects one name or none (try :h)\n")),
            }
            ReplCommand::Stay
        }
        "bt" | "backtrace" => {
            if !args.is_empty() {
                console.notify(&format!("repl: :{cmd} takes no arguments\n"));
            } else {
                console.notify(&repl_backtrace(source));
            }
            ReplCommand::Stay
        }
        _ => {
            console.notify(&format!("repl: unknown command `:{command}` (try :h)\n"));
            ReplCommand::Stay
        }
    }
}

/// Session prompt: `label:line> ` (or `file:line> `) when the execution point
/// maps to a source line, otherwise `repl> ` / `label> `.
fn repl_prompt(label: &str, source: &Option<SourceCtx>, execution_span: Span) -> String {
    let prefix = if label.is_empty() {
        "repl".to_owned()
    } else {
        label.to_owned()
    };
    let Some(ctx) = source else {
        return format!("{prefix}> ");
    };
    if execution_span.start >= ctx.source.len() {
        return format!("{prefix}> ");
    }
    let line = line_of(execution_span.start, &ctx.source) + ctx.line_offset;
    if label.is_empty() {
        format!("{}:{line}> ", ctx.label)
    } else {
        format!("{label}@{}:{line}> ", ctx.label)
    }
}

/// A `:l` window: the source lines around the execution point, the
/// execution-point line marked `>`.
fn repl_source_window(source: &Option<SourceCtx>, execution_span: Span, window: usize) -> String {
    let Some(ctx) = source else {
        return "repl: no source context for the execution point\n".into();
    };
    let offset = execution_span.start.min(ctx.source.len());
    let center = line_of(offset, &ctx.source);
    let newlines = ctx.source.bytes().filter(|b| *b == b'\n').count();
    let total = newlines + 1 - usize::from(ctx.source.ends_with('\n'));
    let first = center.saturating_sub(window).max(1);
    let last = (center + window).min(total);
    let width = format!("{}", last + ctx.line_offset).len();
    let mut out = format!(
        "@ {}:{} (execution point)\n",
        ctx.label,
        center + ctx.line_offset
    );
    for line in first..=last {
        let (start, end) = line_bounds(&ctx.source, line);
        let text = ctx.source[start..end].trim();
        let marker = if line == center { ">" } else { " " };
        out.push_str(&format!(
            " {marker} {:>width$}  {text}\n",
            line + ctx.line_offset,
            width = width
        ));
    }
    out
}

/// `:i`: the effective binding table across the session's scope chain, each
/// name once (innermost wins), `*` marking names with an outer duplicate.
fn repl_binding_table(session: &EnvRef) -> String {
    let mut scopes: Vec<Vec<(String, Cell)>> = Vec::new();
    let mut current = Some(session.clone());
    while let Some(env) = current {
        let (values, parent) = {
            let borrow = env.borrow();
            (borrow.values.clone(), borrow.parent.clone())
        };
        scopes.push(values);
        current = parent;
    }
    let mut seen: HashSet<String> = HashSet::new();
    let mut shadowed: HashSet<String> = HashSet::new();
    let mut rows: Vec<(String, Cell)> = Vec::new();
    for scope in &scopes {
        for (name, cell) in scope {
            if seen.insert(name.clone()) {
                rows.push((name.clone(), cell.clone()));
            } else {
                shadowed.insert(name.clone());
            }
        }
    }
    rows.sort_by(|a, b| a.0.cmp(&b.0));
    let mut out = format!(
        "{} binding{} across {} scope{}\n",
        rows.len(),
        if rows.len() == 1 { "" } else { "s" },
        scopes.len(),
        if scopes.len() == 1 { "" } else { "s" },
    );
    const MAX_ROWS: usize = 200;
    for (name, cell) in rows.iter().take(MAX_ROWS) {
        let value = clip(&repl_value(&cell.borrow()), 60);
        let star = if shadowed.contains(name) { " *" } else { "" };
        out.push_str(&format!("{name} = {value}{star}\n"));
    }
    if rows.len() > MAX_ROWS {
        out.push_str(&format!("… and {} more bindings\n", rows.len() - MAX_ROWS));
    }
    out
}

/// `:i name`: the value of a binding, its type and scope, the shadow chain
/// when one exists, and (for functions) where it was defined.
fn repl_inspect_binding(session: &EnvRef, name: &str, source: &Option<SourceCtx>) -> String {
    let mut occurrences: Vec<(usize, Cell)> = Vec::new();
    let mut current = Some(session.clone());
    let mut depth = 0usize;
    while let Some(env) = current {
        let (hit, parent) = {
            let borrow = env.borrow();
            let hit = borrow
                .values
                .iter()
                .rev()
                .find(|(key, _)| key == name)
                .map(|(_, cell)| cell.clone());
            (hit, borrow.parent.clone())
        };
        if let Some(cell) = hit {
            occurrences.push((depth, cell));
        }
        current = parent;
        depth += 1;
    }
    let Some((depth, cell)) = occurrences.first() else {
        return format!("repl: no binding named `{name}`\n");
    };
    let scope_word = match depth {
        0 => "session",
        1 => "enclosing",
        _ => "outer",
    };
    let value = cell.borrow();
    let rendered = repl_value(&value);
    let type_name = value_type(&value);
    drop(value);
    let mut out = format!("{name} = {rendered}   {type_name}   scope {depth} ({scope_word})\n");
    for (outer_depth, outer) in occurrences.iter().skip(1) {
        let outer_value = outer.borrow();
        out.push_str(&format!(
            "  outer {name} = {} @ scope {outer_depth}\n",
            repl_value(&outer_value),
        ));
    }
    if let Value::Function(function) = &*cell.borrow() {
        out.push_str(&format!(
            "  def: {}\n",
            repl_function_def(function, session, source)
        ));
    }
    out
}

/// Where a function value was defined: `label:line` in a registered source,
/// or a note when it was created inside the session.
fn repl_function_def(function: &Function, session: &EnvRef, source: &Option<SourceCtx>) -> String {
    if env_contains(&function.env, session) {
        return "defined in this session (repl)".into();
    }
    let Some(ctx) = source else {
        return "defined outside the current source".into();
    };
    if function.body.span.start >= ctx.source.len() {
        return "defined outside the current source".into();
    }
    let line = line_of(function.body.span.start, &ctx.source) + ctx.line_offset;
    format!("{}:{}", ctx.label, line)
}

/// `:bt`: the live call stack, innermost frame first, with source positions.
fn repl_backtrace(source: &Option<SourceCtx>) -> String {
    let trace = CALL_TRACE.with(|trace| trace.borrow().clone());
    if trace.is_empty() {
        return "repl: backtrace is empty ((repl) is at the top level, not inside a function)\n"
            .into();
    }
    let noun = if trace.len() == 1 { "frame" } else { "frames" };
    let mut out = format!("backtrace ({} {noun})\n", trace.len());
    let Some(ctx) = source else {
        for (name, _) in trace.iter().rev() {
            out.push_str(&format!("  {name}\n"));
        }
        return out;
    };
    for (name, frame) in trace.iter().rev() {
        if frame.start >= ctx.source.len() {
            out.push_str(&format!("  {name} at <unknown source>\n"));
            continue;
        }
        let line = line_of(frame.start, &ctx.source) + ctx.line_offset;
        let column = column_of(frame.start, &ctx.source);
        let text = line_text_at(frame.start, &ctx.source).trim().to_owned();
        let detail = if text.is_empty() {
            String::new()
        } else {
            format!("   ({text})")
        };
        out.push_str(&format!(
            "  {name} at {}:{line}:{column}{detail}\n",
            ctx.label
        ));
    }
    out
}

/// Render a value for `:i`: functions become `(fn (params))`, null `_`.
fn repl_value(value: &Value) -> String {
    match value {
        Value::Null => "_".into(),
        Value::Function(f) => {
            let params = f.params.join(" ");
            match &f.name {
                Some(name) => format!("(fn {name} ({params}))"),
                None => format!("(fn ({params}))"),
            }
        }
        Value::NativeFunction(f) => format!("<native fn {}>", f.name),
        other => lisp_source(other).unwrap_or_else(|_| debug_render(other)),
    }
}

/// Truncate a rendered value when it does not fit a table row.
fn clip(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        text.to_owned()
    } else {
        let mut clipped: String = text.chars().take(max).collect();
        clipped.push('…');
        clipped
    }
}

/// Whether `env` is `target` or one of its ancestors.
fn env_contains(env: &EnvRef, target: &EnvRef) -> bool {
    let mut current = Some(env.clone());
    while let Some(e) = current {
        if Rc::ptr_eq(&e, target) {
            return true;
        }
        current = e.borrow().parent.clone();
    }
    false
}

/// 1-based line number of a byte offset (the same math diagnostics use).
fn line_of(offset: usize, source: &str) -> usize {
    let offset = offset.min(source.len());
    source[..offset].bytes().filter(|b| *b == b'\n').count() + 1
}

/// 1-based column of a byte offset.
fn column_of(offset: usize, source: &str) -> usize {
    let offset = offset.min(source.len());
    let line_start = source[..offset].rfind('\n').map_or(0, |p| p + 1);
    source[line_start..offset].chars().count() + 1
}

/// Byte range (start, end-exclusive-of-`\n`) of a 1-based line.
fn line_bounds(source: &str, line: usize) -> (usize, usize) {
    let mut start = 0;
    for _ in 1..line {
        match source[start..].find('\n') {
            Some(i) => start += i + 1,
            None => return (source.len(), source.len()),
        }
    }
    let end = source[start..]
        .find('\n')
        .map_or(source.len(), |i| start + i);
    (start, end)
}

/// The text of the line containing a byte offset (no trailing newline).
fn line_text_at(offset: usize, source: &str) -> String {
    let offset = offset.min(source.len());
    let start = source[..offset].rfind('\n').map_or(0, |p| p + 1);
    let end = source[offset..]
        .find('\n')
        .map_or(source.len(), |p| offset + p);
    source[start..end].to_owned()
}

/// Restores the pre-session Ctrl-C state when the session ends, so an
/// interrupt that lands during the session does not linger forever.
struct InterruptedGuard {
    interrupt: bool,
    test_interrupt: bool,
}
impl Drop for InterruptedGuard {
    fn drop(&mut self) {
        INTERRUPTED.store(self.interrupt, Ordering::Relaxed);
        REPL_INTERRUPT.with(|flag| flag.set(self.test_interrupt));
    }
}

fn eval_repl_line(source: &str, env: &EnvRef, loop_depth: usize, match_depth: usize) -> EResult {
    let tokens = lex(source)?;
    let program = Parser { ts: tokens, i: 0 }.program()?;
    push_source(SourceCtx {
        label: "<repl>".into(),
        source: source.to_owned(),
        line_offset: 0,
    });
    let outcome = (|| -> EResult {
        let mut result = Value::Null;
        for form in program {
            result = eval(&form, env, loop_depth, match_depth)?;
        }
        Ok(result)
    })();
    pop_source();
    outcome
}
fn run_repl(
    env: &EnvRef,
    loop_depth: usize,
    match_depth: usize,
    label: &str,
    execution_span: Span,
    source: Option<SourceCtx>,
) -> EResult {
    // Evaluate REPL lines in a disposable child scope: `let` binds only inside
    // the session, while `set` and reads still reach the program's live state.
    let session = new_env(Some(env.clone()));
    let prompt = repl_prompt(label, &source, execution_span);
    // A Ctrl-C inside the session cancels the current line and keeps the
    // session alive; the pre-session state is restored on exit, so a Ctrl-C
    // after resuming still aborts the program.
    let _guard = InterruptedGuard {
        interrupt: INTERRUPTED.load(Ordering::Relaxed),
        test_interrupt: REPL_INTERRUPT.with(|flag| flag.get()),
    };
    let mut console = open_repl_console()?;
    let mut result = Value::Null;
    loop {
        let interrupted = INTERRUPTED.swap(false, Ordering::Relaxed)
            || REPL_INTERRUPT.with(|flag| flag.replace(false));
        if interrupted {
            console.notify("repl: interrupted (:c continues, :q quits)\n");
            continue;
        }
        console.notify(&prompt);
        let Some(raw) = console.read_line() else {
            break;
        };
        let line = raw.trim_end_matches(['\r', '\n']).to_owned();
        if line.trim().is_empty() {
            continue;
        }
        if let Some(command) = line.strip_prefix(':') {
            match handle_repl_command(
                command.trim(),
                &session,
                &mut console,
                &source,
                execution_span,
            ) {
                ReplCommand::Resume => break,
                ReplCommand::Quit => {
                    return Err(Flow::Error(Error::Quit(
                        "repl: aborted by user (:quit)".into(),
                    )))
                }
                ReplCommand::Stay => {}
            }
            continue;
        }
        match eval_repl_line(&line, &session, loop_depth, match_depth) {
            Ok(value) => {
                result = value;
                let echo = repl_echo(&result);
                if !echo.is_empty() {
                    console.echo(&echo);
                }
            }
            Err(Flow::Error(error)) => {
                if matches!(error, Error::Quit(_)) {
                    // A nested (repl) quit aborts the whole run, not just this session.
                    return Err(Flow::Error(error));
                }
                if matches!(error, Error::Interrupted) {
                    // Ctrl-C landed mid-line: cancel the line, stay in the session.
                    INTERRUPTED.store(false, Ordering::Relaxed);
                    REPL_INTERRUPT.with(|flag| flag.set(false));
                    console.notify("repl: interrupted (:c continues, :q quits)\n");
                    continue;
                }
                // Errors inside a REPL line never propagate to the program.
                let span = match &error {
                    Error::Parse(_) => PARSE_ERROR_SPAN.with(|span| *span.borrow()),
                    _ => LAST_ERROR_SPAN.with(|span| *span.borrow()),
                };
                console.notify(&format!("{}\n", repl_diagnostic(&error, &line, span)));
            }
            Err(flow) => return Err(flow), // (break)/(continue) act on the enclosing loop
        }
    }
    Ok(result)
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
                let loc = location(&args[0], env).map_err(Flow::Error)?;
                let v = eval(&args[1], env, l, m)?;
                // Resolve the write target first: aliases on the way to the
                // position are written through, so a cycle check against the
                // raw location would miss chains that land on it.
                let (root, path, _) = terminal_location(loc.0, loc.1).map_err(Flow::Error)?;
                // A cycle needs an alias in the assigned value pointing back
                // at the location being written; plain literals and snapshots
                // only marshal fresh values, so the walk runs only when a
                // Value::Ref is actually present.
                if value_contains_ref(&v) && creates_cycle(&v, &root, &path) {
                    return Err(Error::Type("cyclic reference".into()).into());
                }
                assign_to(root, path, v).map_err(Flow::Error)?;
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
                if let Some(p) = ps.iter().find(|p| is_reserved_name(p)) {
                    return Err(Error::Type(format!(
                        "reserved name cannot be used as a parameter: `{p}`"
                    ))
                    .into());
                }
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
                let mut bound = false;
                let (name, module) = match &args[0].kind {
                    ExprKind::Call(let_head, let_args) if matches!(&let_head.kind, ExprKind::Symbol(name) if name == "let") =>
                    {
                        let (name, module) = define_let(let_args, env, l, m)?;
                        bound = true;
                        (name, module)
                    }
                    ExprKind::Lit(Literal::Str(name)) => (name.clone(), native_module(name)?),
                    _ => {
                        let name = as_str(eval(&args[0], env, l, m)?)?;
                        let module = native_module(&name)?;
                        (name, module)
                    }
                };
                validate_module(&module).map_err(Flow::Error)?;
                if !bound {
                    // Never overwrite an existing binding: only `set` modifies one
                    // (a duplicate in the current scope is DuplicateBindingError).
                    bind_module(&name, module.clone(), env).map_err(Flow::Error)?;
                }
                return Ok(module);
            }
            "$" => return format_value(args, env, l, m),
            "eval" => {
                need(args, 1, "eval")?;
                let source = as_str(eval(&args[0], env, l, m)?)?;
                let program = (|| -> Result<Vec<Expr>, Error> {
                    let ts = lex(&source)?;
                    Parser { ts, i: 0 }.program()
                })()
                .inspect_err(|_| {
                    PARSE_ERROR_SPAN.with(|span| *span.borrow_mut() = None);
                    LAST_ERROR_SPAN.with(|span| *span.borrow_mut() = Some(call_span));
                })?;
                push_source(SourceCtx {
                    label: "<eval>".into(),
                    source: source.clone(),
                    line_offset: 0,
                });
                let mut result = Value::Null;
                let outcome = (|| {
                    for form in program {
                        result = match eval(&form, env, l, m) {
                            Ok(value) => value,
                            Err(error) => {
                                LAST_ERROR_SPAN.with(|span| *span.borrow_mut() = Some(call_span));
                                return Err(error);
                            }
                        };
                    }
                    Ok(result)
                })();
                pop_source();
                return outcome;
            }
            "repl" => {
                if args.len() > 1 {
                    return Err(Error::Arity("repl expects zero or one argument".into()).into());
                }
                let label = if args.is_empty() {
                    String::new()
                } else {
                    as_str(eval(&args[0], env, l, m)?)?
                };
                // The innermost source being evaluated right now is the one the
                // (repl) form lives in; :l/:i/:bt resolve spans against it.
                let session_source = EVAL_SOURCES.with(|sources| sources.borrow().last().cloned());
                return run_repl(env, l, m, &label, call_span, session_source);
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
        if fields.iter().any(|(key, _)| key == "_") && fields.iter().any(|(key, _)| key == "spec") {
            if let Err(error) = validate_descriptor(fields, &vals) {
                LAST_ERROR_SPAN.with(|span| *span.borrow_mut() = Some(call_span));
                return Err(error.into());
            }
            let call = fields
                .iter()
                .find(|(key, _)| key == "_")
                .map(|(_, v)| v.clone())
                .expect("descriptor pair guarantees a _ field");
            return invoke(call, vals, call_span);
        }
    }
    invoke(value, vals, call_span)
}
fn validate_module(value: &Value) -> Result<(), Error> {
    let Value::Struct(fields) = value else {
        return Err(Error::Type("module must be a struct".into()));
    };
    for (_, v) in fields.iter() {
        let Value::Struct(descriptor) = v.clone() else {
            return Err(Error::Type(
                "module members must be callable descriptors".into(),
            ));
        };
        validate_descriptor_spec(&descriptor)?;
    }
    Ok(())
}
/// Valid `spec.type` names: the `value_type` vocabulary plus the `"any"` wildcard.
const SPEC_TYPE_VOCABULARY: [&str; 10] = [
    "null", "bool", "int", "float", "string", "array", "struct", "function", "ref", "any",
];

fn validate_descriptor_spec(
    fields: &Rc<Vec<(String, Value)>>,
) -> Result<(usize, Vec<Vec<String>>), Error> {
    // This runs on every descriptor call, while module members were already
    // validated once at `use` time — so read the spec in place instead of
    // deep-copying each field. The checks and error messages below are
    // unchanged; only the work is gone.
    let field = |name: &str| fields.iter().find(|(key, _)| key == name).map(|(_, v)| v);
    match field("_") {
        Some(v) if is_callable(v) => {}
        Some(_) => return Err(Error::Type("module descriptor _ must be callable".into())),
        None => return Err(Error::Type("module descriptor must contain _".into())),
    }
    let spec = match field("spec") {
        Some(Value::Struct(spec)) => spec.clone(),
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
    let spec_field = |name: &str| spec.iter().find(|(key, _)| key == name).map(|(_, v)| v);
    if !matches!(spec_field("documentation"), Some(Value::Str(_))) {
        return Err(Error::Type(
            "module descriptor spec.documentation must be a string".into(),
        ));
    }
    let arity = match spec_field("arity") {
        Some(Value::Int(arity)) if *arity >= 0 => *arity as usize,
        _ => {
            return Err(Error::Type(
                "module descriptor spec.arity must be an integer".into(),
            ))
        }
    };
    let types: Vec<Vec<String>> = match spec_field("type") {
        Some(Value::Null) if arity == 0 => Vec::new(),
        Some(Value::Array(types)) if arity > 0 => {
            let mut entries = Vec::with_capacity(types.len());
            for entry in types.iter() {
                // Each entry is either a single type name, or a non-empty array of
                // alternative type names (per-argument alternatives: the argument
                // independently matches any member of its set).
                let alternatives: Vec<String> = match entry {
                    Value::Str(name) => vec![name.clone()],
                    Value::Array(set) if !set.is_empty() => {
                        let names = set
                            .iter()
                            .map(|member| match member {
                                Value::Str(name) => Ok(name.clone()),
                                _ => Err(Error::Type(
                                    "module descriptor spec.type set members must be strings"
                                        .into(),
                                )),
                            })
                            .collect::<Result<Vec<_>, _>>()?;
                        // `any` is the top/wildcard constraint and must appear only as a
                        // standalone entry, never combined with alternative types in a set.
                        if names.iter().any(|name| name == "any") {
                            return Err(Error::Type(
                                "module descriptor spec.type `any` cannot be combined with alternative types"
                                    .into(),
                            ));
                        }
                        names
                    }
                    Value::Array(_) => {
                        return Err(Error::Type(
                            "module descriptor spec.type sets must not be empty".into(),
                        ))
                    }
                    _ => return Err(Error::Type(
                        "module descriptor spec.type entries must be strings or arrays of strings"
                            .into(),
                    )),
                };
                // Reject unknown names at registration (both singletons and set members)
                // instead of silently registering a declaration that can never match.
                for name in &alternatives {
                    if !SPEC_TYPE_VOCABULARY.iter().any(|known| known == name) {
                        return Err(Error::Type(format!(
                            "module descriptor spec.type entry `{name}` must name a known type"
                        )));
                    }
                }
                entries.push(alternatives);
            }
            entries
        }
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
fn validate_descriptor(fields: &Rc<Vec<(String, Value)>>, vals: &[Value]) -> Result<(), Error> {
    let (arity, types) = validate_descriptor_spec(fields)?;
    if vals.len() != arity {
        return Err(Error::Arity(format!(
            "module function expects {arity} arguments, got {}",
            vals.len()
        )));
    }
    for (index, (accepted, actual)) in types.iter().zip(vals).enumerate() {
        let actual_type = value_type(actual);
        // "any" is the top/wildcard constraint — valid only as a standalone
        // singleton entry (sets containing it are rejected at registration) —
        // and matches every type.
        let matches = accepted
            .iter()
            .any(|name| name == "any" || name == actual_type);
        if !matches {
            let expects = if accepted.len() == 1 {
                format!("expects {}", accepted[0])
            } else {
                format!("expects one of {}", accepted.join(", "))
            };
            return Err(Error::Type(format!(
                "module function argument {} {expects}, got {actual_type}",
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
    if CALL_TRACE.with(|trace| trace.borrow().len()) > MAX_CALL_DEPTH {
        // Deep or runaway recursion: trip the guard while the native stack
        // still has room, so the program gets the usual file:line:col error
        // (pointing at this call) plus the live call chain — instead of an
        // abort with no diagnostic. The span and trace below mirror what
        // invoke's own error handling would record for a body error.
        LAST_ERROR_SPAN.with(|span| *span.borrow_mut() = Some(call_span));
        CALL_TRACE.with(|trace| {
            let trace = trace.borrow();
            LAST_TRACE.with(|last| *last.borrow_mut() = trace.clone());
        });
        CALL_TRACE.with(|trace| trace.borrow_mut().pop());
        return Err(Error::Recursion(format!(
            "call depth limit ({MAX_CALL_DEPTH}) exceeded — is this function recursing without a base case?"
        ))
        .into());
    }
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
const REGEX_FLAGS: &[u8] = b"gmisxUuR";
fn parse_regex_arg(source: &str) -> Result<(Regex, bool), Error> {
    let mut flags_end = 0;
    while flags_end < source.len() && REGEX_FLAGS.contains(&source.as_bytes()[flags_end]) {
        flags_end += 1;
    }
    let (options, pattern) =
        if flags_end > 0 && flags_end < source.len() && source.as_bytes()[flags_end] == b'~' {
            (&source[..flags_end], &source[flags_end + 1..])
        } else {
            ("gmu", source)
        };
    let mut builder = RegexBuilder::new(pattern);
    for flag in options.bytes() {
        match flag {
            b'g' => {}
            b'm' => {
                builder.multi_line(true);
            }
            b'i' => {
                builder.case_insensitive(true);
            }
            b's' => {
                builder.dot_matches_new_line(true);
            }
            b'x' => {
                builder.ignore_whitespace(true);
            }
            b'U' => {
                builder.swap_greed(true);
            }
            b'u' => {
                builder.unicode(true);
            }
            b'R' => {
                builder.crlf(true);
            }
            _ => unreachable!("flag letters are validated by the leading scan"),
        }
    }
    let find_all = options.bytes().any(|flag| flag == b'g');
    let regex = builder
        .build()
        .map_err(|error| Error::Regex(format!("invalid regex: {error}")))?;
    Ok((regex, find_all))
}
fn emit_capture(
    out: &mut String,
    matches_list: &[Vec<Option<String>>],
    match_index: usize,
    capture_index: usize,
) -> Result<(), Error> {
    if matches_list.is_empty() {
        out.push('f');
        return Ok(());
    }
    let Some(cells) = matches_list.get(match_index - 1) else {
        return Err(Error::Format(format!(
            "FormatError: match index {match_index} out of range"
        )));
    };
    let Some(cell) = cells.get(capture_index - 1) else {
        return Err(Error::Format(format!(
            "FormatError: capture index {capture_index} out of range"
        )));
    };
    match cell {
        Some(capture) => out.push_str(capture),
        None => out.push('_'),
    }
    Ok(())
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
    let mut pending: Option<Vec<Vec<Option<String>>>> = None;
    while let Some(c) = it.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        let mut s = it
            .next()
            .ok_or_else(|| Flow::Error(Error::Format("trailing %".into())))?;
        if s == '~' {
            let regex_value = vs
                .get(i)
                .ok_or_else(|| Flow::Error(Error::Format("FormatArityError".into())))?;
            let text_value = vs
                .get(i + 1)
                .ok_or_else(|| Flow::Error(Error::Format("FormatArityError".into())))?;
            i += 2;
            let regex_source = match regex_value {
                Value::Str(text) => text.clone(),
                _ => {
                    return Err(Error::Format("FormatTypeError: %~ expects string".into()).into());
                }
            };
            let text = match text_value {
                Value::Str(text) => text.clone(),
                _ => {
                    return Err(Error::Format("FormatTypeError: %~ expects string".into()).into());
                }
            };
            let (regex, find_all) = parse_regex_arg(&regex_source)?;
            let cells = |captures: regex::Captures| {
                (0..captures.len())
                    .map(|index| captures.get(index).map(|cap| cap.as_str().to_owned()))
                    .collect::<Vec<Option<String>>>()
            };
            pending = Some(if find_all {
                regex.captures_iter(&text).map(cells).collect()
            } else {
                regex.captures(&text).into_iter().map(cells).collect()
            });
            continue;
        }
        let mut width: Option<usize> = None;
        if s.is_ascii_digit() {
            let mut digits = s.to_string();
            while it.peek().is_some_and(|next| next.is_ascii_digit()) {
                digits.push(it.next().expect("peeked digit must be available"));
            }
            match it.peek() {
                Some('.') => {
                    it.next();
                    let mut capture_digits = String::new();
                    while it.peek().is_some_and(|next| next.is_ascii_digit()) {
                        capture_digits.push(it.next().expect("peeked digit must be available"));
                    }
                    if capture_digits.is_empty() {
                        return Err(Error::Format("invalid capture index".into()).into());
                    }
                    let matches_list = pending.as_ref().ok_or_else(|| {
                        Flow::Error(Error::Format(
                            "FormatError: capture selector without preceding %~".into(),
                        ))
                    })?;
                    let match_index: usize = digits.parse().expect("digits are numeric");
                    if match_index < 1 {
                        return Err(Error::Format(
                            "FormatError: match index must be at least 1".into(),
                        )
                        .into());
                    }
                    let capture_index: usize = capture_digits.parse().expect("digits are numeric");
                    if capture_index < 1 {
                        return Err(Error::Format(
                            "FormatError: capture index must be at least 1".into(),
                        )
                        .into());
                    }
                    emit_capture(&mut out, matches_list, match_index, capture_index)?;
                    continue;
                }
                Some('b') | Some('h') => {
                    s = it.next().expect("peeked width specifier must be available");
                    width =
                        Some(digits.parse::<usize>().map_err(|_| {
                            Flow::Error(Error::Format("invalid binary width".into()))
                        })?);
                }
                _ => {
                    let matches_list = pending.as_ref().ok_or_else(|| {
                        Flow::Error(Error::Format(
                            "FormatError: capture selector without preceding %~".into(),
                        ))
                    })?;
                    let capture_index: usize = digits.parse().expect("digits are numeric");
                    if capture_index < 1 {
                        return Err(Error::Format(
                            "FormatError: capture index must be at least 1".into(),
                        )
                        .into());
                    }
                    emit_capture(&mut out, matches_list, 1, capture_index)?;
                    continue;
                }
            }
        }
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
            'q' => out.push_str(&lisp_quoted(v)?),
            'x' => out.push_str(&lisp_source(v)?),
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
fn float_source(value: f64) -> String {
    if value.is_nan() {
        return "NaN".into();
    }
    if value == f64::INFINITY {
        return "Inf".into();
    }
    if value == f64::NEG_INFINITY {
        return "-Inf".into();
    }
    let plain = value.to_string();
    if let Some((mantissa, exponent)) = plain.split_once(['e', 'E']) {
        let exponent: i64 = exponent.parse().expect("float exponent is numeric");
        let (sign, mantissa) = mantissa
            .strip_prefix('-')
            .map_or(("", mantissa), |rest| ("-", rest));
        let (int, frac) = mantissa
            .split_once('.')
            .map_or((mantissa, ""), |(int, frac)| (int, frac));
        let point = int.len() as i64 + exponent;
        let digits = format!("{int}{frac}");
        let text = if point <= 0 {
            format!("0.{}{}", "0".repeat((-point) as usize), digits)
        } else if point as usize >= digits.len() {
            format!(
                "{}{}.0",
                digits,
                "0".repeat((point as usize) - digits.len())
            )
        } else {
            let position = point as usize;
            format!("{}.{}", &digits[..position], &digits[position..])
        };
        format!("{sign}{text}")
    } else if plain.contains('.') {
        plain
    } else {
        format!("{plain}.0")
    }
}
pub(crate) fn json_render(v: &Value) -> Result<String, Error> {
    match v {
        Value::Null => Ok("null".into()),
        Value::Bool(value) => Ok(value.to_string()),
        Value::Int(value) => Ok(value.to_string()),
        Value::Float(value) if value.is_finite() => Ok(value.to_string()),
        Value::Float(_) => Err(Error::Format(
            "FormatTypeError: %j cannot encode non-finite float".into(),
        )),
        Value::Str(value) => Ok(json_string(value)),
        Value::Ref(loc) => json_render(&deref(&loc.root, &loc.path)?),
        Value::Array(values) => values
            .iter()
            .map(json_render)
            .collect::<Result<Vec<_>, _>>()
            .map(|values| format!("[{}]", values.join(","))),
        Value::Struct(fields) => fields
            .iter()
            .map(|(key, value)| Ok(format!("{}:{}", json_string(key), json_render(value)?)))
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
fn lisp_string(value: &str) -> String {
    let mut escaped = String::from("\"");
    for c in value.chars() {
        match c {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            c => escaped.push(c),
        }
    }
    escaped.push('\"');
    escaped
}
fn lisp_quoted(v: &Value) -> Result<String, Error> {
    match v {
        Value::Str(text) => Ok(lisp_string(text)),
        Value::Ref(loc) => lisp_quoted(&deref(&loc.root, &loc.path)?),
        _ => Err(Error::Format("FormatTypeError: %q expects string".into())),
    }
}
fn lisp_source(v: &Value) -> Result<String, Error> {
    match v {
        Value::Null => Ok("_".into()),
        Value::Bool(x) => Ok(if *x { "t" } else { "f" }.into()),
        Value::Int(x) => Ok(x.to_string()),
        Value::Float(x) => Ok(float_source(*x)),
        Value::Str(x) => Ok(lisp_string(x)),
        Value::Ref(loc) => lisp_source(&deref(&loc.root, &loc.path)?),
        Value::Array(items) => items
            .iter()
            .map(lisp_source)
            .collect::<Result<Vec<_>, Error>>()
            .map(|items| format!("[{}]", items.join(" "))),
        Value::Struct(fields) => fields
            .iter()
            .map(|(key, value)| Ok(format!("{key}:{}", lisp_source(value)?)))
            .collect::<Result<Vec<_>, Error>>()
            .map(|fields| format!("{{{}}}", fields.join(" "))),
        Value::Function(_) | Value::NativeFunction(_) => Err(Error::Format(
            "FormatTypeError: %x cannot serialize function".into(),
        )),
    }
}
fn debug_render(v: &Value) -> String {
    match v {
        Value::Null => "Null".into(),
        Value::Bool(value) => format!("Bool({value})"),
        Value::Int(value) => format!("Int({value})"),
        Value::Float(value) => format!("Float({value:?})"),
        Value::Str(value) => format!("Str({value:?})"),
        Value::Ref(loc) => match deref(&loc.root, &loc.path) {
            Ok(value) => format!("Ref({})", debug_render(&value)),
            Err(_) => "Ref(<invalid>)".into(),
        },
        Value::Array(values) => format!(
            "Array([{}])",
            values
                .iter()
                .map(debug_render)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Value::Struct(fields) => format!(
            "Struct({{{}}})",
            fields
                .iter()
                .map(|(key, value)| format!("{key}: {}", debug_render(value)))
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
            let all_equal = (0..vs.len())
                .all(|index| ((index + 1)..vs.len()).all(|other| equals(&vs[index], &vs[other])));
            Ok(Value::Bool(if name == "eq" {
                all_equal
            } else {
                !all_equal
            }))
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
    let a = match a {
        Value::Ref(loc) => match deref(&loc.root, &loc.path) {
            Ok(value) => return equals(&value, b),
            // A stale reference resolves to nothing and equals nothing.
            Err(_) => return false,
        },
        other => other,
    };
    let b = match b {
        Value::Ref(loc) => match deref(&loc.root, &loc.path) {
            Ok(value) => return equals(a, &value),
            Err(_) => return false,
        },
        other => other,
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
            x.len() == y.len() && x.iter().zip(y.iter()).all(|(a, b)| equals(a, b))
        }
        (Value::Struct(x), Value::Struct(y)) => {
            x.len() == y.len()
                && x.iter().all(|(k, v)| {
                    y.iter()
                        .find(|(q, _)| q == k)
                        .is_some_and(|(_, w)| equals(v, w))
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
        // Innermost frames first; cap the listing so a deep recursion error
        // does not dump the whole (potentially MAX_CALL_DEPTH-long) chain.
        let frames: Vec<&(String, Span)> = trace.iter().rev().collect();
        let shown = frames.len().min(40);
        for (name, frame) in frames.iter().take(shown) {
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
        if frames.len() > shown {
            out.push_str(&format!(
                "\n  … {} more frame(s) omitted",
                frames.len() - shown
            ));
        }
    }
    out
}
fn strip_shebang(source: &str) -> &str {
    match source.strip_prefix("#!") {
        Some(rest) => rest.find('\n').map_or("", |offset| &rest[offset + 1..]),
        None => source,
    }
}
fn main() {
    // The interpreter runs on a dedicated thread with a large explicit stack
    // (see MAX_CALL_DEPTH); propagate its exit code.
    std::process::exit(interpreter_main());
}

/// Runs the interpreter to completion and returns the process exit code.
///
/// The interpreter is a deep tree-walking evaluator: every Lisp call nests a
/// chain of native frames (tens of KB per level in debug builds), so
/// MAX_CALL_DEPTH levels of recursion need ~100 MB of native stack before
/// the recursion guard trips. A regular main-thread stack (8 MB) would run
/// out long before the guard — aborting with no diagnostic — so the whole
/// interpreter runs on a thread with an explicit, generous stack.
fn interpreter_main() -> i32 {
    let handle = thread::Builder::new()
        .name("small-lisp".into())
        .stack_size(INTERPRETER_STACK)
        .spawn(|| {
            if let Err(error) = install_sigint_handler() {
                let _ = writeln!(io::stderr(), "{error}");
                return 1;
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
            // The REPL's :l/:i/:bt report file-accurate line numbers, so
            // record how many leading lines (a shebang) were stripped before
            // parsing.
            let stripped_lines = if matches!(&src, Ok(s) if s.starts_with("#!")) {
                1
            } else {
                0
            };
            let src = src.map(|source| strip_shebang(&source).to_owned());
            let result = (|| -> Result<(), (Error, bool)> {
                let source = src.clone().map_err(|e| (e, false))?;
                let ts = lex(&source).map_err(|e| (e, true))?;
                let p = Parser { ts, i: 0 }.program().map_err(|e| (e, true))?;
                let e = new_env(None);
                push_source(SourceCtx {
                    label: file.to_owned(),
                    source: source.clone(),
                    line_offset: stripped_lines,
                });
                let outcome = (|| {
                    for x in p {
                        eval(&x, &e, 0, 0).map_err(|e| (flow_err(e), false))?;
                    }
                    Ok(())
                })();
                pop_source();
                outcome
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
                return 1;
            }
            0
        })
        .expect("failed to spawn the interpreter thread");
    handle.join().unwrap_or(101)
}

#[cfg(test)]
mod tests;
