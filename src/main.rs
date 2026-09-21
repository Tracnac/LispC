use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    env, fmt,
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    rc::Rc,
};

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
    Ref(Cell),
}
#[derive(Clone)]
struct Function {
    params: Vec<String>,
    body: Expr,
    env: EnvRef,
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

#[derive(Clone, Debug)]
enum Expr {
    Lit(Literal),
    Symbol(String),
    Field(Box<Expr>, String),
    Ref(Box<Expr>),
    Array(Vec<Expr>),
    Struct(Vec<(String, Expr)>),
    Call(Box<Expr>, Vec<Expr>),
    Block(Vec<Expr>),
}
#[derive(Clone, Debug)]
enum Literal {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
}

#[derive(Debug)]
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
        }
    }
}
type EResult = Result<Value, Flow>;
enum Flow {
    Error(Error),
    Break(Value),
    Continue,
}
impl From<Error> for Flow {
    fn from(e: Error) -> Self {
        Flow::Error(e)
    }
}

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    LParen,
    RParen,
    LBrack,
    RBrack,
    LBrace,
    RBrace,
    Colon,
    Dot,
    Caret,
    Symbol(String),
    Str(String),
    Int(i64),
    Float(f64),
}

fn lex(src: &str) -> Result<Vec<Tok>, Error> {
    let mut out = Vec::new();
    let cs: Vec<char> = src.chars().collect();
    let mut i = 0;
    while i < cs.len() {
        let c = cs[i];
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
            '(' => Some(Tok::LParen),
            ')' => Some(Tok::RParen),
            '[' => Some(Tok::LBrack),
            ']' => Some(Tok::RBrack),
            '{' => Some(Tok::LBrace),
            '}' => Some(Tok::RBrace),
            ':' => Some(Tok::Colon),
            '.' => Some(Tok::Dot),
            '^' => Some(Tok::Caret),
            _ => None,
        };
        if let Some(t) = one {
            out.push(t);
            i += 1;
            continue;
        }
        if matches!(c, '<' | '>' | '$' | '~') {
            out.push(Tok::Symbol(c.to_string()));
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
            out.push(Tok::Str(s));
            continue;
        }
        // A dot between digits belongs to a float; other dots remain field
        // separators, so `1.0` and `profile.1` can coexist.
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
                out.push(number(&s)?.expect("decimal float is a number"));
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
            out.push(t)
        } else {
            out.push(Tok::Symbol(s))
        }
    }
    Ok(out)
}
fn number(s: &str) -> Result<Option<Tok>, Error> {
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
        return Ok(Some(Tok::Int(
            v.checked_mul(sign)
                .ok_or_else(|| Error::Parse("integer out of range".into()))?,
        )));
    }
    if rest.chars().all(|c| c.is_ascii_digit()) && !rest.is_empty() {
        return s
            .parse::<i64>()
            .map(|v| Some(Tok::Int(v)))
            .map_err(|_| Error::Parse(format!("integer out of range `{s}`")));
    }
    if rest.contains('.') {
        return s
            .parse::<f64>()
            .map(|v| Some(Tok::Float(v)))
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
        match self
            .take()
            .ok_or_else(|| Error::Parse("unexpected end".into()))?
        {
            Tok::LParen => self.paren(),
            Tok::LBrack => {
                let mut v = vec![];
                while self.peek() != Some(&Tok::RBrack) {
                    if self.peek().is_none() {
                        return Err(Error::Parse("unclosed array".into()));
                    }
                    v.push(self.form()?)
                }
                self.take();
                Ok(Expr::Array(v))
            }
            Tok::LBrace => self.struct_(),
            Tok::Caret => Ok(Expr::Ref(Box::new(self.atom_field()?))),
            Tok::Str(s) => Ok(Expr::Lit(Literal::Str(s))),
            Tok::Int(n) => Ok(Expr::Lit(Literal::Int(n))),
            Tok::Float(n) => Ok(Expr::Lit(Literal::Float(n))),
            Tok::Symbol(s) => {
                let e = match s.as_str() {
                    "t" => Expr::Lit(Literal::Bool(true)),
                    "f" => Expr::Lit(Literal::Bool(false)),
                    "_" => Expr::Lit(Literal::Null),
                    "NaN" => Expr::Lit(Literal::Float(f64::NAN)),
                    "Inf" => Expr::Lit(Literal::Float(f64::INFINITY)),
                    "-Inf" => Expr::Lit(Literal::Float(f64::NEG_INFINITY)),
                    _ => Expr::Symbol(s),
                };
                self.field_tail(e)
            }
            x => Err(Error::Parse(format!("unexpected token {x:?}"))),
        }
    }
    fn atom_field(&mut self) -> Result<Expr, Error> {
        self.form()
    }
    fn field_tail(&mut self, mut e: Expr) -> Result<Expr, Error> {
        while self.peek() == Some(&Tok::Dot) {
            self.take();
            let key = match self.take() {
                Some(Tok::Symbol(x)) => x,
                Some(Tok::Int(n)) => n.to_string(),
                _ => return Err(Error::Parse("expected field name after dot".into())),
            };
            e = Expr::Field(Box::new(e), key)
        }
        Ok(e)
    }
    fn paren(&mut self) -> Result<Expr, Error> {
        if self.peek() == Some(&Tok::RParen) {
            self.take();
            return Ok(Expr::Block(vec![]));
        }
        let first = self.form()?;
        let mut rest = vec![];
        while self.peek() != Some(&Tok::RParen) {
            if self.peek().is_none() {
                return Err(Error::Parse("unclosed parenthesis".into()));
            }
            rest.push(self.form()?)
        }
        self.take();
        match first {
            Expr::Symbol(_) | Expr::Field(_, _) => Ok(Expr::Call(Box::new(first), rest)),
            _ => {
                let mut all = vec![first];
                all.extend(rest);
                Ok(Expr::Block(all))
            }
        }
    }
    fn struct_(&mut self) -> Result<Expr, Error> {
        let mut v = vec![];
        while self.peek() != Some(&Tok::RBrace) {
            let key = match self.take() {
                Some(Tok::Str(x)) => x,
                _ => return Err(Error::Parse("struct key must be string".into())),
            };
            if self.take() != Some(Tok::Colon) {
                return Err(Error::Parse("expected : after struct key".into()));
            }
            let val = self.form()?;
            v.push((key, val));
        }
        self.take();
        Ok(Expr::Struct(v))
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
fn follow(mut c: Cell) -> Cell {
    loop {
        let v = c.borrow().clone();
        if let Value::Ref(next) = v {
            c = next
        } else {
            return c;
        }
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
                .map(|x| render(&x.borrow()))
                .collect::<Vec<_>>()
                .join(" ")
        ),
        Value::Struct(s) => format!(
            "{{{}}}",
            s.borrow()
                .iter()
                .map(|(k, v)| format!("\"{k}\":{}", render(&v.borrow())))
                .collect::<Vec<_>>()
                .join(" ")
        ),
        Value::Function(_) => "<fn>".into(),
    }
}
fn location(e: &Expr, env: &EnvRef) -> Result<Cell, Error> {
    match e {
        Expr::Symbol(n) => lookup(env, n).ok_or_else(|| Error::Name(n.clone())),
        Expr::Field(base, key) => {
            let c = follow(location(base, env)?);
            let result = match &*c.borrow() {
                Value::Array(a) => {
                    let n = key_parse(key)?;
                    a.borrow()
                        .get(n - 1)
                        .cloned()
                        .ok_or_else(|| Error::Name(format!("array index {n}")))
                }
                Value::Struct(s) => s
                    .borrow()
                    .iter()
                    .find(|(k, _)| k == key)
                    .map(|(_, v)| v.clone())
                    .ok_or_else(|| Error::Name(format!("field {key}"))),
                _ => Err(Error::Type("field access requires array or struct".into())),
            };
            result
        }
        _ => Err(Error::Type(
            "reference target must be a variable or field".into(),
        )),
    }
}
fn key_parse(key: &str) -> Result<usize, Error> {
    let n = key
        .parse::<usize>()
        .map_err(|_| Error::Type("array index must be a positive integer".into()))?;
    if n == 0 {
        Err(Error::Type("array indices are 1-based".into()))
    } else {
        Ok(n)
    }
}

fn eval(e: &Expr, env: &EnvRef, loop_depth: usize, match_depth: usize) -> EResult {
    match e {
        Expr::Lit(x) => Ok(match x {
            Literal::Null => Value::Null,
            Literal::Bool(b) => Value::Bool(*b),
            Literal::Int(n) => Value::Int(*n),
            Literal::Float(n) => Value::Float(*n),
            Literal::Str(s) => Value::Str(s.clone()),
        }),
        Expr::Symbol(n) => lookup(env, n)
            .map(|c| copy(&c.borrow()))
            .ok_or_else(|| Flow::Error(Error::Name(n.clone()))),
        Expr::Field(_, _) => location(e, env)
            .map(|c| copy(&c.borrow()))
            .map_err(Into::into),
        Expr::Ref(x) => location(x, env).map(Value::Ref).map_err(Into::into),
        Expr::Array(xs) => {
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
        Expr::Struct(xs) => {
            let mut seen = HashSet::new();
            let mut v = vec![];
            for (k, x) in xs {
                if !seen.insert(k) {
                    return Err(Error::DuplicateKey(k.clone()).into());
                }
                v.push((
                    k.clone(),
                    Rc::new(RefCell::new(eval(x, env, loop_depth, match_depth)?)),
                ))
            }
            Ok(Value::Struct(Rc::new(RefCell::new(v))))
        }
        Expr::Block(xs) => {
            let child = new_env(Some(env.clone()));
            let mut r = Value::Null;
            for x in xs {
                r = eval(x, &child, loop_depth, match_depth)?
            }
            Ok(r)
        }
        Expr::Call(head, args) => call(head, args, env, loop_depth, match_depth),
    }
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
fn call(head: &Expr, args: &[Expr], env: &EnvRef, l: usize, m: usize) -> EResult {
    if let Expr::Symbol(name) = head {
        match name.as_str() {
            "let" => {
                need(args, 2, "let")?;
                let n = if let Expr::Symbol(n) = &args[0] {
                    n
                } else {
                    return Err(Error::Type("let name must be an identifier".into()).into());
                };
                let v = eval(&args[1], env, l, m)?;
                env.borrow_mut()
                    .values
                    .push((n.clone(), Rc::new(RefCell::new(v))));
                return Ok(Value::Null);
            }
            "set" => {
                need(args, 2, "set")?;
                let c = location(&args[0], env).map_err(Flow::Error)?;
                let v = eval(&args[1], env, l, m)?;
                *follow(c).borrow_mut() = v;
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
                let ps = match &args[0] {
                    Expr::Block(xs) => xs
                        .iter()
                        .map(|x| {
                            if let Expr::Symbol(s) = x {
                                Ok(s.clone())
                            } else {
                                Err(Error::Type("function parameter must be identifier".into()))
                            }
                        })
                        .collect::<Result<Vec<_>, _>>()
                        .map_err(Flow::Error)?,
                    Expr::Call(h, xs) => {
                        let mut all = vec![*h.clone()];
                        all.extend(xs.clone());
                        all.iter()
                            .map(|x| {
                                if let Expr::Symbol(s) = x {
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
                }));
                if args.len() == 2 {
                    return Ok(fun);
                }
                return invoke(fun, values(&args[2..], env, l, m)?);
            }
            "loop" => {
                let local = new_env(Some(env.clone()));
                loop {
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
                if args.len() % 2 != 0 {
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
            "io/open" => return io_open(args, env, l, m),
            "io/close" => return io_close(args, env, l, m),
            "io/read" => return io_read(args, env, l, m),
            "io/write" => return io_write(args, env, l, m),
            "$" => return format_value(args, env, l, m),
            "~" => {
                need(args, 2, "~")?;
                let p = as_str(eval(&args[0], env, l, m)?)?;
                let text = render(&eval(&args[1], env, l, m)?);
                return Ok(Value::Bool(regex_match(&p, &text).map_err(Flow::Error)?));
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
    let fun = eval(head, env, l, m)?;
    invoke(fun, values(args, env, l, m)?)
}
fn invoke(f: Value, vals: Vec<Value>) -> EResult {
    let f = match f {
        Value::Function(f) => f,
        Value::Ref(c) => return invoke(copy(&c.borrow()), vals),
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
    eval(&f.body, &e, 0, 0)
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
    let mut bytes = Vec::new();
    let mut byte = [0; 1];
    loop {
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
    let mut line = String::new();
    let bytes = io::stdin()
        .read_line(&mut line)
        .map_err(|e| Error::Io(e.to_string()))?;
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
                .map(|(key, value)| format!("{key:?}: {}", debug_render(&value.borrow())))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Value::Function(function) => format!("Function({})", function.params.join(", ")),
    }
}
fn regex_match(p: &str, text: &str) -> Result<bool, Error> {
    if p.contains(|c| matches!(c, '(' | ')' | '[' | ']' | '|' | '\\')) {
        return Err(Error::Regex("unsupported regex construct".into()));
    }
    let anchored_start = p.starts_with('^');
    let anchored_end = p.ends_with('$') && !p.ends_with("\\$");
    let core = &p[if anchored_start { 1 } else { 0 }..p.len() - if anchored_end { 1 } else { 0 }];
    let a: Vec<char> = core.chars().collect();
    fn go(a: &[char], s: &[char]) -> bool {
        if a.is_empty() {
            return true;
        }
        let c = a[0];
        if c == '*' || c == '+' || c == '?' || c == '^' || c == '$' {
            return false;
        }
        let q = if a.len() > 1 { a[1] } else { ' ' };
        let hit = |x: char| c == '.' || c == x;
        if q == '*' {
            let mut n = 0;
            while n < s.len() && hit(s[n]) {
                n += 1
            }
            return (0..=n).rev().any(|k| go(&a[2..], &s[k..]));
        }
        if q == '+' {
            let mut n = 0;
            while n < s.len() && hit(s[n]) {
                n += 1
            }
            return n > 0 && (1..=n).rev().any(|k| go(&a[2..], &s[k..]));
        }
        if q == '?' {
            return go(&a[2..], s) || (!s.is_empty() && hit(s[0]) && go(&a[2..], &s[1..]));
        }
        !s.is_empty() && hit(s[0]) && go(&a[1..], &s[1..])
    }
    let ss: Vec<char> = text.chars().collect();
    if anchored_start {
        let ok = go(&a, &ss);
        return Ok(ok && (!anchored_end || match_len(&a, &ss)));
    }
    for i in 0..=ss.len() {
        if go(&a, &ss[i..]) {
            if !anchored_end || match_len(&a, &ss[i..]) {
                return Ok(true);
            }
        }
    }
    Ok(false)
}
fn match_len(a: &[char], s: &[char]) -> bool {
    fn rec(a: &[char], s: &[char]) -> bool {
        if a.is_empty() {
            return s.is_empty();
        }
        let c = a[0];
        let hit = |x: char| c == '.' || c == x;
        let q = a.get(1).copied();
        match q {
            Some('*') => {
                let mut n = 0;
                while n < s.len() && hit(s[n]) {
                    n += 1
                }
                (0..=n).rev().any(|k| rec(&a[2..], &s[k..]))
            }
            Some('+') => {
                let mut n = 0;
                while n < s.len() && hit(s[n]) {
                    n += 1
                }
                n > 0 && (1..=n).rev().any(|k| rec(&a[2..], &s[k..]))
            }
            Some('?') => rec(&a[2..], s) || (!s.is_empty() && hit(s[0]) && rec(&a[2..], &s[1..])),
            _ => !s.is_empty() && hit(s[0]) && rec(&a[1..], &s[1..]),
        }
    }
    rec(a, s)
}

fn numeric(v: Value) -> Result<(Option<i64>, f64), Flow> {
    match v {
        Value::Int(n) => Ok((Some(n), n as f64)),
        Value::Float(n) => Ok((None, n)),
        _ => Err(Error::Type("expected number".into()).into()),
    }
}
fn builtin(name: &str, vs: Vec<Value>) -> Result<Value, Error> {
    if [
        "add", "sub", "mul", "div", "mod", "pow", "eq", "ne", "lt", "gt", "le", "ge", "bit-and",
        "bit-or", "bit-xor", "bit-not", "bit-shl", "bit-shr",
    ]
    .contains(&name)
        && vs.len() != if name == "bit-not" { 1 } else { 2 }
    {
        return Err(Error::Arity(format!(
            "{name} expects {} arguments, got {}",
            if name == "bit-not" { 1 } else { 2 },
            vs.len()
        )));
    }
    let a = vs[0].clone();
    if name == "bit-not" {
        return Ok(Value::Int(!integer_value(a)?));
    }
    let b = vs[1].clone();
    if matches!(
        name,
        "bit-and" | "bit-or" | "bit-xor" | "bit-shl" | "bit-shr"
    ) {
        let x = integer_value(a)?;
        let y = integer_value(b)?;
        return match name {
            "bit-and" => Ok(Value::Int(x & y)),
            "bit-or" => Ok(Value::Int(x | y)),
            "bit-xor" => Ok(Value::Int(x ^ y)),
            "bit-shl" | "bit-shr" => {
                if !(0..64).contains(&y) {
                    return Err(Error::Math("InvalidShiftCount".into()));
                }
                let shift = y as u32;
                Ok(Value::Int(if name == "bit-shl" {
                    x << shift
                } else {
                    x >> shift
                }))
            }
            _ => unreachable!(),
        };
    }
    if matches!(name, "eq" | "ne") {
        let x = equals(&a, &b);
        return Ok(Value::Bool(if name == "eq" { x } else { !x }));
    }
    if matches!(name, "lt" | "gt" | "le" | "ge") {
        let (_, x) = numeric(a).map_err(flow_err)?;
        let (_, y) = numeric(b).map_err(flow_err)?;
        return Ok(Value::Bool(match name {
            "lt" => x < y,
            "gt" => x > y,
            "le" => x <= y,
            _ => x >= y,
        }));
    }
    let (ai, af) = numeric(a).map_err(flow_err)?;
    let (bi, bf) = numeric(b).map_err(flow_err)?;
    if name == "div" {
        if let (Some(x), Some(y)) = (ai, bi) {
            if y == 0 {
                return Err(Error::Math("DivisionByZero".into()));
            }
            return Ok(Value::Int(x / y));
        }
        if bf == 0.0 {
            return Err(Error::Math("DivisionByZero".into()));
        }
        return Ok(Value::Float(af / bf));
    }
    if name == "mod" {
        let (Some(x), Some(y)) = (ai, bi) else {
            return Err(Error::Type("mod requires integers".into()));
        };
        if y == 0 {
            return Err(Error::Math("DivisionByZero".into()));
        }
        return Ok(Value::Int(x % y));
    }
    if ai.is_none() || bi.is_none() {
        return Ok(Value::Float(match name {
            "add" => af + bf,
            "sub" => af - bf,
            "mul" => af * bf,
            "pow" => af.powf(bf),
            _ => unreachable!(),
        }));
    }
    let (x, y) = (ai.unwrap(), bi.unwrap());
    let r = match name {
        "add" => x.checked_add(y),
        "sub" => x.checked_sub(y),
        "mul" => x.checked_mul(y),
        "pow" => x.checked_pow(y as u32),
        _ => None,
    };
    r.map(Value::Int)
        .ok_or_else(|| Error::Math("IntegerOverflow".into()))
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
fn main() {
    let args: Vec<String> = env::args().collect();
    let src = if args.len() > 1 {
        fs::read_to_string(&args[1]).map_err(|e| Error::Io(e.to_string()))
    } else {
        let mut s = String::new();
        io::stdin()
            .read_line(&mut s)
            .map(|_| s)
            .map_err(|e| Error::Io(e.to_string()))
    };
    let result = (|| -> Result<(), Error> {
        let ts = lex(&src?)?;
        let p = Parser { ts, i: 0 }.program()?;
        let e = new_env(None);
        for x in p {
            eval(&x, &e, 0, 0).map_err(flow_err)?;
        }
        Ok(())
    })();
    if let Err(e) = result {
        let _ = writeln!(io::stderr(), "{e}");
        std::process::exit(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(src: &str) -> Result<Value, Error> {
        let tokens = lex(src)?;
        let program = Parser { ts: tokens, i: 0 }.program()?;
        let env = new_env(None);
        let mut result = Value::Null;
        for form in program {
            result = eval(&form, &env, 0, 0).map_err(flow_err)?;
        }
        Ok(result)
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
    fn references_mutate_the_original_location() {
        let value =
            run("(let a [1 2]) (let setzero (fn (x) (set x.1 0))) (setzero ^a) a.1").unwrap();
        assert!(matches!(value, Value::Int(0)));
    }

    #[test]
    fn duplicate_struct_key_is_an_error() {
        assert!(matches!(
            run("{\"x\": 1 \"x\": 2}"),
            Err(Error::DuplicateKey(_))
        ));
    }

    #[test]
    fn restricted_regex_works() {
        let value = run("(~ \"^a.+z$\" \"abz\")").unwrap();
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

        let value = run("($ \"%j\" {\"name\": \"Ada\" \"values\": [1 t _]})").unwrap();
        assert!(
            matches!(value, Value::Str(text) if text == r#"{"name":"Ada","values":[1,true,null]}"#)
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

        let value = run("(let values [10]) values.1").unwrap();
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
        assert!(matches!(
            run("(bit-and 1 (div 1.0 2))"),
            Err(Error::Type(message)) if message == "bitwise operations require integers"
        ));
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
}
