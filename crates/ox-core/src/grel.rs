//! The slice of GREL that OpenRefine's "Common transforms" and most recipes use.
use crate::{cluster::fingerprint, err, Result};
use regex::Regex;

pub enum Expr {
    Const(String),
    Value(Vec<(String, Vec<Arg>)>),
}

pub enum Arg {
    Str(String),
    Re(Regex),
}

const HELP: &str = "Supported GREL: value, null, \"text\", and chains like value.trim().toLowercase() using \
trim, strip, toLowercase, toUppercase, toTitlecase, toString, fingerprint, replace(\"a\" or /regex/, \"b\")";

pub fn parse(src: &str) -> Result<Expr> {
    let s = src.trim();
    let s = s.strip_prefix("grel:").unwrap_or(s);
    P { s: s.as_bytes(), i: 0 }.expr().map_err(|e| format!("{e}. {HELP}"))
}

impl Expr {
    pub fn eval(&self, v: &str) -> String {
        match self {
            Expr::Const(s) => s.clone(),
            Expr::Value(calls) => calls.iter().fold(v.to_string(), |s, (f, a)| call(f, a, s)),
        }
    }
}

fn call(f: &str, a: &[Arg], s: String) -> String {
    match (f, a) {
        ("trim" | "strip", _) => s.trim().into(),
        ("toLowercase", _) => s.to_lowercase(),
        ("toUppercase", _) => s.to_uppercase(),
        ("toTitlecase", _) => {
            let mut up = true;
            s.chars()
                .flat_map(|c| {
                    let out: Vec<char> = if up { c.to_uppercase().collect() } else { c.to_lowercase().collect() };
                    up = c.is_whitespace();
                    out
                })
                .collect()
        }
        ("fingerprint", _) => fingerprint(&s),
        ("replace", [Arg::Str(x), Arg::Str(y)]) => s.replace(x.as_str(), y),
        ("replace", [Arg::Re(x), Arg::Str(y)]) => x.replace_all(&s, y.as_str()).into_owned(),
        _ => s, // toString
    }
}

struct P<'a> {
    s: &'a [u8],
    i: usize,
}

impl P<'_> {
    fn peek(&mut self) -> Option<u8> {
        while self.s.get(self.i).is_some_and(u8::is_ascii_whitespace) {
            self.i += 1;
        }
        self.s.get(self.i).copied()
    }
    fn eat(&mut self, c: u8) -> bool {
        let hit = self.peek() == Some(c);
        self.i += hit as usize;
        hit
    }
    fn rest(&self) -> String {
        String::from_utf8_lossy(&self.s[self.i.min(self.s.len())..]).into()
    }
    fn ident(&mut self) -> String {
        self.peek();
        let st = self.i;
        while self.s.get(self.i).is_some_and(|c| c.is_ascii_alphanumeric() || *c == b'_') {
            self.i += 1;
        }
        String::from_utf8_lossy(&self.s[st..self.i]).into()
    }
    /// Body of a literal whose opening delimiter `q` was just consumed.
    fn quoted(&mut self, q: u8) -> Result<String> {
        let mut out = vec![];
        loop {
            match self.s.get(self.i) {
                None => return Err("unterminated literal".into()),
                Some(&c) if c == q => {
                    self.i += 1;
                    return String::from_utf8(out).map_err(err);
                }
                Some(b'\\') => {
                    let n = *self.s.get(self.i + 1).ok_or("unterminated literal")?;
                    if q == b'/' {
                        // regex: only `\/` is ours, every other escape belongs to the regex
                        if n != b'/' {
                            out.push(b'\\');
                        }
                        out.push(n);
                    } else {
                        out.push(match n {
                            b'n' => b'\n',
                            b't' => b'\t',
                            b'r' => b'\r',
                            c => c,
                        });
                    }
                    self.i += 2;
                }
                Some(&c) => {
                    out.push(c);
                    self.i += 1;
                }
            }
        }
    }
    fn string(&mut self) -> Result<Option<String>> {
        match self.peek() {
            Some(q @ (b'"' | b'\'')) => {
                self.i += 1;
                self.quoted(q).map(Some)
            }
            _ => Ok(None),
        }
    }
    fn expr(&mut self) -> Result<Expr> {
        let e = if let Some(s) = self.string()? {
            Expr::Const(s)
        } else {
            match self.ident().as_str() {
                "null" => Expr::Const(String::new()),
                "value" => {
                    let mut calls = vec![];
                    while self.eat(b'.') {
                        calls.push(self.call()?);
                    }
                    Expr::Value(calls)
                }
                _ => return Err(format!("unsupported expression `{}`", String::from_utf8_lossy(self.s))),
            }
        };
        if self.peek().is_some() {
            return Err(format!("unsupported syntax at `{}`", self.rest()));
        }
        Ok(e)
    }
    fn call(&mut self) -> Result<(String, Vec<Arg>)> {
        let name = self.ident();
        if !self.eat(b'(') {
            return Err(format!("expected `(` after `{name}`"));
        }
        let mut args = vec![];
        if !self.eat(b')') {
            loop {
                args.push(if let Some(s) = self.string()? {
                    Arg::Str(s)
                } else if self.eat(b'/') {
                    Arg::Re(Regex::new(&self.quoted(b'/')?).map_err(err)?)
                } else {
                    return Err(format!("unsupported argument at `{}`", self.rest()));
                });
                if self.eat(b')') {
                    break;
                }
                if !self.eat(b',') {
                    return Err(format!("expected `,` or `)` at `{}`", self.rest()));
                }
            }
        }
        let ok = matches!(
            (name.as_str(), args.as_slice()),
            ("trim" | "strip" | "toLowercase" | "toUppercase" | "toTitlecase" | "toString" | "fingerprint", [])
                | ("replace", [_, Arg::Str(_)])
        );
        if !ok {
            return Err(format!("unsupported function `{name}` with {} argument(s)", args.len()));
        }
        Ok((name, args))
    }
}
