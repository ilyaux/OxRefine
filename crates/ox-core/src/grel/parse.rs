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
    depth: usize,
}

/// How deep an expression's tree may be, in units: each nesting level (parentheses, a call, an array, a chained
/// call or index) costs LEVEL, each binary operator 1, as a chain like `a + b + c` nests one deeper per operator.
/// Parsing, evaluating and dropping an expression all recurse once per level, so the limit keeps any input from
/// overflowing the stack. On a 1 MB stack (the Windows main thread) a release build handles about 512 levels or
/// 2048 operators, a debug build about 100 levels or 512 operators; 128 levels or 512 operators leave room.
const MAX_DEPTH: usize = 512;
const LEVEL: usize = 4;

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
        let start = self.depth;
        let mut e = self.sub()?;
        while let Some(op) = self.op(&["==", "!=", "<", ">", "<=", ">="]) {
            self.advance(true)?;
            self.depth += 1;
            self.deeper()?;
            e = Expr::Op(op, Box::new(e), Box::new(self.sub()?));
        }
        self.depth = start;
        Ok(e)
    }
    fn sub(&mut self) -> Result<Expr, String> {
        let start = self.depth;
        let mut e = self.term()?;
        while let Some(op) = self.op(&["+", "-"]) {
            self.advance(true)?;
            self.depth += 1;
            self.deeper()?;
            e = Expr::Op(op, Box::new(e), Box::new(self.term()?));
        }
        self.depth = start;
        Ok(e)
    }
    fn term(&mut self) -> Result<Expr, String> {
        let start = self.depth;
        let mut e = self.factor()?;
        while let Some(op) = self.op(&["*", "/", "%"]) {
            self.advance(true)?;
            self.depth += 1;
            self.deeper()?;
            e = Expr::Op(op, Box::new(e), Box::new(self.factor()?));
        }
        self.depth = start;
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
        self.depth += LEVEL;
        let e = self.nested();
        self.depth -= LEVEL;
        e
    }
    fn deeper(&mut self) -> Result<(), String> {
        if self.depth > MAX_DEPTH {
            return Err(self.err("Expression is nested too deeply"));
        }
        Ok(())
    }
    fn nested(&mut self) -> Result<Expr, String> {
        self.deeper()?;
        let start = self.depth;
        let Some((at, tok)) = self.tok.clone() else { return Err(self.err("Expecting something more at end of expression")) };
        let mut e = match tok {
            Tok::Str(s) => {
                self.advance(false)?;
                Expr::Lit(Value::Str(s))
            }
            Tok::Regex(p, flags) => {
                self.advance(false)?;
                let re = user_regex(&p, flags.contains('i')).map_err(|e| format!("Parsing error at offset {at}: {e}"))?;
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
        // postfix: .name, .name(args), [i], [a, b]; each one nests the expression one level deeper
        loop {
            if self.is_delim('.') || self.is_delim('[') {
                self.depth += LEVEL;
                self.deeper()?;
            }
            if self.is_delim('.') {
                self.advance(false)?;
                let Some((at, Tok::Ident(name))) = self.tok.clone() else { return Err(self.err("Missing function name")) };
                if name.starts_with('\u{0}') {
                    return Err(format!("Parsing error at offset {at}: Missing function name"));
                }
                self.advance(false)?;
                if self.is_delim('(') {
                    self.advance(true)?;
                    // only functions can be called this way: value.if(...) is unknown, as in OpenRefine
                    if !is_function(&name) {
                        return Err(format!("Parsing error at offset {}: Unknown function {name}", self.at()));
                    }
                    let mut args = vec![e];
                    args.extend(self.args(')')?);
                    e = Expr::Call(name, args);
                } else {
                    e = Expr::Field(Box::new(e), name);
                }
            } else if self.is_delim('[') {
                self.advance(true)?;
                let idx = self.args(']')?;
                e = Expr::Index(Box::new(e), idx);
            } else {
                // errors end the whole parse, so only this path needs the depth restored
                self.depth = start;
                return Ok(e);
            }
        }
    }
}

/// Parse a GREL expression (an optional `grel:` prefix is allowed).
pub fn parse(src: &str) -> Result<Expr, String> {
    let src = src.strip_prefix("grel:").unwrap_or(src);
    let mut p = Parser { sc: Scanner { s: src.as_bytes(), src, i: 0 }, tok: None, depth: 0 };
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
/// A regex typed by a user (a GREL /literal/, a text filter, a split separator), in Java syntax. Nested
/// repetitions like `(a*)*` make the regex compiler recurse once per level, so nesting is capped well below
/// what would overflow the stack; no real pattern nests anywhere near 64 deep.
pub fn user_regex(java: &str, case_insensitive: bool) -> Result<regex::Regex, regex::Error> {
    RegexBuilder::new(&java_regex(java)).case_insensitive(case_insensitive).nest_limit(64).build()
}

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

#[cfg(test)]
mod tests {
    use super::{LEVEL, MAX_DEPTH};

    #[test]
    fn nesting_is_limited() {
        // the deepest allowed expression must parse, run and drop, and anything deeper must be an error rather
        // than a stack overflow; debug frames are about 5 times larger, so 4 MB here stands for 1 MB in release
        std::thread::Builder::new()
            .stack_size(4 << 20)
            .spawn(|| {
                let nest = |n: usize, open: &str, close: &str| format!("{}value{}", open.repeat(n), close.repeat(n));
                let run = |src: &str| crate::grel::compile(src).map(|p| p.eval(crate::grel::Value::Str("x".into()), &crate::grel::NoRow).render());
                for (open, close) in [("(", ")"), ("[", "]"), ("trim(", ")"), ("if(true,", ",1)")] {
                    assert!(run(&nest(MAX_DEPTH / LEVEL - 2, open, close)).is_ok(), "{open}");
                    let err = run(&nest(MAX_DEPTH / LEVEL + 1, open, close)).unwrap_err();
                    assert!(err.contains("nested too deeply"), "{open}: {err}");
                    assert!(run(&nest(100_000, open, close)).is_err());
                }
                assert!(run(&format!("value{}", ".trim()".repeat(MAX_DEPTH / LEVEL - 2))).is_ok());
                for op in [" + 1", " * 1", " == 1", " / //"] {
                    assert!(run(&format!("value{}", op.repeat(MAX_DEPTH - 8))).is_ok(), "{op}");
                    assert!(run(&format!("value{}", op.repeat(MAX_DEPTH + 1))).unwrap_err().contains("nested too deeply"), "{op}");
                    assert!(run(&format!("value{}", op.repeat(100_000))).is_err());
                }
                // regexes: nested groups and stacked repetitions, which the regex compiler recurses on
                assert!(run(&format!("value.match(/{}a{}/)", "(".repeat(60), ")".repeat(60))).is_ok());
                for re in [format!("{}a{}", "(".repeat(5000), ")".repeat(5000)), format!(";{}", "*".repeat(5000)), ";*".repeat(5000), "a{2}".repeat(5000)] {
                    let _ = run(&format!("value.match(/{re}/)"));
                    let _ = super::user_regex(&re, true);
                }
                // nesting and chains together: each level holding a long chain
                assert!(run(&format!("{}value{}", "(1 + 1 + 1 + ".repeat(200), ")".repeat(200))).is_err());
                assert!(run(&format!("value{}", ".trim()".repeat(100_000))).is_err());
                assert!(run(&format!("value{}", "[0]".repeat(100_000))).is_err());
            })
            .unwrap()
            .join()
            .unwrap();
    }
}
