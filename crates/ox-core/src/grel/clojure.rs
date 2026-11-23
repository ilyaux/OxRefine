//! `clojure:` expressions. OpenRefine 3.10.1 compiles them with Clojure 1.12.3 as
//! `(fn [value cell cells row rowIndex value1 value2] <expression>)`. This is an interpreter for the
//! part of Clojure such expressions use: the reader, core special forms and macros, the numeric tower
//! (long overflow, ratios, BigInt, BigDecimal), ~150 clojure.core and clojure.string functions, and
//! Java interop on strings, numbers, Math, Character and OpenRefine's cell objects. Symbols resolve at
//! compile time, as in Clojure, so an unknown name is a syntax error. Sequences are eager: infinite
//! ones (`(range)`, `(iterate f x)`) are refused. Pinned to OpenRefine by tests/langs.
use super::{
    eval::Row,
    parse::java_regex,
    value::{fmt_date, java_double, Class, Value},
};
use chrono::NaiveDateTime;
use regex::Regex;
use std::{cell::RefCell, collections::HashMap, rc::Rc};

type R<T> = Result<T, String>;

#[derive(Clone)]
pub enum C {
    Nil,
    Bool(bool),
    Long(i64),
    Double(f64),
    /// clojure.lang.BigInt (bounded to i128 here)
    Big(i128),
    /// clojure.lang.Ratio, normalised, denominator > 1
    Ratio(i128, i128),
    Dec(bigdecimal::BigDecimal),
    Char(char),
    Str(Rc<str>),
    Kw(Rc<str>),
    Sym(Rc<str>),
    List(Rc<Vec<C>>),
    Vec(Rc<Vec<C>>),
    Map(Rc<Vec<(C, C)>>),
    Set(Rc<Vec<C>>),
    Re(Rc<Regex>, Rc<str>),
    Fn(Rc<Lambda>),
    /// A clojure.core / clojure.string function or a Java static method, by name.
    Builtin(&'static str),
    /// A var created by `def`, looked up when evaluated.
    Var(Rc<str>),
    Obj(Rc<Obj>),
    /// `recur` arguments travelling back to the enclosing loop or fn.
    Recur(Rc<Vec<C>>),
}

pub enum Obj {
    /// com.google.refine.expr.WrappedCell
    Cell(Value),
    /// CellTuple: each column's cell, None when the cell is null
    Cells(Vec<(String, Option<C>)>),
    Row { index: i64, starred: bool, flagged: bool, cells: C },
    Date(NaiveDateTime),
    Exception(String, String),
    Class(String),
    VarRef(String),
}

pub struct Lambda {
    name: Option<Rc<str>>,
    arities: Vec<Arity>,
    env: Env,
}

struct Arity {
    params: Vec<Rc<str>>,
    rest: Option<Rc<str>>,
    body: Vec<C>,
}

type Env = Option<Rc<Frame>>;
struct Frame {
    name: Rc<str>,
    value: C,
    parent: Env,
}

fn bind(env: &Env, name: Rc<str>, value: C) -> Env {
    Some(Rc::new(Frame { name, value, parent: env.clone() }))
}

fn lookup(env: &Env, name: &str) -> Option<C> {
    let mut e = env.as_ref();
    while let Some(f) = e {
        if &*f.name == name {
            return Some(f.value.clone());
        }
        e = f.parent.as_ref();
    }
    None
}

fn s(x: &str) -> C {
    C::Str(x.into())
}
fn sym(x: &str) -> C {
    C::Sym(x.into())
}
fn list(items: Vec<C>) -> C {
    C::List(Rc::new(items))
}

// ------------------------------------------------------------------ reader

struct Reader<'a> {
    src: &'a [char],
    pos: usize,
    anon: Option<usize>,
}

fn syntax(msg: &str) -> String {
    format!("Syntax error reading source: {msg}")
}

impl Reader<'_> {
    fn skip(&mut self) {
        while let Some(&c) = self.src.get(self.pos) {
            if c.is_whitespace() || c == ',' {
                self.pos += 1;
            } else if c == ';' {
                while self.src.get(self.pos).is_some_and(|&c| c != '\n') {
                    self.pos += 1;
                }
            } else {
                break;
            }
        }
    }
    fn peek(&self) -> Option<char> {
        self.src.get(self.pos).copied()
    }
    fn read_all(&mut self) -> R<Vec<C>> {
        let mut out = vec![];
        loop {
            self.skip();
            if self.peek().is_none() {
                return Ok(out);
            }
            if let Some(f) = self.read()? {
                out.push(f);
            }
        }
    }
    fn read_until(&mut self, end: char) -> R<Vec<C>> {
        let mut out = vec![];
        loop {
            self.skip();
            match self.peek() {
                None => return Err(syntax("EOF while reading")),
                Some(c) if c == end => {
                    self.pos += 1;
                    return Ok(out);
                }
                Some(')' | ']' | '}') => return Err(syntax("Unmatched delimiter")),
                _ => {
                    if let Some(f) = self.read()? {
                        out.push(f);
                    }
                }
            }
        }
    }
    fn token(&mut self) -> String {
        let start = self.pos;
        while let Some(&c) = self.src.get(self.pos) {
            if c.is_whitespace() || matches!(c, ',' | '(' | ')' | '[' | ']' | '{' | '}' | '"' | ';') {
                break;
            }
            self.pos += 1;
        }
        self.src[start..self.pos].iter().collect()
    }
    /// One form; `None` for `#_` discards.
    fn read(&mut self) -> R<Option<C>> {
        self.skip();
        let c = self.peek().ok_or_else(|| syntax("EOF"))?;
        self.pos += 1;
        Ok(Some(match c {
            '(' => list(self.read_until(')')?),
            '[' => C::Vec(Rc::new(self.read_until(']')?)),
            '{' => {
                let items = self.read_until('}')?;
                if items.len() % 2 == 1 {
                    return Err(syntax("Map literal must contain an even number of forms"));
                }
                C::Map(Rc::new(items.chunks(2).map(|p| (p[0].clone(), p[1].clone())).collect()))
            }
            ')' | ']' | '}' => return Err(syntax("Unmatched delimiter")),
            '"' => s(&self.string()?),
            '\'' => list(vec![sym("quote"), self.read()?.ok_or_else(|| syntax("EOF"))?]),
            '@' => list(vec![sym("deref"), self.read()?.ok_or_else(|| syntax("EOF"))?]),
            '^' => {
                self.read()?;
                return self.read();
            }
            '\\' => C::Char(self.character()?),
            '#' => match self.peek() {
                Some('{') => {
                    self.pos += 1;
                    C::Set(Rc::new(dedupe(self.read_until('}')?)))
                }
                Some('"') => {
                    self.pos += 1;
                    let p = self.raw_string()?;
                    regex(&p).map_err(|e| syntax(&e))?
                }
                Some('_') => {
                    self.pos += 1;
                    self.read()?;
                    return Ok(None);
                }
                Some('(') => {
                    self.pos += 1;
                    if self.anon.is_some() {
                        return Err(syntax("Nested #()s are not allowed"));
                    }
                    self.anon = Some(0);
                    let body = self.read_until(')');
                    let n = self.anon.take().unwrap();
                    let params = (1..=n).map(|i| sym(&format!("%{i}"))).collect();
                    list(vec![sym("fn*"), C::Vec(Rc::new(params)), list(body?)])
                }
                _ => return Err(syntax("No dispatch macro")),
            },
            _ => {
                self.pos -= 1;
                let t = self.token();
                atom(&t, &mut self.anon)?
            }
        }))
    }
    fn string(&mut self) -> R<String> {
        let mut out = String::new();
        loop {
            let c = self.peek().ok_or_else(|| syntax("EOF while reading string"))?;
            self.pos += 1;
            match c {
                '"' => return Ok(out),
                '\\' => {
                    let e = self.peek().ok_or_else(|| syntax("EOF while reading string"))?;
                    self.pos += 1;
                    out.push(match e {
                        'n' => '\n',
                        't' => '\t',
                        'r' => '\r',
                        'b' => '\u{8}',
                        'f' => '\u{c}',
                        '0' => '\0',
                        'u' => {
                            let hex: String = self.src.get(self.pos..self.pos + 4).ok_or_else(|| syntax("bad \\u escape"))?.iter().collect();
                            self.pos += 4;
                            char::from_u32(u32::from_str_radix(&hex, 16).map_err(|_| syntax("bad \\u escape"))?).ok_or_else(|| syntax("bad \\u escape"))?
                        }
                        '"' | '\\' => e,
                        _ => return Err(syntax(&format!("Unsupported escape character: \\{e}"))),
                    });
                }
                c => out.push(c),
            }
        }
    }
    fn raw_string(&mut self) -> R<String> {
        let mut out = String::new();
        loop {
            let c = self.peek().ok_or_else(|| syntax("EOF while reading regex"))?;
            self.pos += 1;
            match c {
                '"' => return Ok(out),
                '\\' => {
                    let e = self.peek().ok_or_else(|| syntax("EOF while reading regex"))?;
                    self.pos += 1;
                    if e != '"' {
                        out.push('\\');
                    }
                    out.push(e);
                }
                c => out.push(c),
            }
        }
    }
    fn character(&mut self) -> R<char> {
        let first = self.peek().ok_or_else(|| syntax("EOF while reading character"))?;
        self.pos += 1;
        let mut name = first.to_string();
        while let Some(&c) = self.src.get(self.pos) {
            if c.is_whitespace() || matches!(c, ',' | '(' | ')' | '[' | ']' | '{' | '}' | '"' | ';' | '\\') {
                break;
            }
            name.push(c);
            self.pos += 1;
        }
        if name.chars().count() == 1 {
            return Ok(first);
        }
        Ok(match name.as_str() {
            "newline" => '\n',
            "space" => ' ',
            "tab" => '\t',
            "return" => '\r',
            "backspace" => '\u{8}',
            "formfeed" => '\u{c}',
            n if n.starts_with('u') && n.len() == 5 => char::from_u32(u32::from_str_radix(&n[1..], 16).map_err(|_| syntax("bad char"))?).ok_or_else(|| syntax("bad char"))?,
            _ => return Err(syntax(&format!("Unsupported character: \\{name}"))),
        })
    }
}

fn atom(t: &str, anon: &mut Option<usize>) -> R<C> {
    if let Some(n) = number(t) {
        return n;
    }
    Ok(match t {
        "nil" => C::Nil,
        "true" => C::Bool(true),
        "false" => C::Bool(false),
        _ if t.starts_with("::") => return Err(syntax("Invalid token")),
        _ if t.starts_with(':') && t.len() > 1 => C::Kw(t[1..].into()),
        _ if t.starts_with('%') && anon.is_some() => {
            let n = if t == "%" { 1 } else { t[1..].parse::<usize>().map_err(|_| syntax("arg literal must be %, %& or %integer"))? };
            let a = anon.as_mut().unwrap();
            *a = (*a).max(n);
            sym(&format!("%{n}"))
        }
        _ => sym(t),
    })
}

fn number(t: &str) -> Option<R<C>> {
    let body = t.strip_prefix(['+', '-']).unwrap_or(t);
    if !body.starts_with(|c: char| c.is_ascii_digit()) {
        return None;
    }
    let neg = t.starts_with('-');
    let bad = || Some(Err(syntax(&format!("Invalid number: {t}"))));
    if let Some((a, b)) = body.split_once('/') {
        let (Ok(a), Ok(b)) = (a.parse::<i128>(), b.parse::<i128>()) else { return bad() };
        return Some(ratio(if neg { -a } else { a }, b, false));
    }
    if let Some(d) = body.strip_suffix('M') {
        return Some(t[..t.len() - 1].parse::<bigdecimal::BigDecimal>().map(C::Dec).map_err(|_| syntax("Invalid number")).and_then(|v| if d.is_empty() { Err(syntax("Invalid number")) } else { Ok(v) }));
    }
    if body.contains(['.', 'e', 'E']) && !body.starts_with("0x") {
        return Some(t.parse::<f64>().map(C::Double).map_err(|_| syntax(&format!("Invalid number: {t}"))));
    }
    let (digits, big) = match body.strip_suffix('N') {
        Some(d) => (d, true),
        None => (body, false),
    };
    let v = if let Some(h) = digits.strip_prefix("0x").or_else(|| digits.strip_prefix("0X")) {
        i128::from_str_radix(h, 16)
    } else if digits.len() > 1 && digits.starts_with('0') {
        i128::from_str_radix(&digits[1..], 8)
    } else {
        digits.parse::<i128>()
    };
    let Ok(v) = v else { return bad() };
    let v = if neg { -v } else { v };
    Some(Ok(if big { C::Big(v) } else { int(v) }))
}

/// A Long when it fits, else a BigInt (as the reader and promoting ops do).
fn int(v: i128) -> C {
    i64::try_from(v).map(C::Long).unwrap_or(C::Big(v))
}

fn ratio(n: i128, d: i128, big: bool) -> R<C> {
    if d == 0 {
        return Err("Divide by zero".into());
    }
    let g = gcd(n.abs(), d.abs()).max(1);
    let (mut n, mut d) = (n / g, d / g);
    if d < 0 {
        n = -n;
        d = -d;
    }
    Ok(if d == 1 { if big { C::Big(n) } else { int(n) } } else { C::Ratio(n, d) })
}

fn gcd(a: i128, b: i128) -> i128 {
    if b == 0 { a } else { gcd(b, a % b) }
}

fn regex(p: &str) -> R<C> {
    let re = Regex::new(&java_regex(p)).map_err(|e| e.to_string())?;
    Ok(C::Re(Rc::new(re), p.into()))
}

fn dedupe(items: Vec<C>) -> Vec<C> {
    let mut out: Vec<C> = vec![];
    for i in items {
        if !out.iter().any(|o| equiv(o, &i)) {
            out.push(i);
        }
    }
    out
}

// ------------------------------------------------------------------ analysis: macros and symbol resolution

const SPECIALS: &[&str] = &["quote", "if", "do", "let*", "fn*", "loop*", "recur", "def", "throw", "try", "case*", ".", "new", "var"];

struct Analyzer {
    gensym: usize,
    defs: Vec<Rc<str>>,
}

fn compile_error(what: &str) -> String {
    format!("Syntax error compiling: Unable to resolve symbol: {what} in this context")
}

impl Analyzer {
    fn gen(&mut self, base: &str) -> Rc<str> {
        self.gensym += 1;
        format!("{base}__{}__auto__", self.gensym).into()
    }

    fn body(&mut self, forms: &[C], scope: &[Rc<str>]) -> R<Vec<C>> {
        forms.iter().map(|f| self.analyze(f, scope)).collect()
    }

    fn analyze(&mut self, f: &C, scope: &[Rc<str>]) -> R<C> {
        match f {
            C::Sym(name) => self.symbol(name, scope),
            C::Vec(items) => Ok(C::Vec(Rc::new(self.body(items, scope)?))),
            C::Set(items) => Ok(C::Set(Rc::new(self.body(items, scope)?))),
            C::Map(pairs) => Ok(C::Map(Rc::new(pairs.iter().map(|(k, v)| Ok((self.analyze(k, scope)?, self.analyze(v, scope)?))).collect::<R<_>>()?))),
            C::List(items) if !items.is_empty() => self.form(items, scope),
            _ => Ok(f.clone()),
        }
    }

    fn symbol(&mut self, name: &Rc<str>, scope: &[Rc<str>]) -> R<C> {
        if scope.iter().rev().any(|s| s == name) {
            return Ok(C::Sym(name.clone()));
        }
        if self.defs.iter().any(|d| d == name) || GLOBALS.with(|g| g.borrow().contains_key(&**name)) {
            return Ok(C::Var(name.clone()));
        }
        let plain = name.strip_prefix("clojure.core/").unwrap_or(name);
        if let Some(b) = builtin(plain) {
            return Ok(C::Builtin(b));
        }
        if let Some(v) = static_field(name) {
            return Ok(v);
        }
        if let Some(c) = class_name(name) {
            return Ok(C::Obj(Rc::new(Obj::Class(c.into()))));
        }
        Err(compile_error(name))
    }

    fn form(&mut self, items: &[C], scope: &[Rc<str>]) -> R<C> {
        let head = match &items[0] {
            C::Sym(h) if !scope.iter().any(|s| s == h) => h.clone(),
            _ => {
                let mut out = vec![self.analyze(&items[0], scope)?];
                out.extend(self.body(&items[1..], scope)?);
                return Ok(list(out));
            }
        };
        let args = &items[1..];
        let h = head.strip_prefix("clojure.core/").unwrap_or(&head);
        match h {
            "quote" => Ok(list(items.to_vec())),
            "if" => {
                if !(2..=3).contains(&args.len()) {
                    return Err("Syntax error compiling if: Too many or too few arguments to if".into());
                }
                let mut out = vec![sym("if")];
                out.extend(self.body(args, scope)?);
                Ok(list(out))
            }
            "do" => {
                let mut out = vec![sym("do")];
                out.extend(self.body(args, scope)?);
                Ok(list(out))
            }
            "let" | "let*" | "loop" | "loop*" => {
                let C::Vec(b) = args.first().ok_or("let requires a vector for its binding")? else { return Err("let requires a vector for its binding".into()) };
                if b.len() % 2 == 1 {
                    return Err("let requires an even number of forms in binding vector".into());
                }
                let mut sc = scope.to_vec();
                let mut out_b = vec![];
                for p in b.chunks(2) {
                    let C::Sym(n) = &p[0] else { return Err("Unsupported binding form (destructuring isn't supported)".into()) };
                    out_b.push(p[0].clone());
                    out_b.push(self.analyze(&p[1], &sc)?);
                    sc.push(n.clone());
                }
                let special = if h.starts_with("loop") { "loop*" } else { "let*" };
                let mut out = vec![sym(special), C::Vec(Rc::new(out_b))];
                out.extend(self.body(&args[1..], &sc)?);
                Ok(list(out))
            }
            "fn" | "fn*" => self.fun(args, scope),
            "recur" => {
                let mut out = vec![sym("recur")];
                out.extend(self.body(args, scope)?);
                Ok(list(out))
            }
            "def" | "defn" => {
                let Some(C::Sym(n)) = args.first() else { return Err("First argument to def must be a Symbol".into()) };
                self.defs.push(n.clone());
                let value = if h == "defn" {
                    let mut f = vec![sym("fn"), C::Sym(n.clone())];
                    f.extend(args[1..].iter().filter(|a| !matches!(a, C::Str(_) | C::Map(_))).cloned());
                    self.analyze(&list(f), scope)?
                } else {
                    args.get(1).map(|v| self.analyze(v, scope)).transpose()?.unwrap_or(C::Nil)
                };
                Ok(list(vec![sym("def"), C::Sym(n.clone()), value]))
            }
            "throw" => Ok(list(vec![sym("throw"), self.analyze(args.first().ok_or("Too few arguments to throw")?, scope)?])),
            "try" => {
                let mut out = vec![sym("try")];
                for a in args {
                    match a {
                        C::List(l) if matches!(l.first(), Some(C::Sym(c)) if &**c == "catch") => {
                            let Some(C::Sym(e)) = l.get(2) else { return Err("Unable to resolve classname in catch".into()) };
                            let mut sc = scope.to_vec();
                            sc.push(e.clone());
                            let mut c = vec![sym("catch"), l.get(1).cloned().unwrap_or(C::Nil), C::Sym(e.clone())];
                            c.extend(self.body(&l[3..], &sc)?);
                            out.push(list(c));
                        }
                        C::List(l) if matches!(l.first(), Some(C::Sym(c)) if &**c == "finally") => {
                            let mut c = vec![sym("finally")];
                            c.extend(self.body(&l[1..], scope)?);
                            out.push(list(c));
                        }
                        a => out.push(self.analyze(a, scope)?),
                    }
                }
                Ok(list(out))
            }
            "case" => {
                let expr = self.analyze(args.first().ok_or("case requires an expression")?, scope)?;
                let rest = &args[1..];
                let mut clauses = vec![];
                for p in rest.chunks(2) {
                    if p.len() == 2 {
                        clauses.push(p[0].clone());
                        clauses.push(self.analyze(&p[1], scope)?);
                    }
                }
                let default = if rest.len() % 2 == 1 { Some(self.analyze(rest.last().unwrap(), scope)?) } else { None };
                let mut out = vec![sym("case*"), expr, C::Vec(Rc::new(clauses))];
                out.extend(default);
                Ok(list(out))
            }
            "when" => self.analyze(&list(vec![sym("if"), args.first().cloned().unwrap_or(C::Nil), list([vec![sym("do")], args.get(1..).unwrap_or(&[]).to_vec()].concat())]), scope),
            "when-not" => self.analyze(&list(vec![sym("if"), args.first().cloned().unwrap_or(C::Nil), C::Nil, list([vec![sym("do")], args.get(1..).unwrap_or(&[]).to_vec()].concat())]), scope),
            "if-not" => self.analyze(&list(vec![sym("if"), args.first().cloned().unwrap_or(C::Nil), args.get(2).cloned().unwrap_or(C::Nil), args.get(1).cloned().unwrap_or(C::Nil)]), scope),
            "cond" => {
                let mut form = C::Nil;
                for p in args.chunks(2).rev() {
                    form = list(vec![sym("if"), p[0].clone(), p.get(1).cloned().unwrap_or(C::Nil), form]);
                }
                self.analyze(&form, scope)
            }
            "and" | "or" => {
                let Some((last, init)) = args.split_last() else { return Ok(C::Bool(h == "and")) };
                let mut form = last.clone();
                for a in init.iter().rev() {
                    let g = C::Sym(self.gen(h));
                    let test = if h == "and" { list(vec![sym("if"), g.clone(), form, g.clone()]) } else { list(vec![sym("if"), g.clone(), g.clone(), form]) };
                    form = list(vec![sym("let*"), C::Vec(Rc::new(vec![g, a.clone()])), test]);
                }
                self.analyze(&form, scope)
            }
            "->" | "->>" => {
                let mut x = args.first().cloned().ok_or("-> requires an argument")?;
                for f in &args[1..] {
                    x = match f {
                        C::List(l) if !l.is_empty() => {
                            let mut l = l.to_vec();
                            if h == "->" { l.insert(1, x) } else { l.push(x) }
                            list(l)
                        }
                        f => list(vec![f.clone(), x]),
                    };
                }
                self.analyze(&x, scope)
            }
            "if-let" | "when-let" => {
                let Some(C::Vec(b)) = args.first() else { return Err("if-let requires a vector for its binding".into()) };
                if b.len() != 2 {
                    return Err("if-let requires exactly 2 forms in binding vector".into());
                }
                let g = C::Sym(self.gen("temp"));
                let then = if h == "when-let" { list([vec![sym("do")], args[1..].to_vec()].concat()) } else { args.get(1).cloned().unwrap_or(C::Nil) };
                let els = if h == "when-let" { C::Nil } else { args.get(2).cloned().unwrap_or(C::Nil) };
                let inner = list(vec![sym("let*"), C::Vec(Rc::new(vec![b[0].clone(), g.clone()])), then]);
                self.analyze(&list(vec![sym("let*"), C::Vec(Rc::new(vec![g.clone(), b[1].clone()])), list(vec![sym("if"), g, inner, els])]), scope)
            }
            "letfn" => {
                let Some(C::Vec(fns)) = args.first() else { return Err("letfn requires a vector of functions".into()) };
                let mut b = vec![];
                for f in fns.iter() {
                    let C::List(l) = f else { return Err("letfn: bad function spec".into()) };
                    b.push(l[0].clone());
                    b.push(list([vec![sym("fn")], l.to_vec()].concat()));
                }
                self.analyze(&list([vec![sym("let*"), C::Vec(Rc::new(b))], args[1..].to_vec()].concat()), scope)
            }
            "doto" => {
                let g = C::Sym(self.gen("doto"));
                let mut body = vec![sym("do")];
                for f in &args[1..] {
                    body.push(match f {
                        C::List(l) => list([vec![l[0].clone(), g.clone()], l[1..].to_vec()].concat()),
                        f => list(vec![f.clone(), g.clone()]),
                    });
                }
                body.push(g.clone());
                self.analyze(&list(vec![sym("let*"), C::Vec(Rc::new(vec![g, args.first().cloned().unwrap_or(C::Nil)])), list(body)]), scope)
            }
            "comment" => Ok(C::Nil),
            "new" => {
                let Some(C::Sym(class)) = args.first() else { return Err("Unable to resolve classname".into()) };
                let class = class_name(class).ok_or_else(|| format!("Unable to resolve classname: {class}"))?;
                let mut out = vec![sym("new"), s(class)];
                out.extend(self.body(&args[1..], scope)?);
                Ok(list(out))
            }
            "." => {
                let target = self.analyze(args.first().ok_or("Malformed member expression")?, scope)?;
                let (method, rest) = match args.get(1) {
                    Some(C::List(l)) if !l.is_empty() => (l[0].clone(), l[1..].to_vec()),
                    Some(m) => (m.clone(), args[2..].to_vec()),
                    None => return Err("Malformed member expression".into()),
                };
                let C::Sym(m) = method else { return Err("Malformed member expression".into()) };
                let mut out = vec![sym("."), target, s(&m)];
                out.extend(self.body(&rest, scope)?);
                Ok(list(out))
            }
            _ if h.len() > 1 && h.starts_with('.') && h != ".." => {
                let mut out = vec![sym("."), self.analyze(args.first().ok_or("Malformed member expression, expecting (.member target ...)")?, scope)?, s(&h[1..])];
                out.extend(self.body(&args[1..], scope)?);
                Ok(list(out))
            }
            _ if h.len() > 1 && h.ends_with('.') => {
                let class = class_name(&h[..h.len() - 1]).ok_or_else(|| compile_error(h))?;
                let mut out = vec![sym("new"), s(class)];
                out.extend(self.body(args, scope)?);
                Ok(list(out))
            }
            _ => {
                let mut out = vec![self.analyze(&items[0], scope)?];
                out.extend(self.body(args, scope)?);
                Ok(list(out))
            }
        }
    }

    fn fun(&mut self, args: &[C], scope: &[Rc<str>]) -> R<C> {
        let (name, rest) = match args.first() {
            Some(C::Sym(n)) => (Some(n.clone()), &args[1..]),
            _ => (None, args),
        };
        let arities: Vec<(Rc<Vec<C>>, &[C])> = match rest.first() {
            Some(C::Vec(p)) => vec![(p.clone(), &rest[1..])],
            Some(C::List(_)) => rest
                .iter()
                .map(|a| match a {
                    C::List(l) => match l.first() {
                        Some(C::Vec(p)) => Ok((p.clone(), &l[1..])),
                        _ => Err("Parameter declaration missing".to_string()),
                    },
                    _ => Err("Parameter declaration missing".to_string()),
                })
                .collect::<R<_>>()?,
            _ => return Err("Parameter declaration missing".into()),
        };
        let mut out = vec![sym("fn*"), name.clone().map(C::Sym).unwrap_or(C::Nil)];
        for (params, body) in arities {
            let mut sc = scope.to_vec();
            sc.extend(name.clone());
            for p in params.iter() {
                match p {
                    C::Sym(n) if &**n != "&" => sc.push(n.clone()),
                    C::Sym(_) => {}
                    _ => return Err("Unsupported binding form (destructuring isn't supported)".into()),
                }
            }
            out.push(list([vec![C::Vec(params.clone())], self.body(body, &sc)?].concat()));
        }
        Ok(list(out))
    }
}

// ------------------------------------------------------------------ evaluation

thread_local! {
    /// Vars created by `def`. As in OpenRefine, they outlive the expression that defined them.
    static GLOBALS: RefCell<HashMap<String, C>> = RefCell::new(HashMap::new());
    /// Analysed forms by source. ponytail: unbounded per thread; bound it if servers compile many expressions.
    static CACHE: RefCell<HashMap<String, Rc<C>>> = RefCell::new(HashMap::new());
}

const PARAMS: [&str; 7] = ["value", "cell", "cells", "row", "rowIndex", "value1", "value2"];

fn analysed(source: &str) -> R<Rc<C>> {
    if let Some(f) = CACHE.with(|c| c.borrow().get(source).cloned()) {
        return Ok(f);
    }
    let chars: Vec<char> = source.chars().collect();
    let mut reader = Reader { src: &chars, pos: 0, anon: None };
    let forms = reader.read_all()?;
    let scope: Vec<Rc<str>> = PARAMS.iter().map(|p| Rc::from(*p)).collect();
    let mut an = Analyzer { gensym: 0, defs: vec![] };
    // (fn [params] forms...) evaluates every form and returns the last
    let body = an.body(&forms, &scope)?;
    let f = Rc::new(list([vec![sym("do")], body].concat()));
    CACHE.with(|c| c.borrow_mut().insert(source.to_string(), f.clone()));
    Ok(f)
}

/// Check that `source` reads and every symbol resolves.
pub fn check(source: &str) -> Result<(), String> {
    analysed(source).map(|_| ())
}

pub fn reads_row(source: &str) -> bool {
    ["cells", "row"].iter().any(|w| source.contains(w))
}

pub fn eval(source: &str, value: Value, row: &dyn Row) -> Value {
    eval_with(source, value, row, None)
}

/// `pair` binds value1 and value2 (a custom clustering distance).
pub fn eval_with(source: &str, value: Value, row: &dyn Row, pair: Option<(Value, Value)>) -> Value {
    let form = match analysed(source) {
        Ok(f) => f,
        Err(e) => return Value::err(e),
    };
    let cell = if value.is_null() && row.current_recon().is_none() { C::Nil } else { C::Obj(Rc::new(Obj::Cell(value.clone()))) };
    let (cells, rowv) = if reads_row(source) {
        let cells: Vec<(String, Option<C>)> = row
            .columns()
            .into_iter()
            .map(|n| {
                let v = row.cell(&n).unwrap_or(Value::Null);
                let c = (!v.is_null() || row.recon(&n).is_some()).then(|| C::Obj(Rc::new(Obj::Cell(v))));
                (n, c)
            })
            .collect();
        let cells = C::Obj(Rc::new(Obj::Cells(cells)));
        let r = C::Obj(Rc::new(Obj::Row { index: row.index() as i64, starred: row.starred(), flagged: row.flagged(), cells: cells.clone() }));
        (cells, r)
    } else {
        (C::Nil, C::Nil)
    };
    let (value1, value2) = pair.as_ref().map_or((C::Nil, C::Nil), |(a, b)| (from_value(a), from_value(b)));
    let mut env: Env = None;
    for (n, v) in PARAMS.iter().zip([from_value(&value), cell, cells, rowv, C::Long(row.index() as i64), value1, value2]) {
        env = bind(&env, (*n).into(), v);
    }
    match ev(&form, &env) {
        Ok(C::Recur(_)) => Value::err("Can only recur from tail position"),
        Ok(v) => to_value(&v),
        Err(e) => Value::err(e),
    }
}

fn from_value(v: &Value) -> C {
    match v {
        Value::Null => C::Nil,
        Value::Bool(b) => C::Bool(*b),
        Value::Int(i) => C::Long(*i),
        Value::Num(n) => C::Double(*n),
        Value::Str(x) => s(x),
        Value::Date(d) => C::Obj(Rc::new(Obj::Date(*d))),
        Value::Array(a) => C::Vec(Rc::new(a.iter().map(from_value).collect())),
        Value::Error(e) => C::Obj(Rc::new(Obj::Exception("EvalError".into(), e.clone()))),
        other => s(&other.to_java_string()),
    }
}

fn to_value(c: &C) -> Value {
    match c {
        C::Nil => Value::Null,
        C::Bool(b) => Value::Bool(*b),
        C::Long(i) => Value::Int(*i),
        C::Double(d) => Value::Num(*d),
        C::Str(x) => Value::Str(x.to_string()),
        C::List(l) | C::Vec(l) => Value::Array(l.iter().map(to_value).collect()),
        C::Obj(o) => match &**o {
            Obj::Cell(v) => Value::Obj(Class::Cell, vec![("value".into(), v.clone()), ("recon".into(), Value::Null)]),
            Obj::Cells(_) => Value::Obj(Class::Cells, vec![]),
            Obj::Row { .. } => Value::Obj(Class::Row, vec![]),
            Obj::Date(d) => Value::Date(*d),
            _ => Value::Java(to_str(c)),
        },
        other => Value::Java(to_str(other)),
    }
}

fn truthy(c: &C) -> bool {
    !matches!(c, C::Nil | C::Bool(false))
}

fn ev(f: &C, env: &Env) -> R<C> {
    match f {
        C::Sym(n) => lookup(env, n).ok_or_else(|| compile_error(n)),
        C::Var(n) => GLOBALS.with(|g| g.borrow().get(&**n).cloned()).ok_or_else(|| format!("Attempting to call unbound fn: #'clojure.core/{n}")),
        C::Vec(items) => Ok(C::Vec(Rc::new(items.iter().map(|x| ev(x, env)).collect::<R<_>>()?))),
        C::Set(items) => Ok(C::Set(Rc::new(dedupe(items.iter().map(|x| ev(x, env)).collect::<R<_>>()?)))),
        C::Map(pairs) => {
            let mut out: Vec<(C, C)> = vec![];
            for (k, v) in pairs.iter() {
                let (k, v) = (ev(k, env)?, ev(v, env)?);
                assoc_in(&mut out, k, v);
            }
            Ok(C::Map(Rc::new(out)))
        }
        C::List(items) if !items.is_empty() => {
            if let C::Sym(h) = &items[0] {
                if SPECIALS.contains(&&**h) && lookup(env, h).is_none() {
                    return special(h, &items[1..], env);
                }
            }
            let f = ev(&items[0], env)?;
            let args = items[1..].iter().map(|x| ev(x, env)).collect::<R<Vec<_>>>()?;
            apply(&f, &args)
        }
        other => Ok(other.clone()),
    }
}

fn do_body(body: &[C], env: &Env) -> R<C> {
    let mut last = C::Nil;
    for f in body {
        last = ev(f, env)?;
    }
    Ok(last)
}

fn special(h: &str, args: &[C], env: &Env) -> R<C> {
    match h {
        "quote" => Ok(args.first().cloned().unwrap_or(C::Nil)),
        "if" => {
            if truthy(&ev(&args[0], env)?) {
                ev(&args[1], env)
            } else {
                args.get(2).map_or(Ok(C::Nil), |e| ev(e, env))
            }
        }
        "do" => do_body(args, env),
        "let*" => {
            let C::Vec(b) = &args[0] else { unreachable!() };
            let mut env = env.clone();
            for p in b.chunks(2) {
                let C::Sym(n) = &p[0] else { unreachable!() };
                let v = ev(&p[1], &env)?;
                env = bind(&env, n.clone(), v);
            }
            do_body(&args[1..], &env)
        }
        "loop*" => {
            let C::Vec(b) = &args[0] else { unreachable!() };
            let names: Vec<Rc<str>> = b.chunks(2).map(|p| if let C::Sym(n) = &p[0] { n.clone() } else { unreachable!() }).collect();
            let mut env2 = env.clone();
            let mut vals = vec![];
            for p in b.chunks(2) {
                let C::Sym(n) = &p[0] else { unreachable!() };
                let v = ev(&p[1], &env2)?;
                env2 = bind(&env2, n.clone(), v.clone());
                vals.push(v);
            }
            loop {
                let mut e = env.clone();
                for (n, v) in names.iter().zip(&vals) {
                    e = bind(&e, n.clone(), v.clone());
                }
                match do_body(&args[1..], &e)? {
                    C::Recur(next) => {
                        if next.len() != names.len() {
                            return Err(format!("Mismatched argument count to recur, expected: {} args, got: {}", names.len(), next.len()));
                        }
                        vals = next.to_vec();
                    }
                    v => return Ok(v),
                }
            }
        }
        "recur" => Ok(C::Recur(Rc::new(args.iter().map(|x| ev(x, env)).collect::<R<_>>()?))),
        "fn*" => {
            let name = match &args[0] {
                C::Sym(n) => Some(n.clone()),
                _ => None,
            };
            let arities = args[1..]
                .iter()
                .map(|a| {
                    let C::List(l) = a else { unreachable!() };
                    let C::Vec(p) = &l[0] else { unreachable!() };
                    let mut params = vec![];
                    let mut rest = None;
                    let mut it = p.iter();
                    while let Some(C::Sym(n)) = it.next() {
                        if &**n == "&" {
                            if let Some(C::Sym(r)) = it.next() {
                                rest = Some(r.clone());
                            }
                        } else {
                            params.push(n.clone());
                        }
                    }
                    Arity { params, rest, body: l[1..].to_vec() }
                })
                .collect();
            Ok(C::Fn(Rc::new(Lambda { name, arities, env: env.clone() })))
        }
        "def" => {
            let C::Sym(n) = &args[0] else { unreachable!() };
            let v = args.get(1).map_or(Ok(C::Nil), |e| ev(e, env))?;
            GLOBALS.with(|g| g.borrow_mut().insert(n.to_string(), v));
            Ok(C::Obj(Rc::new(Obj::VarRef(format!("#'clojure.core/{n}")))))
        }
        "throw" => {
            let e = ev(&args[0], env)?;
            Err(match &e {
                C::Obj(o) => match &**o {
                    Obj::Exception(_, m) => m.clone(),
                    _ => to_str(&e),
                },
                _ => format!("class {} cannot be cast to class java.lang.Throwable", class_of(&e)),
            })
        }
        "try" => {
            let (mut body, mut catches, mut finally) = (vec![], vec![], None);
            for a in args {
                match a {
                    C::List(l) if matches!(l.first(), Some(C::Sym(c)) if &**c == "catch") => catches.push(l.clone()),
                    C::List(l) if matches!(l.first(), Some(C::Sym(c)) if &**c == "finally") => finally = Some(l.clone()),
                    a => body.push(a.clone()),
                }
            }
            let mut r = do_body(&body, env);
            if let Err(msg) = &r {
                if let Some(c) = catches.first() {
                    let C::Sym(name) = &c[2] else { unreachable!() };
                    let e = bind(env, name.clone(), C::Obj(Rc::new(Obj::Exception("java.lang.Exception".into(), msg.clone()))));
                    r = do_body(&c[3..], &e);
                }
            }
            if let Some(f) = finally {
                do_body(&f[1..], env)?;
            }
            r
        }
        "case*" => {
            let v = ev(&args[0], env)?;
            let C::Vec(clauses) = &args[1] else { unreachable!() };
            for p in clauses.chunks(2) {
                let hit = match &p[0] {
                    C::List(alts) => alts.iter().any(|a| equiv(a, &v)),
                    k => equiv(k, &v),
                };
                if hit {
                    return ev(&p[1], env);
                }
            }
            match args.get(2) {
                Some(d) => ev(d, env),
                None => Err(format!("No matching clause: {}", to_str(&v))),
            }
        }
        "." => {
            let target = ev(&args[0], env)?;
            let C::Str(m) = &args[1] else { unreachable!() };
            let rest = args[2..].iter().map(|x| ev(x, env)).collect::<R<Vec<_>>>()?;
            method(&target, m, &rest)
        }
        "new" => {
            let C::Str(class) = &args[0] else { unreachable!() };
            let rest = args[1..].iter().map(|x| ev(x, env)).collect::<R<Vec<_>>>()?;
            construct(class, &rest)
        }
        "var" => Ok(C::Nil),
        _ => Err(format!("Unknown special form {h}")),
    }
}

fn apply(f: &C, args: &[C]) -> R<C> {
    match f {
        C::Builtin(name) => call(name, args),
        C::Fn(l) => call_fn(l, args),
        C::Kw(_) | C::Map(_) | C::Set(_) => {
            let (coll, key) = if matches!(f, C::Kw(_)) { (args.first().cloned().unwrap_or(C::Nil), f.clone()) } else { (f.clone(), args.first().cloned().unwrap_or(C::Nil)) };
            Ok(get(&coll, &key).or_else(|| args.get(1).cloned()).unwrap_or(C::Nil))
        }
        C::Vec(v) => {
            let i = as_index(args.first().unwrap_or(&C::Nil))?;
            v.get(i).cloned().ok_or_else(|| "null".to_string())
        }
        C::Var(n) => {
            let f = GLOBALS.with(|g| g.borrow().get(&**n).cloned()).ok_or_else(|| format!("Attempting to call unbound fn: #'clojure.core/{n}"))?;
            apply(&f, args)
        }
        other => Err(format!("class {} cannot be cast to class clojure.lang.IFn", class_of(other))),
    }
}

fn call_fn(l: &Rc<Lambda>, args: &[C]) -> R<C> {
    let mut args = args.to_vec();
    loop {
        let arity = l
            .arities
            .iter()
            .find(|a| a.params.len() == args.len() && a.rest.is_none())
            .or_else(|| l.arities.iter().find(|a| a.rest.is_some() && args.len() >= a.params.len()))
            .ok_or_else(|| format!("Wrong number of args ({}) passed to: {}", args.len(), l.name.as_deref().unwrap_or("fn")))?;
        let mut env = l.env.clone();
        if let Some(n) = &l.name {
            env = bind(&env, n.clone(), C::Fn(l.clone()));
        }
        for (p, a) in arity.params.iter().zip(&args) {
            env = bind(&env, p.clone(), a.clone());
        }
        if let Some(r) = &arity.rest {
            let extra = args[arity.params.len()..].to_vec();
            env = bind(&env, r.clone(), if extra.is_empty() { C::Nil } else { list(extra) });
        }
        match do_body(&arity.body, &env)? {
            C::Recur(next) => args = next.to_vec(),
            v => return Ok(v),
        }
    }
}

// ------------------------------------------------------------------ printing

fn class_of(c: &C) -> &'static str {
    match c {
        C::Nil => "nil",
        C::Bool(_) => "java.lang.Boolean",
        C::Long(_) => "java.lang.Long",
        C::Double(_) => "java.lang.Double",
        C::Big(_) => "clojure.lang.BigInt",
        C::Ratio(..) => "clojure.lang.Ratio",
        C::Dec(_) => "java.math.BigDecimal",
        C::Char(_) => "java.lang.Character",
        C::Str(_) => "java.lang.String",
        C::Kw(_) => "clojure.lang.Keyword",
        C::Sym(_) => "clojure.lang.Symbol",
        C::List(_) => "clojure.lang.PersistentList",
        C::Vec(_) => "clojure.lang.PersistentVector",
        C::Map(_) => "clojure.lang.PersistentArrayMap",
        C::Set(_) => "clojure.lang.PersistentHashSet",
        C::Re(..) => "java.util.regex.Pattern",
        C::Fn(_) | C::Builtin(_) | C::Var(_) => "clojure.lang.AFunction",
        C::Recur(_) => "clojure.lang.Recur",
        C::Obj(o) => match &**o {
            Obj::Cell(_) => "com.google.refine.expr.WrappedCell",
            Obj::Cells(_) => "com.google.refine.expr.CellTuple",
            Obj::Row { .. } => "com.google.refine.expr.WrappedRow",
            Obj::Date(_) => "java.time.OffsetDateTime",
            Obj::Exception(..) => "java.lang.Exception",
            Obj::Class(_) => "java.lang.Class",
            Obj::VarRef(_) => "clojure.lang.Var",
        },
    }
}

fn num_str(c: &C) -> String {
    match c {
        C::Long(i) => i.to_string(),
        C::Double(d) => java_double(*d),
        C::Big(i) => i.to_string(),
        C::Ratio(n, d) => format!("{n}/{d}"),
        C::Dec(d) => d.to_string(),
        _ => unreachable!(),
    }
}

/// Java toString(), which is what `str` and OpenRefine's preview show.
fn to_str(c: &C) -> String {
    match c {
        C::Nil => String::new(),
        C::Bool(b) => b.to_string(),
        C::Long(_) | C::Double(_) | C::Big(_) | C::Ratio(..) | C::Dec(_) => num_str(c),
        C::Char(ch) => ch.to_string(),
        C::Str(x) => x.to_string(),
        C::Kw(k) => format!(":{k}"),
        C::Sym(x) => x.to_string(),
        C::Re(_, p) => p.to_string(),
        C::List(_) | C::Vec(_) | C::Map(_) | C::Set(_) => pr(c),
        C::Fn(l) => format!("clojure.core$eval$fn__{}", l.name.as_deref().unwrap_or("anon")),
        C::Builtin(n) => format!("clojure.core${}", n.replace('-', "_")),
        C::Var(n) => format!("#'clojure.core/{n}"),
        C::Recur(_) => "recur".into(),
        C::Obj(o) => match &**o {
            Obj::Cell(_) => "com.google.refine.expr.WrappedCell".into(),
            Obj::Cells(_) => "com.google.refine.expr.CellTuple".into(),
            Obj::Row { .. } => "com.google.refine.expr.WrappedRow".into(),
            Obj::Date(d) => fmt_date(d),
            Obj::Exception(class, m) => format!("{class}: {m}"),
            Obj::Class(n) => format!("class {n}"),
            Obj::VarRef(v) => v.clone(),
        },
    }
}

/// pr-str: what Clojure prints inside collections.
fn pr(c: &C) -> String {
    let join = |items: &[C]| items.iter().map(pr).collect::<Vec<_>>().join(" ");
    match c {
        C::Nil => "nil".into(),
        C::Str(x) => format!("\"{}\"", x.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n").replace('\t', "\\t").replace('\r', "\\r")),
        C::Char(ch) => match ch {
            '\n' => "\\newline".into(),
            ' ' => "\\space".into(),
            '\t' => "\\tab".into(),
            '\r' => "\\return".into(),
            ch => format!("\\{ch}"),
        },
        C::Dec(d) => format!("{d}M"),
        C::Big(i) => format!("{i}N"),
        C::List(l) => format!("({})", join(l)),
        C::Vec(l) => format!("[{}]", join(l)),
        C::Set(l) => format!("#{{{}}}", join(l)),
        C::Map(m) => format!("{{{}}}", m.iter().map(|(k, v)| format!("{} {}", pr(k), pr(v))).collect::<Vec<_>>().join(", ")),
        C::Re(_, p) => format!("#\"{p}\""),
        other => to_str(other),
    }
}

// ------------------------------------------------------------------ equality and collections

#[derive(PartialEq)]
enum Cat {
    Exact,
    Float,
    Decimal,
}

fn cat(c: &C) -> Option<Cat> {
    match c {
        C::Long(_) | C::Big(_) | C::Ratio(..) => Some(Cat::Exact),
        C::Double(_) => Some(Cat::Float),
        C::Dec(_) => Some(Cat::Decimal),
        _ => None,
    }
}

fn seq_items(c: &C) -> Option<Vec<C>> {
    match c {
        C::List(l) | C::Vec(l) => Some(l.to_vec()),
        _ => None,
    }
}

/// clojure.core/=
fn equiv(a: &C, b: &C) -> bool {
    if let (Some(x), Some(y)) = (cat(a), cat(b)) {
        return x == y && num_cmp(a, b) == Some(std::cmp::Ordering::Equal);
    }
    match (a, b) {
        (C::Nil, C::Nil) => true,
        (C::Bool(x), C::Bool(y)) => x == y,
        (C::Char(x), C::Char(y)) => x == y,
        (C::Str(x), C::Str(y)) | (C::Kw(x), C::Kw(y)) | (C::Sym(x), C::Sym(y)) => x == y,
        (C::Map(x), C::Map(y)) => x.len() == y.len() && x.iter().all(|(k, v)| y.iter().any(|(k2, v2)| equiv(k, k2) && equiv(v, v2))),
        (C::Set(x), C::Set(y)) => x.len() == y.len() && x.iter().all(|v| y.iter().any(|w| equiv(v, w))),
        _ => match (seq_items(a), seq_items(b)) {
            (Some(x), Some(y)) => x.len() == y.len() && x.iter().zip(&y).all(|(p, q)| equiv(p, q)),
            _ => false,
        },
    }
}

fn get(coll: &C, key: &C) -> Option<C> {
    match coll {
        C::Map(m) => m.iter().find(|(k, _)| equiv(k, key)).map(|(_, v)| v.clone()),
        C::Set(s) => s.iter().find(|v| equiv(v, key)).cloned(),
        C::Vec(v) => match key {
            C::Long(i) if *i >= 0 => v.get(*i as usize).cloned(),
            _ => None,
        },
        C::Str(x) => match key {
            C::Long(i) if *i >= 0 => x.chars().nth(*i as usize).map(C::Char),
            _ => None,
        },
        C::Obj(o) => field(o, &to_str(key)).ok().flatten(),
        _ => None,
    }
}

fn assoc_in(m: &mut Vec<(C, C)>, k: C, v: C) {
    match m.iter_mut().find(|(k2, _)| equiv(k2, &k)) {
        Some(slot) => slot.1 = v,
        None => m.push((k, v)),
    }
}

/// `seq` over anything seqable; nil and empty give an empty vector.
fn items(c: &C) -> R<Vec<C>> {
    Ok(match c {
        C::Nil => vec![],
        C::List(l) | C::Vec(l) | C::Set(l) => l.to_vec(),
        C::Map(m) => m.iter().map(|(k, v)| C::Vec(Rc::new(vec![k.clone(), v.clone()]))).collect(),
        C::Str(x) => x.chars().map(C::Char).collect(),
        other => return Err(format!("Don't know how to create ISeq from: {}", class_of(other))),
    })
}

fn seq_or_nil(v: Vec<C>) -> C {
    if v.is_empty() { C::Nil } else { list(v) }
}

fn as_index(c: &C) -> R<usize> {
    match c {
        C::Long(i) if *i >= 0 => Ok(*i as usize),
        C::Double(d) if *d >= 0.0 => Ok(*d as usize),
        C::Long(_) | C::Double(_) => Err("Index out of range".into()),
        other => Err(format!("class {} cannot be cast to class java.lang.Number", class_of(other))),
    }
}

// ------------------------------------------------------------------ numbers

fn num_err(c: &C) -> String {
    format!("class {} cannot be cast to class java.lang.Number", class_of(c))
}

fn to_f64(c: &C) -> R<f64> {
    use bigdecimal::ToPrimitive;
    Ok(match c {
        C::Long(i) => *i as f64,
        C::Double(d) => *d,
        C::Big(i) => *i as f64,
        C::Ratio(n, d) => *n as f64 / *d as f64,
        C::Dec(d) => d.to_f64().unwrap_or(f64::NAN),
        C::Char(_) | _ => return Err(num_err(c)),
    })
}

fn to_dec(c: &C) -> bigdecimal::BigDecimal {
    use bigdecimal::BigDecimal;
    match c {
        C::Long(i) => BigDecimal::from(*i),
        C::Big(i) => i.to_string().parse().unwrap(),
        C::Ratio(n, d) => BigDecimal::from(i64::try_from(*n).unwrap_or(0)) / BigDecimal::from(i64::try_from(*d).unwrap_or(1)),
        C::Dec(d) => d.clone(),
        _ => BigDecimal::from(0),
    }
}

/// Exact numbers as a ratio n/d.
fn rat(c: &C) -> Option<(i128, i128)> {
    match c {
        C::Long(i) => Some((*i as i128, 1)),
        C::Big(i) => Some((*i, 1)),
        C::Ratio(n, d) => Some((*n, *d)),
        _ => None,
    }
}

fn num_cmp(a: &C, b: &C) -> Option<std::cmp::Ordering> {
    if let (Some((an, ad)), Some((bn, bd))) = (rat(a), rat(b)) {
        return Some((an * bd).cmp(&(bn * ad)));
    }
    if matches!(a, C::Dec(_)) && matches!(b, C::Dec(_)) {
        return to_dec(a).partial_cmp(&to_dec(b));
    }
    to_f64(a).ok()?.partial_cmp(&to_f64(b).ok()?)
}

#[derive(Clone, Copy, PartialEq)]
enum Op {
    Add,
    Sub,
    Mul,
    Div,
}

/// Binary arithmetic with Clojure's contagion: Double > BigDecimal > Ratio > BigInt > Long.
/// `promote` is the primed ops (+' *'): overflow becomes a BigInt instead of an error.
fn arith(op: Op, a: &C, b: &C, promote: bool) -> R<C> {
    let (ca, cb) = (cat(a).ok_or_else(|| num_err(a))?, cat(b).ok_or_else(|| num_err(b))?);
    if ca == Cat::Float || cb == Cat::Float {
        let (x, y) = (to_f64(a)?, to_f64(b)?);
        return Ok(C::Double(match op {
            Op::Add => x + y,
            Op::Sub => x - y,
            Op::Mul => x * y,
            Op::Div => x / y,
        }));
    }
    if ca == Cat::Decimal || cb == Cat::Decimal {
        let (x, y) = (to_dec(a), to_dec(b));
        return Ok(C::Dec(match op {
            Op::Add => x + y,
            Op::Sub => x - y,
            Op::Mul => x * y,
            Op::Div => {
                if y == bigdecimal::BigDecimal::from(0) {
                    return Err("Divide by zero".into());
                }
                x / y
            }
        }));
    }
    if let (C::Long(x), C::Long(y)) = (a, b) {
        if op == Op::Div {
            return ratio(*x as i128, *y as i128, false);
        }
        let r = match op {
            Op::Add => x.checked_add(*y),
            Op::Sub => x.checked_sub(*y),
            _ => x.checked_mul(*y),
        };
        return match r {
            Some(v) => Ok(C::Long(v)),
            None if promote => Ok(C::Big(match op {
                Op::Add => *x as i128 + *y as i128,
                Op::Sub => *x as i128 - *y as i128,
                _ => *x as i128 * *y as i128,
            })),
            None => Err("long overflow".into()),
        };
    }
    let ((an, ad), (bn, bd)) = (rat(a).unwrap(), rat(b).unwrap());
    let big = matches!(a, C::Big(_)) || matches!(b, C::Big(_));
    let ovf = || "integer overflow".to_string();
    let (n, d) = match op {
        Op::Add => (an.checked_mul(bd).and_then(|x| bn.checked_mul(ad).and_then(|y| x.checked_add(y))).ok_or_else(ovf)?, ad.checked_mul(bd).ok_or_else(ovf)?),
        Op::Sub => (an.checked_mul(bd).and_then(|x| bn.checked_mul(ad).and_then(|y| x.checked_sub(y))).ok_or_else(ovf)?, ad.checked_mul(bd).ok_or_else(ovf)?),
        Op::Mul => (an.checked_mul(bn).ok_or_else(ovf)?, ad.checked_mul(bd).ok_or_else(ovf)?),
        Op::Div => (an.checked_mul(bd).ok_or_else(ovf)?, ad.checked_mul(bn).ok_or_else(ovf)?),
    };
    ratio(n, d, big)
}

fn fold(op: Op, args: &[C], identity: i64, promote: bool) -> R<C> {
    match args {
        [] => Ok(C::Long(identity)),
        [x] => match op {
            Op::Sub => arith(Op::Sub, &C::Long(0), x, promote),
            Op::Div => arith(Op::Div, &C::Long(1), x, promote),
            _ => {
                cat(x).ok_or_else(|| num_err(x))?;
                Ok(x.clone())
            }
        },
        [first, rest @ ..] => rest.iter().try_fold(first.clone(), |acc, x| arith(op, &acc, x, promote)),
    }
}

fn int_div(name: &str, a: &C, b: &C) -> R<C> {
    if let (C::Long(x), C::Long(y)) = (a, b) {
        if *y == 0 {
            return Err("Divide by zero".into());
        }
        return Ok(C::Long(match name {
            "quot" => x.wrapping_div(*y),
            "rem" => x.wrapping_rem(*y),
            _ => {
                let m = x.wrapping_rem(*y);
                if m != 0 && ((m < 0) != (*y < 0)) { m + y } else { m }
            }
        }));
    }
    let (x, y) = (to_f64(a)?, to_f64(b)?);
    if y == 0.0 {
        return Err("Divide by zero".into());
    }
    let q = (x / y).trunc();
    Ok(C::Double(match name {
        "quot" => q,
        "rem" => x - q * y,
        _ => {
            let m = x - q * y;
            if m != 0.0 && ((m < 0.0) != (y < 0.0)) { m + y } else { m }
        }
    }))
}

fn long_of(c: &C) -> R<i64> {
    match c {
        C::Long(i) => Ok(*i),
        C::Double(d) => Ok(*d as i64),
        C::Big(i) => Ok(*i as i64),
        C::Ratio(n, d) => Ok((n / d) as i64),
        C::Dec(_) => Ok(to_f64(c)? as i64),
        C::Char(ch) => Ok(*ch as i64),
        other => Err(num_err(other)),
    }
}

// ------------------------------------------------------------------ builtins

/// The functions an expression can name; anything else is an unresolved symbol.
const BUILTINS: &[&str] = &[
    "+", "-", "*", "/", "+'", "-'", "*'", "inc", "dec", "inc'", "dec'", "quot", "rem", "mod", "max", "min", "abs", "=", "==", "not=", "<", ">", "<=", ">=", "compare", "not", "nil?",
    "some?", "true?", "false?", "zero?", "pos?", "neg?", "even?", "odd?", "number?", "integer?", "float?", "ratio?", "decimal?", "string?", "char?", "keyword?", "symbol?", "map?",
    "vector?", "list?", "set?", "seq?", "coll?", "sequential?", "fn?", "ifn?", "boolean?", "empty?", "not-empty", "identity", "count", "str", "subs", "format", "first", "second",
    "rest", "next", "last", "butlast", "nth", "nthrest", "get", "get-in", "keys", "vals", "key", "val", "assoc", "assoc-in", "dissoc", "update", "conj", "cons", "concat", "into",
    "vec", "list", "vector", "hash-map", "array-map", "hash-set", "set", "seq", "sequence", "reverse", "sort", "sort-by", "distinct", "dedupe", "frequencies", "group-by",
    "zipmap", "map", "mapv", "map-indexed", "mapcat", "filter", "filterv", "remove", "keep", "reduce", "reductions", "apply", "take", "drop", "take-last", "drop-last",
    "take-while", "drop-while", "split-at", "split-with", "range", "repeat", "interpose", "interleave", "partition", "partition-all", "partition-by", "flatten", "some",
    "every?", "not-every?", "not-any?", "contains?", "merge", "select-keys", "find", "keyword", "name", "namespace", "symbol", "int", "long", "double", "float", "short",
    "byte", "char", "boolean", "bigint", "bigdec", "num", "rationalize", "numerator", "denominator", "pr-str", "prn-str", "print-str", "println-str", "println", "print",
    "prn", "pr", "re-find", "re-seq", "re-matches", "re-pattern", "type", "class", "instance?", "hash", "bit-and", "bit-or", "bit-xor", "bit-not", "bit-shift-left",
    "bit-shift-right", "unchecked-inc", "unchecked-dec", "unchecked-add", "unchecked-subtract", "unchecked-multiply", "comp", "partial", "constantly", "complement",
    "juxt", "fnil", "max-key", "min-key", "empty", "peek", "pop", "subvec", "doall", "dorun", "realized?", "boolean", "ex-info", "ex-message", "deref", "parse-long",
    "parse-double", "rand", "gensym", "trampoline", "list*", "replace", "shuffle", "char-escape-string", "rseq", "last-index-of",
    "clojure.string/upper-case", "clojure.string/lower-case", "clojure.string/capitalize", "clojure.string/trim", "clojure.string/triml", "clojure.string/trimr",
    "clojure.string/trim-newline", "clojure.string/reverse", "clojure.string/replace", "clojure.string/replace-first", "clojure.string/split", "clojure.string/split-lines",
    "clojure.string/join", "clojure.string/blank?", "clojure.string/includes?", "clojure.string/starts-with?", "clojure.string/ends-with?", "clojure.string/index-of",
    "clojure.string/last-index-of", "clojure.string/escape",
    "Integer/parseInt", "Integer/valueOf", "Long/parseLong", "Long/valueOf", "Double/parseDouble", "Double/valueOf", "Boolean/parseBoolean", "String/valueOf",
    "Long/toBinaryString", "Long/toHexString", "Long/toOctalString", "Integer/toBinaryString", "Integer/toHexString", "Integer/toOctalString", "Long/toString",
    "Integer/toString", "Math/sqrt", "Math/abs", "Math/round", "Math/floor", "Math/ceil", "Math/pow", "Math/max", "Math/min", "Math/log", "Math/log10", "Math/exp",
    "Math/sin", "Math/cos", "Math/tan", "Math/signum", "Math/cbrt", "Math/rint", "Character/isLetter", "Character/isDigit", "Character/isLetterOrDigit",
    "Character/isUpperCase", "Character/isLowerCase", "Character/isWhitespace", "Character/isAlphabetic", "Character/toUpperCase", "Character/toLowerCase",
];

fn builtin(name: &str) -> Option<&'static str> {
    BUILTINS.iter().find(|b| **b == name).copied()
}

fn static_field(name: &str) -> Option<C> {
    Some(match name {
        "Math/PI" => C::Double(std::f64::consts::PI),
        "Math/E" => C::Double(std::f64::consts::E),
        "Long/MAX_VALUE" => C::Long(i64::MAX),
        "Long/MIN_VALUE" => C::Long(i64::MIN),
        "Integer/MAX_VALUE" => C::Long(i32::MAX as i64),
        "Integer/MIN_VALUE" => C::Long(i32::MIN as i64),
        "Double/NaN" => C::Double(f64::NAN),
        "Double/POSITIVE_INFINITY" => C::Double(f64::INFINITY),
        "Double/NEGATIVE_INFINITY" => C::Double(f64::NEG_INFINITY),
        "Double/MAX_VALUE" => C::Double(f64::MAX),
        _ => return None,
    })
}

fn class_name(name: &str) -> Option<&'static str> {
    let n = name.rsplit('.').next().unwrap_or(name);
    Some(match n {
        "String" => "java.lang.String",
        "Long" => "java.lang.Long",
        "Integer" => "java.lang.Integer",
        "Double" => "java.lang.Double",
        "Number" => "java.lang.Number",
        "Boolean" => "java.lang.Boolean",
        "Character" => "java.lang.Character",
        "Object" => "java.lang.Object",
        "Math" => "java.lang.Math",
        "Exception" => "java.lang.Exception",
        "RuntimeException" => "java.lang.RuntimeException",
        "IllegalArgumentException" => "java.lang.IllegalArgumentException",
        "IllegalStateException" => "java.lang.IllegalStateException",
        "Throwable" => "java.lang.Throwable",
        "Date" => "java.util.Date",
        "Keyword" => "clojure.lang.Keyword",
        "BigInt" => "clojure.lang.BigInt",
        "Ratio" => "clojure.lang.Ratio",
        "BigDecimal" => "java.math.BigDecimal",
        _ => return None,
    })
}

fn is_instance(class: &str, c: &C) -> bool {
    let own = class_of(c);
    own == class
        || match class {
            "java.lang.Object" => !matches!(c, C::Nil),
            "java.lang.Number" => cat(c).is_some(),
            "java.lang.Throwable" | "java.lang.Exception" => matches!(c, C::Obj(o) if matches!(**o, Obj::Exception(..))),
            _ => false,
        }
}

fn arg<'a>(args: &'a [C], i: usize) -> &'a C {
    args.get(i).unwrap_or(&C::Nil)
}

fn text<'a>(c: &'a C) -> R<&'a str> {
    match c {
        C::Str(x) => Ok(x),
        C::Nil => Err("null".into()),
        other => Err(format!("class {} cannot be cast to class java.lang.String", class_of(other))),
    }
}

fn arity(name: &str, args: &[C], ok: impl Fn(usize) -> bool) -> R<()> {
    if ok(args.len()) { Ok(()) } else { Err(format!("Wrong number of args ({}) passed to: clojure.core/{name}", args.len())) }
}

fn pattern(c: &C) -> R<Rc<Regex>> {
    match c {
        C::Re(r, _) => Ok(r.clone()),
        C::Nil => Err("null".into()),
        other => Err(format!("class {} cannot be cast to class java.util.regex.Pattern", class_of(other))),
    }
}

/// A regex match as Clojure returns it: the string, or [whole group1 ...] when there are groups.
fn match_value(caps: &regex::Captures) -> C {
    if caps.len() == 1 {
        return s(caps.get(0).unwrap().as_str());
    }
    C::Vec(Rc::new(caps.iter().map(|g| g.map_or(C::Nil, |m| s(m.as_str()))).collect()))
}

fn char_fn(args: &[C], f: impl Fn(char) -> C) -> R<C> {
    match arg(args, 0) {
        C::Char(c) => Ok(f(*c)),
        C::Long(i) => char::from_u32(*i as u32).map(f).ok_or_else(|| "Invalid code point".into()),
        other => Err(format!("No matching method found taking 1 args for class {}", class_of(other))),
    }
}

/// Java Integer/Long.parseX: an optional sign and digits, nothing else.
fn parse_java_int(t: &str, bits: u32) -> R<C> {
    let ok = !t.is_empty() && t.strip_prefix(['+', '-']).unwrap_or(t).chars().all(|c| c.is_ascii_digit()) && t != "+" && t != "-";
    let v = if ok { t.parse::<i64>().ok() } else { None };
    match v {
        Some(v) if bits == 64 || (i32::MIN as i64..=i32::MAX as i64).contains(&v) => Ok(C::Long(v)),
        _ => Err(format!("For input string: \"{t}\"")),
    }
}

/// Java Double.parseDouble: trims ASCII whitespace; accepts NaN, Infinity and a d/f suffix.
fn parse_java_double(t: &str) -> R<C> {
    let x = t.trim_matches(|c: char| c <= ' ');
    let body = x.strip_suffix(['d', 'D', 'f', 'F']).unwrap_or(x);
    let plain = body.strip_prefix(['+', '-']).unwrap_or(body);
    let valid = matches!(plain, "NaN" | "Infinity") || (plain.chars().any(|c| c.is_ascii_digit()) && plain.chars().all(|c| c.is_ascii_digit() || matches!(c, '.' | 'e' | 'E' | '+' | '-')));
    match body.parse::<f64>() {
        Ok(v) if valid => Ok(C::Double(v)),
        _ => Err(format!("For input string: \"{x}\"")),
    }
}

/// Java's String.split: trailing empty strings are dropped.
fn java_split(t: &str, re: &Regex, limit: i64) -> Vec<C> {
    let mut parts: Vec<&str> = if limit > 0 { re.splitn(t, limit as usize).collect() } else { re.split(t).collect() };
    if parts.first() == Some(&"") && re.find(t).is_some_and(|m| m.start() == 0 && m.end() > 0) {
        parts.remove(0);
    }
    if limit == 0 {
        while parts.last() == Some(&"") && parts.len() > 0 {
            parts.pop();
        }
    }
    if t.is_empty() {
        return vec![s("")];
    }
    parts.into_iter().map(s).collect()
}

fn java_hash(t: &str) -> i32 {
    t.encode_utf16().fold(0i32, |h, c| h.wrapping_mul(31).wrapping_add(c as i32))
}

/// clojure.lang.Murmur3.hashInt / hashLong, as `hash` uses for strings and numbers.
fn murmur_int(input: i32) -> i32 {
    if input == 0 {
        return 0;
    }
    fmix(mix_h1(0, mix_k1(input)), 4)
}
fn murmur_long(input: i64) -> i32 {
    if input == 0 {
        return 0;
    }
    let low = input as i32;
    let high = (input >> 32) as i32;
    let h1 = mix_h1(mix_h1(0, mix_k1(low)), mix_k1(high));
    fmix(h1, 8)
}
fn mix_k1(k: i32) -> i32 {
    (k.wrapping_mul(0xcc9e2d51u32 as i32)).rotate_left(15).wrapping_mul(0x1b873593)
}
fn mix_h1(h: i32, k: i32) -> i32 {
    (h ^ k).rotate_left(13).wrapping_mul(5).wrapping_add(0xe6546b64u32 as i32)
}
fn fmix(h: i32, len: i32) -> i32 {
    let mut h = (h ^ len) as u32;
    h ^= h >> 16;
    h = h.wrapping_mul(0x85ebca6b);
    h ^= h >> 13;
    h = h.wrapping_mul(0xc2b2ae35);
    h ^= h >> 16;
    h as i32
}

/// java.util.Formatter for the conversions expressions use: %s %d %f %e %x %o %c %b %n %%.
fn java_format(fmt: &str, args: &[C]) -> R<String> {
    let mut out = String::new();
    let mut it = fmt.chars().peekable();
    let mut next = 0;
    while let Some(c) = it.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        let mut flags = String::new();
        while let Some(&f) = it.peek() {
            if "-0+ ,#".contains(f) {
                flags.push(f);
                it.next();
            } else {
                break;
            }
        }
        let mut width = String::new();
        while let Some(&d) = it.peek().filter(|d| d.is_ascii_digit()) {
            width.push(d);
            it.next();
        }
        let mut prec = None;
        if it.peek() == Some(&'.') {
            it.next();
            let mut p = String::new();
            while let Some(&d) = it.peek().filter(|d| d.is_ascii_digit()) {
                p.push(d);
                it.next();
            }
            prec = p.parse::<usize>().ok();
        }
        let conv = it.next().ok_or("Format specifier missing conversion")?;
        let body = match conv {
            '%' => "%".to_string(),
            'n' => "\n".to_string(),
            _ => {
                let a = args.get(next).cloned().ok_or_else(|| format!("Format specifier '%{conv}'"))?;
                next += 1;
                match conv {
                    's' | 'S' => {
                        let t = if matches!(a, C::Nil) { "null".to_string() } else { to_str(&a) };
                        let t = match prec {
                            Some(p) => t.chars().take(p).collect(),
                            None => t,
                        };
                        if conv == 'S' { t.to_uppercase() } else { t }
                    }
                    'd' => match &a {
                        C::Long(_) | C::Big(_) => {
                            let v = num_str(&a);
                            if flags.contains(',') { group_digits(&v) } else { v }
                        }
                        C::Nil => "null".into(),
                        other => return Err(format!("d != {}", class_of(other))),
                    },
                    'f' | 'e' | 'E' => match &a {
                        C::Double(_) | C::Dec(_) => {
                            let v = to_f64(&a)?;
                            let p = prec.unwrap_or(6);
                            let t = if conv == 'f' { format!("{v:.p$}") } else { java_exp(v, p, conv == 'E') };
                            if flags.contains(',') { group_digits(&t) } else { t }
                        }
                        C::Nil => "null".into(),
                        other => return Err(format!("{conv} != {}", class_of(other))),
                    },
                    'x' | 'X' | 'o' => match &a {
                        C::Long(v) => match conv {
                            'x' => format!("{v:x}"),
                            'X' => format!("{v:X}"),
                            _ => format!("{v:o}"),
                        },
                        other => return Err(format!("{conv} != {}", class_of(other))),
                    },
                    'c' => match &a {
                        C::Char(ch) => ch.to_string(),
                        C::Long(v) => char::from_u32(*v as u32).map(String::from).unwrap_or_default(),
                        other => return Err(format!("c != {}", class_of(other))),
                    },
                    'b' | 'B' => (!matches!(a, C::Nil | C::Bool(false))).to_string(),
                    other => return Err(format!("Conversion = '{other}'")),
                }
            }
        };
        let w: usize = width.parse().unwrap_or(0);
        let len = body.chars().count();
        if len >= w {
            out.push_str(&body);
        } else if flags.contains('-') {
            out.push_str(&body);
            out.push_str(&" ".repeat(w - len));
        } else if flags.contains('0') && body.starts_with('-') {
            out.push('-');
            out.push_str(&"0".repeat(w - len));
            out.push_str(&body[1..]);
        } else if flags.contains('0') {
            out.push_str(&"0".repeat(w - len));
            out.push_str(&body);
        } else {
            out.push_str(&" ".repeat(w - len));
            out.push_str(&body);
        }
    }
    Ok(out)
}

fn group_digits(t: &str) -> String {
    let (neg, t) = t.strip_prefix('-').map_or((false, t), |x| (true, x));
    let (int, frac) = t.split_once('.').map_or((t, None), |(a, b)| (a, Some(b)));
    let mut g = String::new();
    for (i, c) in int.chars().enumerate() {
        if i > 0 && (int.len() - i) % 3 == 0 {
            g.push(',');
        }
        g.push(c);
    }
    format!("{}{g}{}", if neg { "-" } else { "" }, frac.map(|f| format!(".{f}")).unwrap_or_default())
}

fn java_exp(v: f64, p: usize, upper: bool) -> String {
    let t = format!("{v:.p$e}");
    let (m, e) = t.split_once('e').unwrap();
    let e: i32 = e.parse().unwrap();
    let t = format!("{m}e{}{:02}", if e < 0 { '-' } else { '+' }, e.abs());
    if upper { t.to_uppercase() } else { t }
}

fn bool_fn(f: impl Fn(&C) -> bool, args: &[C]) -> R<C> {
    Ok(C::Bool(f(arg(args, 0))))
}

fn call(name: &str, args: &[C]) -> R<C> {
    let a0 = arg(args, 0);
    let a1 = arg(args, 1);
    Ok(match name {
        "+" => fold(Op::Add, args, 0, false)?,
        "+'" => fold(Op::Add, args, 0, true)?,
        "*" => fold(Op::Mul, args, 1, false)?,
        "*'" => fold(Op::Mul, args, 1, true)?,
        "-" | "-'" => {
            arity(name, args, |n| n > 0)?;
            fold(Op::Sub, args, 0, name == "-'")?
        }
        "/" => {
            arity(name, args, |n| n > 0)?;
            fold(Op::Div, args, 1, false)?
        }
        "inc" | "inc'" => arith(Op::Add, a0, &C::Long(1), name == "inc'")?,
        "dec" | "dec'" => arith(Op::Sub, a0, &C::Long(1), name == "dec'")?,
        "unchecked-inc" => C::Long(long_of(a0)?.wrapping_add(1)),
        "unchecked-dec" => C::Long(long_of(a0)?.wrapping_sub(1)),
        "unchecked-add" => C::Long(long_of(a0)?.wrapping_add(long_of(a1)?)),
        "unchecked-subtract" => C::Long(long_of(a0)?.wrapping_sub(long_of(a1)?)),
        "unchecked-multiply" => C::Long(long_of(a0)?.wrapping_mul(long_of(a1)?)),
        "quot" | "rem" | "mod" => int_div(name, a0, a1)?,
        "max" | "min" => {
            arity(name, args, |n| n > 0)?;
            let mut best = a0.clone();
            for x in &args[1..] {
                let o = num_cmp(x, &best).ok_or_else(|| num_err(x))?;
                if (name == "max" && o.is_gt()) || (name == "min" && o.is_lt()) {
                    best = x.clone();
                }
            }
            cat(&best).ok_or_else(|| num_err(&best))?;
            best
        }
        "abs" => match a0 {
            C::Long(i) => C::Long(i.checked_abs().ok_or("long overflow")?),
            C::Double(d) => C::Double(d.abs()),
            other => {
                if num_cmp(other, &C::Long(0)).ok_or_else(|| num_err(other))?.is_lt() {
                    arith(Op::Sub, &C::Long(0), other, true)?
                } else {
                    other.clone()
                }
            }
        },
        "=" => C::Bool(args.windows(2).all(|w| equiv(&w[0], &w[1]))),
        "not=" => C::Bool(!args.windows(2).all(|w| equiv(&w[0], &w[1]))),
        "==" => C::Bool(args.windows(2).map(|w| num_cmp(&w[0], &w[1]).ok_or_else(|| num_err(&w[0]))).collect::<R<Vec<_>>>()?.iter().all(|o| o.is_eq())),
        "<" | ">" | "<=" | ">=" => {
            let mut ok = true;
            for w in args.windows(2) {
                let o = num_cmp(&w[0], &w[1]).ok_or_else(|| num_err(if cat(&w[0]).is_none() { &w[0] } else { &w[1] }))?;
                ok &= match name {
                    "<" => o.is_lt(),
                    ">" => o.is_gt(),
                    "<=" => o.is_le(),
                    _ => o.is_ge(),
                };
            }
            if args.len() == 1 {
                cat(a0).ok_or_else(|| num_err(a0))?;
            }
            C::Bool(ok)
        }
        "compare" => C::Long(compare(a0, a1)? as i64),
        "not" => C::Bool(!truthy(a0)),
        "nil?" => C::Bool(matches!(a0, C::Nil)),
        "some?" => C::Bool(!matches!(a0, C::Nil)),
        "true?" => C::Bool(matches!(a0, C::Bool(true))),
        "false?" => C::Bool(matches!(a0, C::Bool(false))),
        "zero?" | "pos?" | "neg?" => {
            let o = num_cmp(a0, &C::Long(0)).ok_or_else(|| num_err(a0))?;
            C::Bool(match name {
                "zero?" => o.is_eq(),
                "pos?" => o.is_gt(),
                _ => o.is_lt(),
            })
        }
        "even?" | "odd?" => {
            let C::Long(i) = a0 else { return Err(format!("Argument must be an integer: {}", to_str(a0))) };
            C::Bool((i % 2 == 0) == (name == "even?"))
        }
        "number?" => bool_fn(|c| cat(c).is_some(), args)?,
        "integer?" => bool_fn(|c| matches!(c, C::Long(_) | C::Big(_)), args)?,
        "float?" => bool_fn(|c| matches!(c, C::Double(_)), args)?,
        "ratio?" => bool_fn(|c| matches!(c, C::Ratio(..)), args)?,
        "decimal?" => bool_fn(|c| matches!(c, C::Dec(_)), args)?,
        "string?" => bool_fn(|c| matches!(c, C::Str(_)), args)?,
        "char?" => bool_fn(|c| matches!(c, C::Char(_)), args)?,
        "keyword?" => bool_fn(|c| matches!(c, C::Kw(_)), args)?,
        "symbol?" => bool_fn(|c| matches!(c, C::Sym(_)), args)?,
        "boolean?" => bool_fn(|c| matches!(c, C::Bool(_)), args)?,
        "map?" => bool_fn(|c| matches!(c, C::Map(_)), args)?,
        "vector?" => bool_fn(|c| matches!(c, C::Vec(_)), args)?,
        "list?" | "seq?" => bool_fn(|c| matches!(c, C::List(_)), args)?,
        "set?" => bool_fn(|c| matches!(c, C::Set(_)), args)?,
        "coll?" => bool_fn(|c| matches!(c, C::List(_) | C::Vec(_) | C::Map(_) | C::Set(_)), args)?,
        "sequential?" => bool_fn(|c| matches!(c, C::List(_) | C::Vec(_)), args)?,
        "fn?" => bool_fn(|c| matches!(c, C::Fn(_) | C::Builtin(_)), args)?,
        "ifn?" => bool_fn(|c| matches!(c, C::Fn(_) | C::Builtin(_) | C::Kw(_) | C::Map(_) | C::Set(_) | C::Vec(_)), args)?,
        "empty?" => C::Bool(items(a0)?.is_empty()),
        "not-empty" => {
            if items(a0)?.is_empty() {
                C::Nil
            } else {
                a0.clone()
            }
        }
        "identity" => a0.clone(),
        "count" => C::Long(match a0 {
            C::Str(x) => x.chars().count(),
            C::Map(m) => m.len(),
            other => items(other).map_err(|_| format!("count not supported on this type: {}", class_of(other)))?.len(),
        } as i64),
        "str" => s(&args.iter().map(to_str).collect::<String>()),
        "subs" => {
            let t: Vec<char> = text(a0)?.chars().collect();
            let b = long_of(a1)?;
            let e = if args.len() > 2 { long_of(&args[2])? } else { t.len() as i64 };
            if b < 0 || e > t.len() as i64 || b > e {
                return Err(format!("begin {b}, end {e}, length {}", t.len()));
            }
            s(&t[b as usize..e as usize].iter().collect::<String>())
        }
        "format" => s(&java_format(text(a0)?, &args[1..])?),
        "first" => items(a0)?.into_iter().next().unwrap_or(C::Nil),
        "second" => items(a0)?.into_iter().nth(1).unwrap_or(C::Nil),
        "rest" => list(items(a0)?.into_iter().skip(1).collect()),
        "next" => seq_or_nil(items(a0)?.into_iter().skip(1).collect()),
        "last" => items(a0)?.pop().unwrap_or(C::Nil),
        "butlast" => {
            let mut v = items(a0)?;
            v.pop();
            seq_or_nil(v)
        }
        "nth" => {
            let v = items(a0)?;
            let i = as_index(a1)?;
            match v.get(i) {
                Some(x) => x.clone(),
                None if args.len() > 2 => args[2].clone(),
                None => return Err("null".into()),
            }
        }
        "nthrest" => list(items(a0)?.into_iter().skip(as_index(a1)?).collect()),
        "get" => get(a0, a1).or_else(|| args.get(2).cloned()).unwrap_or(C::Nil),
        "get-in" => {
            let mut v = a0.clone();
            for k in items(a1)? {
                v = get(&v, &k).unwrap_or(C::Nil);
            }
            if matches!(v, C::Nil) { args.get(2).cloned().unwrap_or(C::Nil) } else { v }
        }
        "keys" | "vals" => match a0 {
            C::Map(m) => seq_or_nil(m.iter().map(|(k, v)| if name == "keys" { k.clone() } else { v.clone() }).collect()),
            C::Nil => C::Nil,
            other => return Err(format!("Don't know how to create ISeq from: {}", class_of(other))),
        },
        "key" | "val" => match a0 {
            C::Vec(p) if p.len() == 2 => p[if name == "key" { 0 } else { 1 }].clone(),
            other => return Err(format!("class {} cannot be cast to class java.util.Map$Entry", class_of(other))),
        },
        "assoc" => {
            let mut c = a0.clone();
            for p in args[1..].chunks(2) {
                c = assoc(&c, &p[0], p.get(1).cloned().unwrap_or(C::Nil))?;
            }
            c
        }
        "assoc-in" => assoc_path(a0, &items(a1)?, arg(args, 2).clone())?,
        "update" => {
            let old = get(a0, a1).unwrap_or(C::Nil);
            let mut fargs = vec![old];
            fargs.extend(args[3.min(args.len())..].iter().cloned());
            assoc(a0, a1, apply(arg(args, 2), &fargs)?)?
        }
        "dissoc" => match a0 {
            C::Map(m) => C::Map(Rc::new(m.iter().filter(|(k, _)| !args[1..].iter().any(|d| equiv(d, k))).cloned().collect())),
            other => other.clone(),
        },
        "conj" => {
            let mut c = if args.is_empty() { C::Vec(Rc::new(vec![])) } else { a0.clone() };
            for x in args.iter().skip(1) {
                c = conj(&c, x.clone())?;
            }
            c
        }
        "cons" => list([vec![a0.clone()], items(a1)?].concat()),
        "concat" => list(args.iter().map(items).collect::<R<Vec<_>>>()?.concat()),
        "list*" => {
            let (last, init) = args.split_last().ok_or("Wrong number of args")?;
            list([init.to_vec(), items(last)?].concat())
        }
        "into" => {
            let mut c = if args.is_empty() { C::Vec(Rc::new(vec![])) } else { a0.clone() };
            for x in items(a1)? {
                c = conj(&c, x)?;
            }
            c
        }
        "vec" => C::Vec(Rc::new(items(a0)?)),
        "list" => list(args.to_vec()),
        "vector" => C::Vec(Rc::new(args.to_vec())),
        "hash-map" | "array-map" => {
            let mut m = vec![];
            for p in args.chunks(2) {
                assoc_in(&mut m, p[0].clone(), p.get(1).cloned().unwrap_or(C::Nil));
            }
            C::Map(Rc::new(m))
        }
        "hash-set" => C::Set(Rc::new(dedupe(args.to_vec()))),
        "set" => C::Set(Rc::new(dedupe(items(a0)?))),
        "seq" | "sequence" | "doall" | "dorun" => {
            if name == "dorun" {
                C::Nil
            } else if name == "seq" {
                seq_or_nil(items(a0)?)
            } else {
                list(items(a0)?)
            }
        }
        "rseq" | "reverse" => {
            let mut v = items(a0)?;
            v.reverse();
            if name == "rseq" { seq_or_nil(v) } else { list(v) }
        }
        "sort" | "sort-by" => {
            let (key, cmp_fn, coll) = match (name, args.len()) {
                ("sort", 1) => (None, None, a0),
                ("sort", _) => (None, Some(a0), a1),
                (_, 2) => (Some(a0), None, a1),
                _ => (Some(a0), Some(a1), arg(args, 2)),
            };
            let mut v: Vec<(C, C)> = items(coll)?.into_iter().map(|x| Ok((key.map(|k| apply(k, &[x.clone()])).transpose()?.unwrap_or_else(|| x.clone()), x))).collect::<R<_>>()?;
            let mut err = None;
            v.sort_by(|a, b| {
                let r = match cmp_fn {
                    Some(f) => apply(f, &[a.0.clone(), b.0.clone()]).and_then(|r| match r {
                        C::Bool(true) => Ok(std::cmp::Ordering::Less),
                        C::Bool(false) | C::Nil => Ok(if truthy(&apply(f, &[b.0.clone(), a.0.clone()])?) { std::cmp::Ordering::Greater } else { std::cmp::Ordering::Equal }),
                        n => Ok(long_of(&n)?.cmp(&0)),
                    }),
                    None => compare(&a.0, &b.0).map(|o| o.cmp(&0)),
                };
                r.unwrap_or_else(|e| {
                    err.get_or_insert(e);
                    std::cmp::Ordering::Equal
                })
            });
            if let Some(e) = err {
                return Err(e);
            }
            list(v.into_iter().map(|p| p.1).collect())
        }
        "distinct" => list(dedupe(items(a0)?)),
        "dedupe" => {
            let mut out: Vec<C> = vec![];
            for x in items(a0)? {
                if !out.last().is_some_and(|l| equiv(l, &x)) {
                    out.push(x);
                }
            }
            list(out)
        }
        "frequencies" => {
            let mut m: Vec<(C, C)> = vec![];
            for x in items(a0)? {
                match m.iter_mut().find(|(k, _)| equiv(k, &x)) {
                    Some(slot) => slot.1 = C::Long(long_of(&slot.1)? + 1),
                    None => m.push((x, C::Long(1))),
                }
            }
            C::Map(Rc::new(m))
        }
        "group-by" => {
            let mut m: Vec<(C, Vec<C>)> = vec![];
            for x in items(a1)? {
                let k = apply(a0, &[x.clone()])?;
                match m.iter_mut().find(|(k2, _)| equiv(k2, &k)) {
                    Some(slot) => slot.1.push(x),
                    None => m.push((k, vec![x])),
                }
            }
            C::Map(Rc::new(m.into_iter().map(|(k, v)| (k, C::Vec(Rc::new(v)))).collect()))
        }
        "zipmap" => {
            let mut m = vec![];
            for (k, v) in items(a0)?.into_iter().zip(items(a1)?) {
                assoc_in(&mut m, k, v);
            }
            C::Map(Rc::new(m))
        }
        "map" | "mapv" => {
            arity(name, args, |n| n >= 2)?;
            let colls = args[1..].iter().map(items).collect::<R<Vec<_>>>()?;
            let n = colls.iter().map(|c| c.len()).min().unwrap_or(0);
            let out = (0..n).map(|i| apply(a0, &colls.iter().map(|c| c[i].clone()).collect::<Vec<_>>())).collect::<R<Vec<_>>>()?;
            if name == "mapv" { C::Vec(Rc::new(out)) } else { list(out) }
        }
        "map-indexed" => list(items(a1)?.into_iter().enumerate().map(|(i, x)| apply(a0, &[C::Long(i as i64), x])).collect::<R<_>>()?),
        "mapcat" => {
            let mut out = vec![];
            for x in call("map", args).and_then(|m| items(&m))? {
                out.extend(items(&x)?);
            }
            list(out)
        }
        "filter" | "filterv" | "remove" | "keep" => {
            let mut out = vec![];
            for x in items(a1)? {
                let r = apply(a0, &[x.clone()])?;
                match name {
                    "keep" if !matches!(r, C::Nil) => out.push(r),
                    "remove" if !truthy(&r) => out.push(x),
                    "filter" | "filterv" if truthy(&r) => out.push(x),
                    _ => {}
                }
            }
            if name == "filterv" { C::Vec(Rc::new(out)) } else { list(out) }
        }
        "reduce" | "reductions" => {
            let (mut acc, xs) = if args.len() == 3 {
                (a1.clone(), items(&args[2])?)
            } else {
                let mut v = items(a1)?;
                if v.is_empty() {
                    return apply(a0, &[]);
                }
                (v.remove(0), v)
            };
            let mut steps = vec![acc.clone()];
            for x in xs {
                acc = apply(a0, &[acc, x])?;
                steps.push(acc.clone());
            }
            if name == "reductions" { list(steps) } else { acc }
        }
        "apply" => {
            arity(name, args, |n| n >= 2)?;
            let (last, init) = args[1..].split_last().unwrap();
            apply(a0, &[init.to_vec(), items(last)?].concat())?
        }
        "take" | "drop" | "take-last" | "drop-last" => {
            let (n, coll) = if args.len() == 1 && name == "drop-last" { (1, a0) } else { (long_of(a0)?.max(0) as usize, a1) };
            let v = items(coll)?;
            let len = v.len();
            list(match name {
                "take" => v.into_iter().take(n).collect(),
                "drop" => v.into_iter().skip(n).collect(),
                "take-last" => v.into_iter().skip(len.saturating_sub(n)).collect(),
                _ => v.into_iter().take(len.saturating_sub(n)).collect(),
            })
        }
        "take-while" | "drop-while" | "split-with" => {
            let v = items(a1)?;
            let mut i = 0;
            while i < v.len() && truthy(&apply(a0, &[v[i].clone()])?) {
                i += 1;
            }
            match name {
                "take-while" => list(v[..i].to_vec()),
                "drop-while" => list(v[i..].to_vec()),
                _ => C::Vec(Rc::new(vec![list(v[..i].to_vec()), list(v[i..].to_vec())])),
            }
        }
        "split-at" => {
            let v = items(a1)?;
            let n = (long_of(a0)?.max(0) as usize).min(v.len());
            C::Vec(Rc::new(vec![list(v[..n].to_vec()), list(v[n..].to_vec())]))
        }
        "range" => {
            let (start, end, step) = match args.len() {
                0 => return Err("Infinite sequences (range) aren't supported".into()),
                1 => (C::Long(0), a0.clone(), C::Long(1)),
                2 => (a0.clone(), a1.clone(), C::Long(1)),
                _ => (a0.clone(), a1.clone(), args[2].clone()),
            };
            let mut out = vec![];
            let mut x = start;
            let up = num_cmp(&step, &C::Long(0)).ok_or_else(|| num_err(&step))?;
            if up.is_eq() {
                return Err("Infinite sequences (range with step 0) aren't supported".into());
            }
            while num_cmp(&x, &end).ok_or_else(|| num_err(&end))? == if up.is_gt() { std::cmp::Ordering::Less } else { std::cmp::Ordering::Greater } {
                out.push(x.clone());
                if out.len() > 10_000_000 {
                    return Err("range too long".into());
                }
                x = arith(Op::Add, &x, &step, false)?;
            }
            list(out)
        }
        "repeat" => {
            if args.len() < 2 {
                return Err("Infinite sequences (repeat) aren't supported".into());
            }
            list(vec![a1.clone(); long_of(a0)?.max(0) as usize])
        }
        "interpose" => {
            let mut out = vec![];
            for (i, x) in items(a1)?.into_iter().enumerate() {
                if i > 0 {
                    out.push(a0.clone());
                }
                out.push(x);
            }
            list(out)
        }
        "interleave" => {
            let colls = args.iter().map(items).collect::<R<Vec<_>>>()?;
            let n = colls.iter().map(|c| c.len()).min().unwrap_or(0);
            list((0..n).flat_map(|i| colls.iter().map(move |c| c[i].clone())).collect())
        }
        "partition" | "partition-all" => {
            let n = long_of(a0)?.max(1) as usize;
            let v = items(arg(args, args.len() - 1))?;
            let step = if args.len() > 2 { long_of(a1)?.max(1) as usize } else { n };
            let mut out = vec![];
            let mut i = 0;
            while i < v.len() {
                let chunk = &v[i..(i + n).min(v.len())];
                if chunk.len() == n || name == "partition-all" {
                    out.push(list(chunk.to_vec()));
                }
                i += step;
            }
            list(out)
        }
        "partition-by" => {
            let mut out: Vec<(C, Vec<C>)> = vec![];
            for x in items(a1)? {
                let k = apply(a0, &[x.clone()])?;
                match out.last_mut() {
                    Some((k2, v)) if equiv(k2, &k) => v.push(x),
                    _ => out.push((k, vec![x])),
                }
            }
            list(out.into_iter().map(|(_, v)| list(v)).collect())
        }
        "flatten" => {
            fn flat(c: &C, out: &mut Vec<C>) {
                match c {
                    C::List(l) | C::Vec(l) => l.iter().for_each(|x| flat(x, out)),
                    other => out.push(other.clone()),
                }
            }
            let mut out = vec![];
            if matches!(a0, C::List(_) | C::Vec(_)) {
                flat(a0, &mut out);
            }
            list(out)
        }
        "some" => {
            for x in items(a1)? {
                let r = apply(a0, &[x])?;
                if truthy(&r) {
                    return Ok(r);
                }
            }
            C::Nil
        }
        "every?" | "not-every?" | "not-any?" => {
            let mut all = true;
            let mut any = false;
            for x in items(a1)? {
                let t = truthy(&apply(a0, &[x])?);
                all &= t;
                any |= t;
            }
            C::Bool(match name {
                "every?" => all,
                "not-every?" => !all,
                _ => !any,
            })
        }
        "contains?" => C::Bool(match a0 {
            C::Map(_) | C::Set(_) => get(a0, a1).is_some() || matches!(a0, C::Map(m) if m.iter().any(|(k, _)| equiv(k, a1))),
            C::Vec(v) => matches!(a1, C::Long(i) if *i >= 0 && (*i as usize) < v.len()),
            C::Str(t) => matches!(a1, C::Long(i) if *i >= 0 && (*i as usize) < t.chars().count()),
            C::Nil => false,
            other => return Err(format!("contains? not supported on type: {}", class_of(other))),
        }),
        "merge" => {
            let mut m: Vec<(C, C)> = vec![];
            for x in args {
                if let C::Map(p) = x {
                    for (k, v) in p.iter() {
                        assoc_in(&mut m, k.clone(), v.clone());
                    }
                }
            }
            if args.iter().all(|a| matches!(a, C::Nil)) { C::Nil } else { C::Map(Rc::new(m)) }
        }
        "select-keys" => {
            let mut m = vec![];
            for k in items(a1)? {
                if let Some(v) = get(a0, &k) {
                    m.push((k, v));
                }
            }
            C::Map(Rc::new(m))
        }
        "find" => get(a0, a1).map(|v| C::Vec(Rc::new(vec![a1.clone(), v]))).unwrap_or(C::Nil),
        "keyword" => match a0 {
            C::Nil => C::Nil,
            C::Kw(k) => C::Kw(k.clone()),
            C::Str(x) | C::Sym(x) => C::Kw(if args.len() > 1 { format!("{x}/{}", text(a1)?).into() } else { x.clone() }),
            _ => C::Nil,
        },
        "symbol" => C::Sym(to_str(a0).into()),
        "name" => match a0 {
            C::Kw(k) | C::Sym(k) => s(k.rsplit('/').next().unwrap_or(k)),
            C::Str(x) => C::Str(x.clone()),
            other => return Err(format!("class {} cannot be cast to class clojure.lang.Named", class_of(other))),
        },
        "namespace" => match a0 {
            C::Kw(k) | C::Sym(k) => k.rsplit_once('/').map(|(n, _)| s(n)).unwrap_or(C::Nil),
            other => return Err(format!("class {} cannot be cast to class clojure.lang.Named", class_of(other))),
        },
        "int" | "long" | "short" | "byte" => match a0 {
            C::Double(d) if !d.is_finite() || d.abs() >= 9.3e18 => return Err(format!("Value out of range for long: {}", java_double(*d))),
            other => C::Long(long_of(other)?),
        },
        "double" | "float" | "num" => C::Double(to_f64(a0)?),
        "char" => char_fn(args, C::Char)?,
        "boolean" => C::Bool(truthy(a0)),
        "bigint" => C::Big(long_of(a0)? as i128),
        "bigdec" => C::Dec(to_dec(a0)),
        "rationalize" => a0.clone(),
        "numerator" | "denominator" => match a0 {
            C::Ratio(n, d) => int(if name == "numerator" { *n } else { *d }),
            other => return Err(format!("class {} cannot be cast to class clojure.lang.Ratio", class_of(other))),
        },
        "pr-str" | "prn-str" => s(&format!("{}{}", args.iter().map(pr).collect::<Vec<_>>().join(" "), if name == "prn-str" { "\n" } else { "" })),
        "print-str" | "println-str" => s(&format!("{}{}", args.iter().map(to_str).collect::<Vec<_>>().join(" "), if name == "println-str" { "\n" } else { "" })),
        "println" | "print" | "prn" | "pr" => C::Nil,
        "re-pattern" => match a0 {
            C::Re(..) => a0.clone(),
            other => regex(text(other)?)?,
        },
        "re-find" => {
            let re = pattern(a0)?;
            re.captures(text(a1)?).map(|c| match_value(&c)).unwrap_or(C::Nil)
        }
        "re-seq" => {
            let re = pattern(a0)?;
            seq_or_nil(re.captures_iter(text(a1)?).map(|c| match_value(&c)).collect())
        }
        "re-matches" => {
            let re = pattern(a0)?;
            let t = text(a1)?;
            re.captures(t).filter(|c| c.get(0).is_some_and(|m| m.start() == 0 && m.end() == t.len())).map(|c| match_value(&c)).unwrap_or(C::Nil)
        }
        "type" | "class" => match a0 {
            C::Nil => C::Nil,
            other => C::Obj(Rc::new(Obj::Class(class_of(other).into()))),
        },
        "instance?" => {
            let C::Obj(o) = a0 else { return Err(format!("class {} cannot be cast to class java.lang.Class", class_of(a0))) };
            let Obj::Class(c) = &**o else { return Err("not a class".into()) };
            C::Bool(is_instance(c, a1))
        }
        "hash" => C::Long(match a0 {
            C::Nil => 0,
            C::Str(x) => murmur_int(java_hash(x)),
            C::Long(i) => murmur_long(*i),
            C::Bool(b) => if *b { 1231 } else { 1237 },
            C::Kw(k) => murmur_int(java_hash(k)).wrapping_add(0x9e3779b9u32 as i32),
            other => murmur_int(java_hash(&to_str(other))),
        } as i64),
        "bit-and" => C::Long(args.iter().map(long_of).collect::<R<Vec<_>>>()?.into_iter().reduce(|a, b| a & b).unwrap_or(-1)),
        "bit-or" => C::Long(args.iter().map(long_of).collect::<R<Vec<_>>>()?.into_iter().reduce(|a, b| a | b).unwrap_or(0)),
        "bit-xor" => C::Long(args.iter().map(long_of).collect::<R<Vec<_>>>()?.into_iter().reduce(|a, b| a ^ b).unwrap_or(0)),
        "bit-not" => C::Long(!long_of(a0)?),
        "bit-shift-left" => C::Long(long_of(a0)?.wrapping_shl(long_of(a1)? as u32)),
        "bit-shift-right" => C::Long(long_of(a0)?.wrapping_shr(long_of(a1)? as u32)),
        "comp" | "partial" | "constantly" | "complement" | "juxt" | "fnil" => {
            // closures over builtins: build a Lambda whose body calls back into this function
            return Ok(C::Fn(Rc::new(Lambda {
                name: None,
                arities: vec![Arity { params: vec![], rest: Some("args".into()), body: vec![list(vec![C::Builtin("__combinator"), s(name), list(vec![sym("quote"), C::Vec(Rc::new(args.to_vec()))]), sym("args")])] }],
                env: None,
            })));
        }
        "__combinator" => {
            let C::Str(kind) = a0 else { unreachable!() };
            let C::Vec(fs) = a1 else { unreachable!() };
            let xs = items(arg(args, 2))?;
            match &**kind {
                "comp" => {
                    let mut v = xs;
                    let mut r = C::Nil;
                    if fs.is_empty() {
                        return Ok(v.into_iter().next().unwrap_or(C::Nil));
                    }
                    for (i, f) in fs.iter().rev().enumerate() {
                        r = apply(f, &v)?;
                        if i + 1 < fs.len() {
                            v = vec![r.clone()];
                        }
                    }
                    r
                }
                "partial" => apply(&fs[0], &[fs[1..].to_vec(), xs].concat())?,
                "constantly" => fs.first().cloned().unwrap_or(C::Nil),
                "complement" => C::Bool(!truthy(&apply(&fs[0], &xs)?)),
                "juxt" => C::Vec(Rc::new(fs.iter().map(|f| apply(f, &xs)).collect::<R<_>>()?)),
                _ => {
                    let mut xs = xs;
                    for (i, d) in fs[1..].iter().enumerate() {
                        if matches!(xs.get(i), Some(C::Nil)) {
                            xs[i] = d.clone();
                        }
                    }
                    apply(&fs[0], &xs)?
                }
            }
        }
        "max-key" | "min-key" => {
            let mut best = a1.clone();
            let mut bk = apply(a0, &[best.clone()])?;
            for x in &args[2..] {
                let k = apply(a0, &[x.clone()])?;
                let o = num_cmp(&k, &bk).ok_or_else(|| num_err(&k))?;
                if (name == "max-key" && o.is_ge()) || (name == "min-key" && o.is_le()) {
                    best = x.clone();
                    bk = k;
                }
            }
            best
        }
        "empty" => match a0 {
            C::Vec(_) => C::Vec(Rc::new(vec![])),
            C::Map(_) => C::Map(Rc::new(vec![])),
            C::Set(_) => C::Set(Rc::new(vec![])),
            C::List(_) => list(vec![]),
            _ => C::Nil,
        },
        "peek" => match a0 {
            C::Vec(v) => v.last().cloned().unwrap_or(C::Nil),
            C::List(v) => v.first().cloned().unwrap_or(C::Nil),
            _ => C::Nil,
        },
        "pop" => match a0 {
            C::Vec(v) if !v.is_empty() => C::Vec(Rc::new(v[..v.len() - 1].to_vec())),
            C::List(v) if !v.is_empty() => list(v[1..].to_vec()),
            _ => return Err("Can't pop empty vector".into()),
        },
        "subvec" => {
            let v = items(a0)?;
            let b = long_of(a1)? as usize;
            let e = if args.len() > 2 { long_of(&args[2])? as usize } else { v.len() };
            if b > e || e > v.len() {
                return Err("Index out of bounds".into());
            }
            C::Vec(Rc::new(v[b..e].to_vec()))
        }
        "realized?" => C::Bool(true),
        "deref" => a0.clone(),
        "ex-info" => C::Obj(Rc::new(Obj::Exception("clojure.lang.ExceptionInfo".into(), to_str(a0)))),
        "ex-message" => match a0 {
            C::Obj(o) => match &**o {
                Obj::Exception(_, m) => s(m),
                _ => C::Nil,
            },
            _ => C::Nil,
        },
        "parse-long" => parse_java_int(text(a0)?, 64).unwrap_or(C::Nil),
        "parse-double" => parse_java_double(text(a0)?).unwrap_or(C::Nil),
        "rand" | "shuffle" | "gensym" => return Err(format!("{name} isn't supported in OxRefine expressions")),
        "trampoline" => {
            let mut r = apply(a0, &args[1..])?;
            while matches!(r, C::Fn(_)) {
                r = apply(&r, &[])?;
            }
            r
        }
        "replace" => {
            let v = items(a1)?;
            list(v.into_iter().map(|x| get(a0, &x).unwrap_or(x)).collect())
        }
        "char-escape-string" => C::Nil,
        // clojure.string
        "clojure.string/upper-case" => s(&to_str(&nonnull(a0)?).to_uppercase()),
        "clojure.string/lower-case" => s(&to_str(&nonnull(a0)?).to_lowercase()),
        "clojure.string/capitalize" => {
            let t = to_str(&nonnull(a0)?);
            let mut c = t.chars();
            match c.next() {
                Some(f) => s(&format!("{}{}", f.to_uppercase(), c.as_str().to_lowercase())),
                None => s(""),
            }
        }
        "clojure.string/trim" => s(text(a0)?.trim()),
        "clojure.string/triml" => s(text(a0)?.trim_start()),
        "clojure.string/trimr" => s(text(a0)?.trim_end()),
        "clojure.string/trim-newline" => s(text(a0)?.trim_end_matches(['\n', '\r'])),
        "clojure.string/reverse" => s(&text(a0)?.chars().rev().collect::<String>()),
        "clojure.string/replace" | "clojure.string/replace-first" => {
            let t = text(a0)?;
            let first = name.ends_with("first");
            match a1 {
                C::Str(m) => s(&if first { t.replacen(&**m, &to_str(arg(args, 2)), 1) } else { t.replace(&**m, &to_str(arg(args, 2))) }),
                C::Char(m) => s(&t.replace(*m, &to_str(arg(args, 2)))),
                C::Re(re, _) => match arg(args, 2) {
                    C::Str(rep) => {
                        let rep = java_replacement(rep);
                        s(&if first { re.replacen(t, 1, rep.as_str()) } else { re.replace_all(t, rep.as_str()) })
                    }
                    f => {
                        let mut err = None;
                        let out = re.replacen(t, if first { 1 } else { 0 }, |c: &regex::Captures| match apply(f, &[match_value(c)]) {
                            Ok(v) => to_str(&v),
                            Err(e) => {
                                err.get_or_insert(e);
                                String::new()
                            }
                        });
                        if let Some(e) = err {
                            return Err(e);
                        }
                        s(&out)
                    }
                },
                other => return Err(format!("Invalid match arg: {}", to_str(other))),
            }
        }
        "clojure.string/split" => {
            let t = text(a0)?;
            let re = pattern(a1)?;
            let limit = if args.len() > 2 { long_of(&args[2])? } else { 0 };
            C::Vec(Rc::new(java_split(t, &re, limit)))
        }
        "clojure.string/split-lines" => C::Vec(Rc::new(java_split(text(a0)?, &Regex::new(r"\r?\n").unwrap(), 0))),
        "clojure.string/join" => {
            let (sep, coll) = if args.len() > 1 { (to_str(a0), a1) } else { (String::new(), a0) };
            s(&items(coll)?.iter().map(to_str).collect::<Vec<_>>().join(&sep))
        }
        "clojure.string/blank?" => C::Bool(match a0 {
            C::Nil => true,
            other => text(other)?.trim().is_empty(),
        }),
        "clojure.string/includes?" => C::Bool(text(a0)?.contains(&*to_str(&nonnull(a1)?))),
        "clojure.string/starts-with?" => C::Bool(text(a0)?.starts_with(&*to_str(&nonnull(a1)?))),
        "clojure.string/ends-with?" => C::Bool(text(a0)?.ends_with(&*to_str(&nonnull(a1)?))),
        "clojure.string/index-of" | "clojure.string/last-index-of" => {
            let t = text(a0)?;
            let n = to_str(&nonnull(a1)?);
            let pos = if name.ends_with("last-index-of") { t.rfind(&*n) } else { t.find(&*n) };
            pos.map(|b| C::Long(t[..b].chars().count() as i64)).unwrap_or(C::Nil)
        }
        "clojure.string/escape" => {
            let t = text(a0)?;
            let mut out = String::new();
            for ch in t.chars() {
                match apply(a1, &[C::Char(ch)])? {
                    C::Nil => out.push(ch),
                    r => out.push_str(&to_str(&r)),
                }
            }
            s(&out)
        }
        // Java statics
        "Integer/parseInt" | "Integer/valueOf" => parse_java_int(text(a0)?, 32)?,
        "Long/parseLong" | "Long/valueOf" => parse_java_int(text(a0)?, 64)?,
        "Double/parseDouble" | "Double/valueOf" => parse_java_double(text(a0)?)?,
        "Boolean/parseBoolean" => C::Bool(matches!(a0, C::Str(x) if x.eq_ignore_ascii_case("true"))),
        "String/valueOf" => s(&if matches!(a0, C::Nil) { "null".to_string() } else { to_str(a0) }),
        "Long/toBinaryString" | "Integer/toBinaryString" => s(&format!("{:b}", long_of(a0)?)),
        "Long/toHexString" | "Integer/toHexString" => s(&format!("{:x}", long_of(a0)?)),
        "Long/toOctalString" | "Integer/toOctalString" => s(&format!("{:o}", long_of(a0)?)),
        "Long/toString" | "Integer/toString" => s(&long_of(a0)?.to_string()),
        "Math/round" => {
            let v = to_f64(a0)?;
            C::Long(if v.is_nan() { 0 } else { (v + 0.5).floor() as i64 })
        }
        "Math/abs" => match a0 {
            C::Long(i) => C::Long(i.wrapping_abs()),
            other => C::Double(to_f64(other)?.abs()),
        },
        "Math/max" | "Math/min" => match (a0, a1) {
            (C::Long(x), C::Long(y)) => C::Long(if name == "Math/max" { *x.max(y) } else { *x.min(y) }),
            _ => {
                let (x, y) = (to_f64(a0)?, to_f64(a1)?);
                C::Double(if name == "Math/max" { x.max(y) } else { x.min(y) })
            }
        },
        m if m.starts_with("Math/") => {
            let x = to_f64(a0)?;
            C::Double(match m {
                "Math/sqrt" => x.sqrt(),
                "Math/floor" => x.floor(),
                "Math/ceil" => x.ceil(),
                "Math/pow" => x.powf(to_f64(a1)?),
                "Math/log" => x.ln(),
                "Math/log10" => x.log10(),
                "Math/exp" => x.exp(),
                "Math/sin" => x.sin(),
                "Math/cos" => x.cos(),
                "Math/tan" => x.tan(),
                "Math/signum" => {
                    if x == 0.0 || x.is_nan() {
                        x
                    } else {
                        x.signum()
                    }
                }
                "Math/cbrt" => x.cbrt(),
                _ => {
                    let r = x.round();
                    if (x - x.trunc()).abs() == 0.5 { 2.0 * (x / 2.0).round() } else { r }
                }
            })
        }
        "Character/isLetter" | "Character/isAlphabetic" => char_fn(args, |c| C::Bool(c.is_alphabetic()))?,
        "Character/isDigit" => char_fn(args, |c| C::Bool(c.is_numeric()))?,
        "Character/isLetterOrDigit" => char_fn(args, |c| C::Bool(c.is_alphanumeric()))?,
        "Character/isUpperCase" => char_fn(args, |c| C::Bool(c.is_uppercase()))?,
        "Character/isLowerCase" => char_fn(args, |c| C::Bool(c.is_lowercase()))?,
        "Character/isWhitespace" => char_fn(args, |c| C::Bool(c.is_whitespace()))?,
        "Character/toUpperCase" => char_fn(args, |c| C::Char(c.to_uppercase().next().unwrap_or(c)))?,
        "Character/toLowerCase" => char_fn(args, |c| C::Char(c.to_lowercase().next().unwrap_or(c)))?,
        other => return Err(format!("{other} isn't available")),
    })
}

fn nonnull(c: &C) -> R<C> {
    if matches!(c, C::Nil) { Err("null".into()) } else { Ok(c.clone()) }
}

/// Java replacement strings use $1; the regex crate uses ${1}.
fn java_replacement(rep: &str) -> String {
    let mut out = String::new();
    let mut it = rep.chars().peekable();
    while let Some(c) = it.next() {
        match c {
            '\\' => {
                if let Some(n) = it.next() {
                    if n == '$' { out.push_str("$$") } else { out.push(n) }
                }
            }
            '$' => {
                let mut d = String::new();
                while let Some(&x) = it.peek().filter(|x| x.is_ascii_digit()) {
                    d.push(x);
                    it.next();
                }
                out.push_str(&format!("${{{d}}}"));
            }
            c => out.push(c),
        }
    }
    out
}

fn compare(a: &C, b: &C) -> R<i32> {
    if let (Some(_), Some(_)) = (cat(a), cat(b)) {
        return Ok(num_cmp(a, b).map_or(0, |o| o as i32));
    }
    Ok(match (a, b) {
        (C::Nil, C::Nil) => 0,
        (C::Nil, _) => -1,
        (_, C::Nil) => 1,
        (C::Str(x), C::Str(y)) => {
            let (x, y): (Vec<u16>, Vec<u16>) = (x.encode_utf16().collect(), y.encode_utf16().collect());
            for (p, q) in x.iter().zip(&y) {
                if p != q {
                    return Ok(*p as i32 - *q as i32);
                }
            }
            x.len() as i32 - y.len() as i32
        }
        (C::Kw(x), C::Kw(y)) | (C::Sym(x), C::Sym(y)) => x.cmp(y) as i32,
        (C::Char(x), C::Char(y)) => *x as i32 - *y as i32,
        (C::Bool(x), C::Bool(y)) => x.cmp(y) as i32,
        (C::Vec(x), C::Vec(y)) => {
            if x.len() != y.len() {
                return Ok(if x.len() < y.len() { -1 } else { 1 });
            }
            for (p, q) in x.iter().zip(y.iter()) {
                let c = compare(p, q)?;
                if c != 0 {
                    return Ok(c);
                }
            }
            0
        }
        _ => return Err(format!("class {} cannot be cast to class java.lang.Comparable", class_of(a))),
    })
}

fn assoc(c: &C, k: &C, v: C) -> R<C> {
    match c {
        C::Nil => Ok(C::Map(Rc::new(vec![(k.clone(), v)]))),
        C::Map(m) => {
            let mut m = m.to_vec();
            assoc_in(&mut m, k.clone(), v);
            Ok(C::Map(Rc::new(m)))
        }
        C::Vec(x) => {
            let i = as_index(k)?;
            let mut x = x.to_vec();
            match i.cmp(&x.len()) {
                std::cmp::Ordering::Less => x[i] = v,
                std::cmp::Ordering::Equal => x.push(v),
                _ => return Err("Index out of bounds".into()),
            }
            Ok(C::Vec(Rc::new(x)))
        }
        other => Err(format!("class {} cannot be cast to class clojure.lang.Associative", class_of(other))),
    }
}

fn assoc_path(c: &C, path: &[C], v: C) -> R<C> {
    match path {
        [] => Ok(v),
        [k, rest @ ..] => assoc(c, k, assoc_path(&get(c, k).unwrap_or(C::Nil), rest, v)?),
    }
}

fn conj(c: &C, x: C) -> R<C> {
    Ok(match c {
        C::Nil => list(vec![x]),
        C::Vec(v) => C::Vec(Rc::new([v.to_vec(), vec![x]].concat())),
        C::List(v) => list([vec![x], v.to_vec()].concat()),
        C::Set(v) => C::Set(Rc::new(dedupe([v.to_vec(), vec![x]].concat()))),
        C::Map(_) => match &x {
            C::Vec(p) if p.len() == 2 => assoc(c, &p[0], p[1].clone())?,
            C::Map(p) => {
                let mut out = c.clone();
                for (k, v) in p.iter() {
                    out = assoc(&out, k, v.clone())?;
                }
                out
            }
            _ => return Err("Vector arg to map conj must be a pair".into()),
        },
        other => return Err(format!("class {} cannot be cast to class clojure.lang.IPersistentCollection", class_of(other))),
    })
}

// ------------------------------------------------------------------ Java interop

fn field(o: &Obj, name: &str) -> R<Option<C>> {
    Ok(match o {
        Obj::Cell(v) => match name {
            "value" => Some(from_value(v)),
            "recon" => None,
            _ => None,
        },
        Obj::Cells(cells) => cells.iter().find(|(n, _)| n == name).and_then(|(_, c)| c.clone()),
        Obj::Row { index, starred, flagged, cells } => match name {
            "index" => Some(C::Long(*index)),
            "starred" => Some(C::Bool(*starred)),
            "flagged" => Some(C::Bool(*flagged)),
            "cells" => Some(cells.clone()),
            _ => None,
        },
        _ => return Err("No matching method getField".into()),
    })
}

fn method(target: &C, m: &str, args: &[C]) -> R<C> {
    let a0 = arg(args, 0);
    let no_method = || format!("No matching method {m} found taking {} args for class {}", args.len(), class_of(target));
    if let Some(f) = m.strip_prefix('-') {
        return Err(format!("No matching field found: {f} for class {}", class_of(target)));
    }
    match target {
        C::Nil => Err("null".into()),
        C::Obj(o) => match (m, &**o) {
            ("getField", Obj::Cell(_) | Obj::Cells(_) | Obj::Row { .. }) => Ok(field(o, &to_str(a0))?.unwrap_or(C::Nil)),
            ("getMessage", Obj::Exception(_, msg)) => Ok(s(msg)),
            ("toString", _) => Ok(s(&to_str(target))),
            ("getTime", Obj::Date(d)) => Ok(C::Long(d.and_utc().timestamp_millis())),
            (_, Obj::Cell(_)) if args.is_empty() => Err(format!("No matching field found: {m} for class com.google.refine.expr.WrappedCell")),
            _ => Err(no_method()),
        },
        C::Str(t) => {
            let chars = || t.chars().collect::<Vec<_>>();
            let idx = |i: i64, len: usize| -> R<usize> { if i < 0 || i as usize > len { Err(format!("begin {i}, end {len}, length {len}")) } else { Ok(i as usize) } };
            Ok(match (m, args.len()) {
                ("toUpperCase", 0) => s(&t.to_uppercase()),
                ("toLowerCase", 0) => s(&t.to_lowercase()),
                ("length", 0) => C::Long(t.encode_utf16().count() as i64),
                ("isEmpty", 0) => C::Bool(t.is_empty()),
                ("isBlank", 0) => C::Bool(t.trim().is_empty()),
                ("trim", 0) => s(t.trim_matches(|c: char| c <= ' ')),
                ("strip", 0) => s(t.trim()),
                ("toString", 0) | ("intern", 0) => target.clone(),
                ("hashCode", 0) => C::Long(java_hash(t) as i64),
                ("toCharArray", 0) => C::Vec(Rc::new(t.chars().map(C::Char).collect())),
                ("substring", 1 | 2) => {
                    let c = chars();
                    let b = idx(long_of(a0)?, c.len())?;
                    let e = if args.len() == 2 { idx(long_of(&args[1])?, c.len())? } else { c.len() };
                    if b > e {
                        return Err(format!("begin {b}, end {e}, length {}", c.len()));
                    }
                    s(&c[b..e].iter().collect::<String>())
                }
                ("charAt", 1) => chars().get(as_index(a0)?).copied().map(C::Char).ok_or_else(|| format!("Index {} out of bounds for length {}", to_str(a0), t.chars().count()))?,
                ("indexOf" | "lastIndexOf", 1 | 2) => {
                    let needle = match a0 {
                        C::Char(c) => c.to_string(),
                        C::Long(i) => char::from_u32(*i as u32).map(String::from).unwrap_or_default(),
                        other => text(other)?.to_string(),
                    };
                    let from = if args.len() == 2 { long_of(&args[1])?.max(0) as usize } else { 0 };
                    let byte_from = t.char_indices().nth(from).map_or(t.len(), |(b, _)| b);
                    let pos = if m == "indexOf" { t[byte_from..].find(&needle).map(|p| p + byte_from) } else { t.rfind(&needle) };
                    C::Long(pos.map_or(-1, |b| t[..b].chars().count() as i64))
                }
                ("contains", 1) => C::Bool(t.contains(&*to_str(&nonnull(a0)?))),
                ("startsWith", 1) => C::Bool(t.starts_with(text(a0)?)),
                ("endsWith", 1) => C::Bool(t.ends_with(text(a0)?)),
                ("equals", 1) => C::Bool(matches!(a0, C::Str(o) if o == t)),
                ("equalsIgnoreCase", 1) => C::Bool(matches!(a0, C::Str(o) if o.to_lowercase() == t.to_lowercase())),
                ("concat", 1) => s(&format!("{t}{}", text(a0)?)),
                ("replace", 2) => s(&t.replace(&*to_str(a0), &to_str(&args[1]))),
                ("replaceAll" | "replaceFirst", 2) => {
                    let re = Regex::new(&java_regex(text(a0)?)).map_err(|e| e.to_string())?;
                    let rep = java_replacement(text(&args[1])?);
                    s(&if m == "replaceAll" { re.replace_all(t, rep.as_str()) } else { re.replacen(t, 1, rep.as_str()) })
                }
                ("matches", 1) => {
                    let re = Regex::new(&format!("^(?:{})$", java_regex(text(a0)?))).map_err(|e| e.to_string())?;
                    C::Bool(re.is_match(t))
                }
                ("split", 1 | 2) => {
                    let re = Regex::new(&java_regex(text(a0)?)).map_err(|e| e.to_string())?;
                    let limit = if args.len() == 2 { long_of(&args[1])? } else { 0 };
                    C::Vec(Rc::new(java_split(t, &re, limit)))
                }
                _ => return Err(no_method()),
            })
        }
        C::Long(_) | C::Double(_) | C::Big(_) | C::Dec(_) | C::Ratio(..) => Ok(match (m, args.len()) {
            ("toString", 0) => s(&to_str(target)),
            ("intValue" | "longValue", 0) => C::Long(long_of(target)?),
            ("doubleValue" | "floatValue", 0) => C::Double(to_f64(target)?),
            _ => return Err(no_method()),
        }),
        _ if m == "toString" && args.is_empty() => Ok(s(&to_str(target))),
        _ if m == "equals" && args.len() == 1 => Ok(C::Bool(equiv(target, a0))),
        _ => Err(no_method()),
    }
}

fn construct(class: &str, args: &[C]) -> R<C> {
    let a0 = arg(args, 0);
    Ok(match class {
        "java.lang.Exception" | "java.lang.RuntimeException" | "java.lang.IllegalArgumentException" | "java.lang.IllegalStateException" | "java.lang.Throwable" => {
            C::Obj(Rc::new(Obj::Exception(class.into(), if args.is_empty() { "null".into() } else { to_str(a0) })))
        }
        "java.lang.Long" | "java.lang.Integer" => match a0 {
            C::Str(t) => parse_java_int(t, if class.ends_with("Long") { 64 } else { 32 })?,
            other => C::Long(long_of(other)?),
        },
        "java.lang.Double" => match a0 {
            C::Str(t) => parse_java_double(t)?,
            other => C::Double(to_f64(other)?),
        },
        "java.lang.String" => s(&if args.is_empty() { String::new() } else { to_str(a0) }),
        "java.util.Date" => {
            let ms = if args.is_empty() { 0 } else { long_of(a0)? };
            let d = chrono::DateTime::from_timestamp_millis(ms).ok_or("Invalid date")?.naive_utc();
            C::Str(d.format("%a %b %d %H:%M:%S UTC %Y").to_string().into())
        }
        other => return Err(format!("No matching ctor found for class {other}")),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn run(src: &str) -> Value {
        check(src).unwrap();
        eval(src, Value::Str("ab".into()), &super::super::NoRow)
    }
    #[test]
    fn basics() {
        assert_eq!(run("(+ 1 2)"), Value::Int(3));
        assert_eq!(run("(/ 1 2)").render().unwrap(), "1/2");
        assert_eq!(run("(clojure.string/upper-case value)"), Value::Str("AB".into()));
        assert_eq!(run("(loop [i 0 acc 1] (if (< i 3) (recur (inc i) (* acc 2)) acc))"), Value::Int(8));
        assert_eq!(run("(hash \"a\")"), Value::Int(1455541201));
        assert!(check("(undefined-fn 1)").is_err());
        assert!(check("(+ 1").is_err());
    }
}
