//! GREL scanner and parser, following OpenRefine's Parser.java precedence and quirks:
//! comparisons < (+ -) < (* / %) < factors; no && || or unary !; a top-level expression
//! ignores trailing tokens ("1e3" is 1); integer literals wrap like Java longs.
use super::funcs::{is_control, is_function, CONTROL_ARITY};
use super::value::Value;
use regex::{Regex, RegexBuilder};

#[derive(Clone, Debug)]
pub enum Expr {
    Lit(Value),
    Regex(Regex),
    Var(String),
    Call(String, Vec<Expr>),
    Field(Box<Expr>, String),
    Index(Box<Expr>, Vec<Expr>),
    Op(&'static str, Box<Expr>, Box<Expr>),
    Array(Vec<Expr>),
}

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    Num(Value),
    Str(String),
    Regex(String, String),
    Ident(String),
    Op(&'static str),
    Delim(char),
}

struct Scanner<'a> {
    s: &'a [u8],
    src: &'a str,
    i: usize,
}

impl Scanner<'_> {
    fn skip_ws(&mut self) {
        while self.s.get(self.i).is_some_and(|c| c.is_ascii_whitespace()) {
            self.i += 1;
        }
    }
    /// Next token; `regex_ok` says whether a '/' here starts a regex literal.
    fn next(&mut self, regex_ok: bool) -> Result<Option<(usize, Tok)>, String> {
        self.skip_ws();
        let start = self.i;
        let Some(&c) = self.s.get(self.i) else { return Ok(None) };
        let tok = if c.is_ascii_digit() {
            let mut n: i64 = 0;
            while let Some(d) = self.s.get(self.i).filter(|d| d.is_ascii_digit()) {
                n = n.wrapping_mul(10).wrapping_add((d - b'0') as i64);
                self.i += 1;
            }
            if self.s.get(self.i) == Some(&b'.') && self.s.get(self.i + 1).is_some_and(|d| d.is_ascii_digit()) {
                self.i += 1;
                while self.s.get(self.i).is_some_and(|d| d.is_ascii_digit()) {
                    self.i += 1;
                }
                Tok::Num(Value::Num(self.src[start..self.i].parse().unwrap()))
            } else {
                Tok::Num(Value::Int(n))
            }
        } else if c == b'"' || c == b'\'' {
            self.i += 1;
            let mut out = String::new();
            loop {
                let rest = &self.src[self.i..];
                let Some(ch) = rest.chars().next() else { return Err(format!("Parsing error at offset {start}: Missing number, string, identifier, regex, or parenthesized expression")) };
                self.i += ch.len_utf8();
                if ch as u32 == c as u32 {
                    break;
                }
                if ch == '\\' {
                    let Some(e) = self.src[self.i..].chars().next() else { continue };
                    self.i += e.len_utf8();
                    out.push(match e {
                        'n' => '\n',
                        't' => '\t',
                        'r' => '\r',
                        'b' => '\u{8}',
                        'f' => '\u{c}',
                        other => other,
                    });
                } else {
                    out.push(ch);
                }
            }
            Tok::Str(out)
        } else if c == b'/' && regex_ok {
            self.i += 1;
            let mut pat = String::new();
            loop {
                let Some(ch) = self.src[self.i..].chars().next() else { return Err(format!("Parsing error at offset {start}: Unterminated regex")) };
                self.i += ch.len_utf8();
                if ch == '/' {
                    break;
                }
                if ch == '\\' && self.s.get(self.i) == Some(&b'/') {
                    pat.push('/');
                    self.i += 1;
                    continue;
                }
                pat.push(ch);
            }
            let fs = self.i;
            while self.s.get(self.i).is_some_and(|c| c.is_ascii_alphabetic()) {
                self.i += 1;
            }
            Tok::Regex(pat, self.src[fs..self.i].to_string())
        } else if c.is_ascii_alphabetic() || c == b'_' {
            while self.s.get(self.i).is_some_and(|c| c.is_ascii_alphanumeric() || *c == b'_') {
                self.i += 1;
            }
            Tok::Ident(self.src[start..self.i].to_string())
        } else {
            let two = self.s.get(self.i..self.i + 2);
            let op = match two {
                Some(b"==") => Some("=="),
                Some(b"!=") => Some("!="),
                Some(b"<=") => Some("<="),
                Some(b">=") => Some(">="),
                _ => None,
            };
            if let Some(op) = op {
                self.i += 2;
                Tok::Op(op)
            } else {
                self.i += 1;
                match c {
                    b'+' => Tok::Op("+"),
                    b'-' => Tok::Op("-"),
                    b'*' => Tok::Op("*"),
                    b'/' => Tok::Op("/"),
                    b'%' => Tok::Op("%"),
                    b'<' => Tok::Op("<"),
                    b'>' => Tok::Op(">"),
                    b'(' | b')' | b'[' | b']' | b',' | b'.' => Tok::Delim(c as char),
                    _ => {
                        // any other character is an operator OpenRefine doesn't know (& | ! = { ...)
                        let ch = self.src[start..].chars().next().unwrap();
                        self.i = start + ch.len_utf8();
                        Tok::Ident(format!("\u{0}{ch}"))
                    }
                }
            }
        };
        Ok(Some((start, tok)))
    }
}

struct Parser<'a> {
    sc: Scanner<'a>,
    tok: Option<(usize, Tok)>,
}

const MISSING: &str = "Missing number, string, identifier, regex, or parenthesized expression";

impl Parser<'_> {
    fn advance(&mut self, regex_ok: bool) -> Result<(), String> {
        self.tok = self.sc.next(regex_ok)?;
        Ok(())
    }
    fn at(&self) -> usize {
        self.tok.as_ref().map_or(self.sc.src.len(), |t| t.0)
    }
    fn err(&self, msg: impl std::fmt::Display) -> String {
        format!("Parsing error at offset {}: {msg}", self.at())
    }
    fn is_delim(&self, c: char) -> bool {
        matches!(self.tok, Some((_, Tok::Delim(d))) if d == c)
    }
    fn op(&self, ops: &[&str]) -> Option<&'static str> {
        match self.tok {
            Some((_, Tok::Op(o))) if ops.contains(&o) => Some(o),
            _ => None,
        }
    }

    fn expression(&mut self) -> Result<Expr, String> {
        let mut e = self.sub()?;
        while let Some(op) = self.op(&["==", "!=", "<", ">", "<=", ">="]) {
            self.advance(true)?;
            e = Expr::Op(op, Box::new(e), Box::new(self.sub()?));
        }
        Ok(e)
    }
    fn sub(&mut self) -> Result<Expr, String> {
        let mut e = self.term()?;
        while let Some(op) = self.op(&["+", "-"]) {
            self.advance(true)?;
            e = Expr::Op(op, Box::new(e), Box::new(self.term()?));
        }
        Ok(e)
    }
    fn term(&mut self) -> Result<Expr, String> {
        let mut e = self.factor()?;
        while let Some(op) = self.op(&["*", "/", "%"]) {
            self.advance(true)?;
            e = Expr::Op(op, Box::new(e), Box::new(self.factor()?));
        }
        Ok(e)
    }

    fn args(&mut self, close: char) -> Result<Vec<Expr>, String> {
        let mut args = vec![];
        if self.is_delim(close) {
            self.advance(false)?;
            return Ok(args);
        }
        loop {
            args.push(self.expression()?);
            if self.is_delim(',') {
                self.advance(true)?;
            } else if self.is_delim(close) {
                self.advance(false)?;
                return Ok(args);
            } else {
                return Err(self.err(format!("Missing {close}")));
            }
        }
    }

    fn call(&mut self, name: String, args: Vec<Expr>, at: usize) -> Result<Expr, String> {
        if is_control(&name) {
            if let Some((_, n)) = CONTROL_ARITY.iter().find(|(c, _)| *c == name) {
                if !n.contains(&args.len()) {
                    let want = match (n.start(), n.end()) {
                        (1, 1) => "one argument".to_string(),
                        (2, 2) => "two arguments".into(),
                        (3, 3) => "three arguments".into(),
                        (4, 4) => "four arguments".into(),
                        (5, 5) => "five arguments".into(),
                        (a, b) => format!("{a} to {b} arguments"),
                    };
                    return Err(format!("Parsing error at offset {}: {name} expects {want}", self.at()));
                }
            }
            Ok(Expr::Call(name, args))
        } else if is_function(&name) {
            Ok(Expr::Call(name, args))
        } else {
            Err(format!("Parsing error at offset {at}: Unknown function or control named {}", name.trim_start_matches('\u{0}')))
        }
    }

    fn factor(&mut self) -> Result<Expr, String> {
        let Some((at, tok)) = self.tok.clone() else { return Err(self.err("Expecting something more at end of expression")) };
        let mut e = match tok {
            Tok::Str(s) => {
                self.advance(false)?;
                Expr::Lit(Value::Str(s))
            }
            Tok::Regex(p, flags) => {
                self.advance(false)?;
                let re = RegexBuilder::new(&java_regex(&p)).case_insensitive(flags.contains('i')).build().map_err(|e| format!("Parsing error at offset {at}: {e}"))?;
                Expr::Regex(re)
            }
            Tok::Num(n) => {
                self.advance(false)?;
                Expr::Lit(n)
            }
            Tok::Op("-") => {
                self.advance(false)?;
                match self.tok.clone() {
                    Some((_, Tok::Num(n))) => {
                        self.advance(false)?;
                        Expr::Lit(match n {
                            Value::Int(i) => Value::Int(i.wrapping_neg()),
                            Value::Num(f) => Value::Num(-f),
                            other => other,
                        })
                    }
                    _ => return Err(format!("Parsing error at offset {at}: Bad negative number")),
                }
            }
            Tok::Ident(name) => {
                self.advance(false)?;
                if self.is_delim('(') {
                    self.advance(true)?;
                    let args = self.args(')')?;
                    self.call(name, args, at)?
                } else if name.starts_with('\u{0}') {
                    return Err(format!("Parsing error at offset {at}: {MISSING}"));
                } else {
                    Expr::Var(name)
                }
            }
            Tok::Delim('(') => {
                self.advance(true)?;
                let inner = self.expression()?;
                if !self.is_delim(')') {
                    return Err(self.err("Missing )"));
                }
                self.advance(false)?;
                inner
            }
            Tok::Delim('[') => {
                self.advance(true)?;
                Expr::Array(self.args(']')?)
            }
            _ => return Err(format!("Parsing error at offset {at}: {MISSING}")),
        };
        // postfix: .name, .name(args), [i], [a, b]
        loop {
            if self.is_delim('.') {
                self.advance(false)?;
                let Some((at, Tok::Ident(name))) = self.tok.clone() else { return Err(self.err("Missing function name")) };
                if name.starts_with('\u{0}') {
                    return Err(format!("Parsing error at offset {at}: Missing function name"));
                }
                self.advance(false)?;
                if self.is_delim('(') {
                    self.advance(true)?;
                    let mut args = vec![e];
                    args.extend(self.args(')')?);
                    if !is_function(&name) && !is_control(&name) {
                        return Err(format!("Parsing error at offset {}: Unknown function {name}", self.at()));
                    }
                    e = Expr::Call(name, args);
                } else {
                    e = Expr::Field(Box::new(e), name);
                }
            } else if self.is_delim('[') {
                self.advance(true)?;
                let idx = self.args(']')?;
                e = Expr::Index(Box::new(e), idx);
            } else {
                return Ok(e);
            }
        }
    }
}

/// Parse a GREL expression (an optional `grel:` prefix is allowed).
pub fn parse(src: &str) -> Result<Expr, String> {
    let src = src.strip_prefix("grel:").unwrap_or(src);
    let mut p = Parser { sc: Scanner { s: src.as_bytes(), src, i: 0 }, tok: None };
    p.advance(true)?;
    // trailing tokens after a complete expression are ignored, as in OpenRefine ("1e3" is 1),
    // except characters GREL has no operator for (&&, ||, !)
    let e = p.expression()?;
    if let Some((at, Tok::Ident(name))) = &p.tok {
        if let Some(op) = name.strip_prefix('\u{0}') {
            return Err(format!("Parsing error at offset {at}: Unknown function or control named{op}"));
        }
    }
    Ok(e)
}

/// Java regex classes are ASCII-only unless UNICODE_CHARACTER_CLASS is set; Rust's are Unicode.
pub fn java_regex(p: &str) -> String {
    let (mut out, mut in_class, mut it) = (String::new(), false, p.chars());
    while let Some(c) = it.next() {
        match c {
            '\\' => match it.next() {
                Some(k @ ('w' | 'W' | 'd' | 'D' | 's' | 'S')) => {
                    let set = match k.to_ascii_lowercase() {
                        'w' => "0-9A-Za-z_",
                        'd' => "0-9",
                        _ => " \\t\\n\\x0B\\f\\r",
                    };
                    let neg = k.is_ascii_uppercase();
                    out.push_str(&match (in_class, neg) {
                        (true, false) => set.to_string(),
                        (_, true) => format!("[^{set}]"),
                        (false, false) => format!("[{set}]"),
                    });
                }
                Some(k @ ('b' | 'B')) if !in_class => out.push_str(&format!("(?-u:\\{k})")),
                Some(k) => {
                    out.push('\\');
                    out.push(k);
                }
                None => out.push('\\'),
            },
            '[' if !in_class => {
                in_class = true;
                out.push(c);
            }
            ']' if in_class => {
                in_class = false;
                out.push(c);
            }
            _ => out.push(c),
        }
    }
    out
}
