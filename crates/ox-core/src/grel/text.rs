//! Text helpers matching the Java libraries OpenRefine calls (Apache Commons Lang/Text/Codec, URLEncoder).
use regex::Regex;
use std::sync::OnceLock;

/// Java Character.getType categories with the names OpenRefine's unicodeType() prints.
const CATEGORIES: &[(&str, &str)] = &[
    ("Lu", "uppercase letter"), ("Ll", "lowercase letter"), ("Lt", "titlecase letter"), ("Lm", "modifier letter"),
    ("Lo", "other letter"), ("Mn", "non spacing mark"), ("Me", "enclosing mark"), ("Mc", "combining spacing mark"),
    ("Nd", "decimal digit number"), ("Nl", "letter number"), ("No", "other number"), ("Zs", "space separator"),
    ("Zl", "line separator"), ("Zp", "paragraph separator"), ("Cc", "control"), ("Cf", "format"), ("Co", "private use"),
    ("Pd", "dash punctuation"), ("Ps", "start punctuation"), ("Pe", "end punctuation"),
    ("Pc", "connector punctuation"), ("Po", "other punctuation"), ("Sm", "math symbol"), ("Sc", "currency symbol"),
    ("Sk", "modifier symbol"), ("So", "other symbol"), ("Pi", "initial quote punctuation"), ("Pf", "final quote punctuation"),
];

pub fn category(c: char) -> &'static str {
    static RES: OnceLock<Vec<(Regex, &'static str)>> = OnceLock::new();
    let res = RES.get_or_init(|| CATEGORIES.iter().map(|(k, n)| (Regex::new(&format!(r"^\p{{{k}}}$")).unwrap(), *n)).collect());
    let mut buf = [0u8; 4];
    let s = c.encode_utf8(&mut buf);
    res.iter().find(|(r, _)| r.is_match(s)).map_or("unassigned", |(_, n)| n)
}

/// Java String.length(): UTF-16 code units.
pub fn utf16_len(s: &str) -> usize {
    s.encode_utf16().count()
}

/// Java substring by UTF-16 indices (callers clamp).
pub fn utf16_slice(s: &str, from: usize, to: usize) -> String {
    let u: Vec<u16> = s.encode_utf16().collect();
    String::from_utf16_lossy(&u[from.min(u.len())..to.min(u.len()).max(from.min(u.len()))])
}

/// Java String.trim()-like strip of Unicode whitespace, keeping non-breaking spaces (Character.isWhitespace).
pub fn java_strip(s: &str) -> &str {
    s.trim_matches(|c: char| c.is_whitespace() && !matches!(c, '\u{a0}' | '\u{2007}' | '\u{202f}'))
}

/// WordUtils.capitalizeFully: lowercase everything, then uppercase the first letter after a delimiter.
pub fn title_case(s: &str, delims: Option<&str>) -> String {
    let is_delim = |c: char| delims.map_or(c.is_whitespace(), |d| d.contains(c));
    let mut up = true;
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if is_delim(c) {
            out.push(c);
            up = true;
        } else if up {
            out.extend(c.to_uppercase());
            up = false;
        } else {
            out.extend(c.to_lowercase());
        }
    }
    out
}

const LATIN1_ENTITIES: [&str; 96] = [
    "nbsp", "iexcl", "cent", "pound", "curren", "yen", "brvbar", "sect", "uml", "copy", "ordf", "laquo", "not", "shy", "reg", "macr",
    "deg", "plusmn", "sup2", "sup3", "acute", "micro", "para", "middot", "cedil", "sup1", "ordm", "raquo", "frac14", "frac12", "frac34", "iquest",
    "Agrave", "Aacute", "Acirc", "Atilde", "Auml", "Aring", "AElig", "Ccedil", "Egrave", "Eacute", "Ecirc", "Euml", "Igrave", "Iacute", "Icirc", "Iuml",
    "ETH", "Ntilde", "Ograve", "Oacute", "Ocirc", "Otilde", "Ouml", "times", "Oslash", "Ugrave", "Uacute", "Ucirc", "Uuml", "Yacute", "THORN", "szlig",
    "agrave", "aacute", "acirc", "atilde", "auml", "aring", "aelig", "ccedil", "egrave", "eacute", "ecirc", "euml", "igrave", "iacute", "icirc", "iuml",
    "eth", "ntilde", "ograve", "oacute", "ocirc", "otilde", "ouml", "divide", "oslash", "ugrave", "uacute", "ucirc", "uuml", "yacute", "thorn", "yuml",
];
// ponytail: Latin-1 + the XML five; HTML4 also names ~150 Greek/math/symbol entities, add the table when a user hits one.

pub fn escape_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("&quot;"),
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '\u{a0}'..='\u{ff}' => {
                out.push('&');
                out.push_str(LATIN1_ENTITIES[c as usize - 0xa0]);
                out.push(';');
            }
            _ => out.push(c),
        }
    }
    out
}

pub fn escape_xml(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("&quot;"),
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(c),
        }
    }
    out
}

pub fn unescape_html(s: &str) -> String {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"&(#[0-9]+|#[xX][0-9a-fA-F]+|[A-Za-z][A-Za-z0-9]*);").unwrap());
    re.replace_all(s, |c: &regex::Captures| {
        let e = &c[1];
        let ch = if let Some(h) = e.strip_prefix("#x").or_else(|| e.strip_prefix("#X")) {
            u32::from_str_radix(h, 16).ok().and_then(char::from_u32)
        } else if let Some(d) = e.strip_prefix('#') {
            d.parse().ok().and_then(char::from_u32)
        } else {
            match e {
                "quot" => Some('"'),
                "amp" => Some('&'),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "apos" => Some('\''),
                _ => LATIN1_ENTITIES.iter().position(|n| *n == e).and_then(|i| char::from_u32(0xa0 + i as u32)),
            }
        };
        ch.map_or_else(|| c[0].to_string(), |ch| ch.to_string())
    })
    .into_owned()
}

/// StringEscapeUtils.escapeEcmaScript.
pub fn escape_js(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for u in s.encode_utf16() {
        match u {
            0x27 => out.push_str("\\'"),
            0x22 => out.push_str("\\\""),
            0x5c => out.push_str("\\\\"),
            0x2f => out.push_str("\\/"),
            0x08 => out.push_str("\\b"),
            0x0a => out.push_str("\\n"),
            0x09 => out.push_str("\\t"),
            0x0c => out.push_str("\\f"),
            0x0d => out.push_str("\\r"),
            0x20..=0x7e => out.push(u as u8 as char),
            _ => out.push_str(&format!("\\u{u:04X}")),
        }
    }
    out
}

pub fn unescape_js(s: &str) -> String {
    let mut out = String::new();
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match it.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('b') => out.push('\u{8}'),
            Some('f') => out.push('\u{c}'),
            Some('u') => {
                let h: String = (0..4).filter_map(|_| it.next()).collect();
                if let Some(ch) = u32::from_str_radix(&h, 16).ok().and_then(char::from_u32) {
                    out.push(ch);
                }
            }
            Some(o) => out.push(o),
            None => out.push('\\'),
        }
    }
    out
}

/// StringEscapeUtils.escapeCsv.
pub fn escape_csv(s: &str) -> String {
    if s.contains([',', '"', '\r', '\n']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

pub fn unescape_csv(s: &str) -> String {
    match s.strip_prefix('"').and_then(|x| x.strip_suffix('"')) {
        Some(inner) if s.len() >= 2 => inner.replace("\"\"", "\""),
        _ => s.to_string(),
    }
}

/// java.net.URLEncoder.encode(s, UTF-8).
pub fn url_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'.' | b'-' | b'*' | b'_' => out.push(b as char),
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// java.net.URLDecoder.decode(s, UTF-8); None on a malformed escape.
pub fn url_decode(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'+' => out.push(b' '),
            b'%' => {
                let h = std::str::from_utf8(b.get(i + 1..i + 3)?).ok()?;
                out.push(u8::from_str_radix(h, 16).ok()?);
                i += 2;
            }
            c => out.push(c),
        }
        i += 1;
    }
    Some(String::from_utf8_lossy(&out).into_owned())
}

/// Commons Codec Soundex; Err for a letter outside A-Z (Codec throws IllegalArgumentException).
pub fn soundex(s: &str) -> Result<String, String> {
    const MAP: &[u8; 26] = b"0123012#02245501262301#202";
    let letters: Vec<char> = s.chars().filter(|c| c.is_alphabetic()).flat_map(|c| c.to_uppercase()).collect();
    let Some(&first) = letters.first() else { return Ok(String::new()) };
    let code = |c: char| if c.is_ascii_uppercase() { Ok(MAP[(c as u8 - b'A') as usize]) } else { Err(format!("The character is not mapped: {c}")) };
    let mut out = String::from(first);
    let mut last = code(first)?;
    for &c in &letters[1..] {
        let m = code(c)?;
        if m == b'#' {
            continue;
        }
        if m != b'0' && m != last {
            out.push(m as char);
            if out.len() == 4 {
                break;
            }
        }
        last = m;
    }
    while out.len() < 4 {
        out.push('0');
    }
    Ok(out)
}

/// Apache StringUtils.replaceEach: one left-to-right pass, earliest match wins, first search string on ties.
pub fn replace_each(s: &str, find: &[String], repl: &[String]) -> String {
    let mut out = String::new();
    let mut pos = 0;
    loop {
        let hit = find.iter().enumerate().filter(|(_, f)| !f.is_empty()).filter_map(|(i, f)| s[pos..].find(f.as_str()).map(|at| (pos + at, i))).min();
        match hit {
            Some((at, i)) => {
                out.push_str(&s[pos..at]);
                out.push_str(repl.get(i).map_or("", |r| r.as_str()));
                pos = at + find[i].len();
            }
            None => {
                out.push_str(&s[pos..]);
                return out;
            }
        }
    }
}

/// opencsv CSVParser line split, as smartSplit uses it.
pub fn csv_split(s: &str, sep: char) -> Vec<Option<String>> {
    let mut out = vec![];
    let mut cur = String::new();
    let mut quoted = false;
    let mut it = s.chars().peekable();
    let mut field_start = true;
    let mut was_quoted = false;
    let finish = |cur: &mut String, q: &mut bool| -> Option<String> {
        let f = std::mem::take(cur);
        let quoted = std::mem::replace(q, false);
        if f.is_empty() && !quoted { None } else { Some(f) }
    };
    while let Some(c) = it.next() {
        // a quote is special only at the start of a field or inside a quoted field
        if c == '"' && (quoted || field_start) {
            if quoted && it.peek() == Some(&'"') {
                cur.push('"');
                it.next();
            } else {
                quoted = !quoted;
                was_quoted = true;
            }
            field_start = false;
        } else if c == sep && !quoted {
            field_start = true;
            out.push(finish(&mut cur, &mut was_quoted));
        } else {
            cur.push(c);
            field_start = false;
        }
    }
    out.push(finish(&mut cur, &mut was_quoted));
    out
}

pub fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut cur = vec![i + 1; b.len() + 1];
        for (j, cb) in b.iter().enumerate() {
            cur[j + 1] = (prev[j] + (ca != cb) as usize).min(prev[j + 1] + 1).min(cur[j] + 1);
        }
        prev = cur;
    }
    prev[b.len()]
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn codecs() {
        assert_eq!(soundex("Tymczak").unwrap(), "T522");
        assert_eq!(soundex("Pfister").unwrap(), "P236");
        assert_eq!(url_encode("a b&c=d/é"), "a+b%26c%3Dd%2F%C3%A9");
        assert_eq!(escape_js("\"x'\n"), "\\\"x\\'\\n");
        assert_eq!(replace_each("abc", &["a".into(), "b".into()], &["1".into(), "2".into()]), "12c");
        assert_eq!(levenshtein("kitten", "sitting"), 3);
        assert_eq!(category(' '), "space separator");
    }
}
