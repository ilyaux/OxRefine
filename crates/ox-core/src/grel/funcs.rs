//! GREL function library (OpenRefine 3.10 ControlFunctionRegistry), checked against tests/grel/fixtures.json.
use super::{
    dates, text,
    value::{from_json, json_pretty, Markup, Value, Value::*},
};
use base64::Engine as _;
use regex::Regex;
use std::ops::RangeInclusive;

pub const CONTROLS: &[&str] = &[
    "if", "with", "forEach", "forEachIndex", "forRange", "filter", "forNonBlank", "isNull", "isNotNull", "isBlank", "isNonBlank", "isEmptyString", "isError",
];

pub const CONTROL_ARITY: &[(&str, RangeInclusive<usize>)] = &[
    ("if", 3..=3), ("with", 3..=3), ("forEach", 3..=3), ("forEachIndex", 4..=4), ("forRange", 5..=5), ("filter", 3..=3), ("forNonBlank", 4..=4),
    ("isNull", 1..=1), ("isNotNull", 1..=1), ("isBlank", 1..=1), ("isNonBlank", 1..=1), ("isEmptyString", 1..=1), ("isError", 1..=1),
];

pub const FUNCTIONS: &[&str] = &[
    // types and conversion
    "type", "toString", "toNumber", "toDate", "timeSinceUnixEpochToDate", "jsonize", "parseJson", "hasField", "get", "coalesce", "isNumeric",
    // strings
    "toUppercase", "toLowercase", "toTitlecase", "trim", "strip", "length", "chomp", "contains", "startsWith", "endsWith", "indexOf", "lastIndexOf",
    "replace", "replaceChars", "replaceEach", "split", "smartSplit", "splitByCharType", "splitByLengths", "partition", "rpartition", "substring",
    "slice", "find", "match", "ngram", "fingerprint", "ngramFingerprint", "phonetic", "unicode", "unicodeType", "escape", "unescape", "encode",
    "decode", "md5", "sha1", "reinterpret", "detectLanguage", "levenshteinDistance", "diff", "range", "parseUri",
    // html / xml
    "parseHtml", "parseXml", "select", "htmlText", "xmlText", "wholeText", "ownText", "innerHtml", "innerXml", "htmlAttr", "xmlAttr", "parent", "scriptText",
    // arrays
    "join", "reverse", "sort", "uniques", "inArray",
    // dates
    "now", "inc", "datePart",
    // math
    "round", "floor", "ceil", "abs", "mod", "pow", "exp", "ln", "log", "sum", "max", "min", "fact", "factn", "combin", "gcd", "lcm", "multinomial",
    "quotient", "even", "odd", "degrees", "radians", "sin", "cos", "tan", "asin", "acos", "atan", "atan2", "sinh", "cosh", "tanh", "random", "randomNumber",
    // booleans
    "and", "or", "not", "xor",
    // need project context
    "cross", "facetCount",
];

pub fn is_control(n: &str) -> bool {
    CONTROLS.contains(&n)
}
pub fn is_function(n: &str) -> bool {
    FUNCTIONS.contains(&n)
}

macro_rules! fail {
    ($($t:tt)*) => { return Value::err(format!($($t)*)) };
}

fn list(v: &Value) -> Option<Vec<Value>> {
    match v {
        Array(a) => Some(a.clone()),
        Json(j) => j.as_array().map(|a| a.iter().cloned().map(from_json).collect()),
        _ => None,
    }
}

fn strs(v: &Value) -> Option<Vec<String>> {
    match v {
        Str(s) => Some(vec![s.clone()]),
        _ => list(v).map(|a| a.iter().map(|x| x.to_java_string()).collect()),
    }
}

fn arr(v: Vec<String>) -> Value {
    Array(v.into_iter().map(Str).collect())
}

/// Text of a non-null argument: strings as-is, other values via toString.
fn text_of(v: &Value) -> Option<String> {
    match v {
        Null | Error(_) => None,
        Str(s) => Some(s.clone()),
        other => Some(other.to_java_string()),
    }
}

pub fn parse_number(s: &str) -> Option<Value> {
    let t = s.trim();
    if t.is_empty() {
        return None;
    }
    let digits = t.strip_prefix(['-', '+']).unwrap_or(t);
    if !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()) {
        if let Ok(i) = t.parse::<i64>() {
            return Some(Int(i));
        }
    }
    let lower = t.to_ascii_lowercase();
    if (lower.contains("inf") || lower.contains("nan")) && !matches!(t, "NaN" | "Infinity" | "-Infinity" | "+Infinity") {
        return None;
    }
    t.parse::<f64>().ok().map(Num)
}

/// OpenRefine's fingerprint keyer: trim, lowercase, fold accents, drop punctuation and control
/// characters (Unicode \p{P}; symbols like < = + stay), then sorted unique whitespace tokens.
pub fn fingerprint(s: &str) -> String {
    let s = strip_punct(&crate::cluster::asciify(&text::java_strip(s).to_lowercase()));
    let mut t: Vec<&str> = s.split_whitespace().collect();
    t.sort_unstable();
    t.dedup();
    t.join(" ")
}

pub fn ngram_fingerprint(s: &str, n: usize) -> String {
    let c: Vec<char> = crate::cluster::asciify(&strip_punct(&s.to_lowercase())).chars().filter(|c| !c.is_whitespace()).collect();
    if c.len() < n {
        return String::new();
    }
    let mut g: Vec<String> = c.windows(n).map(|w| w.iter().collect()).collect();
    g.sort_unstable();
    g.dedup();
    g.concat()
}

fn strip_punct(s: &str) -> String {
    static RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\p{P}|[\x00-\x08\x0A-\x1F\x7F]").unwrap()).replace_all(s, "").into_owned()
}

fn anchored(r: &Regex) -> Regex {
    // Java Matcher.matches(): the whole input must match
    Regex::new(&format!("^(?:{})$", r.as_str())).unwrap_or_else(|_| r.clone())
}

fn slice_bounds(len: i64, from: i64, to: Option<i64>) -> (usize, usize) {
    let mut f = if from < 0 { len + from } else { from };
    f = f.clamp(0, len);
    let mut t = to.map_or(len, |t| if t < 0 { len + t } else { t });
    t = t.clamp(f, len);
    (f as usize, t as usize)
}

fn html_doc(markup: &str, document: bool) -> scraper::Html {
    if document {
        scraper::Html::parse_document(markup)
    } else {
        scraper::Html::parse_fragment(markup)
    }
}

/// Jsoup Element.text(): text content with whitespace collapsed and trimmed.
fn norm_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The single element an Html value stands for (first element of a fragment, root of a document).
fn with_element<T>(markup: &str, document: bool, f: impl FnOnce(scraper::ElementRef) -> T) -> Option<T> {
    let doc = html_doc(markup, document);
    let el = if document {
        Some(doc.root_element())
    } else {
        doc.root_element().children().find_map(scraper::ElementRef::wrap)
    };
    el.map(f)
}

fn num_args(a: &[Value], name: &str) -> Result<Vec<f64>, Value> {
    a.iter().map(|v| v.as_f64().ok_or_else(|| Value::err(format!("{name} expects numbers")))).collect()
}

fn int_result(x: f64) -> Value {
    Int(x as i64)
}

fn factorial(n: i64) -> Option<i64> {
    (1..=n).try_fold(1i64, |acc, k| acc.checked_mul(k))
}

fn gcd(a: i64, b: i64) -> i64 {
    if b == 0 { a.abs() } else { gcd(b, a % b) }
}

pub fn call(name: &str, a: &[Value]) -> Value {
    let a0 = a.first().unwrap_or(&Null);
    let a1 = a.get(1).unwrap_or(&Null);
    let a2 = a.get(2).unwrap_or(&Null);
    match name {
        // ---------------------------------------------------------------- types and conversion
        "type" => Str(a0.type_name().into()),
        "toString" => match (a0, a1) {
            (Date(d), Str(f)) => Str(dates::format(d, f)),
            _ => Str(a0.to_java_string()),
        },
        "toNumber" => match a0 {
            Null => fail!("toNumber expects one non-null argument"),
            Int(_) | Num(_) => a0.clone(),
            Str(s) => parse_number(s).unwrap_or_else(|| Value::err("Unable to parse as number")),
            _ => fail!("Unable to parse as number"),
        },
        "toDate" => match a0 {
            Date(_) => a0.clone(),
            Null => fail!("Unable to parse as date"),
            Str(s) => {
                let formats: Vec<&str> = a[1..].iter().filter_map(Value::as_str).collect();
                // formats first, then OpenRefine falls back to its default parser
                let parsed = formats.iter().find_map(|f| dates::parse_with(s, f)).or_else(|| dates::parse_default(s));
                parsed.map(Date).unwrap_or_else(|| Value::err("Unable to convert to date"))
            }
            _ => fail!("Unable to convert to date"),
        },
        "timeSinceUnixEpochToDate" => {
            let Some(n) = a0.as_i64() else { fail!("timeSinceUnixEpochToDate expects a number") };
            let unit = a1.as_str().unwrap_or("second");
            let d = match unit {
                "second" | "seconds" => chrono::DateTime::from_timestamp(n, 0),
                "millisecond" | "milliseconds" => chrono::DateTime::from_timestamp_millis(n),
                "microsecond" | "microseconds" => chrono::DateTime::from_timestamp_micros(n),
                _ => fail!("timeSinceUnixEpochToDate: unknown unit {unit}"),
            };
            d.map(|d| Date(d.naive_utc())).unwrap_or_else(|| Value::err("timeSinceUnixEpochToDate: out of range"))
        }
        "now" => Date(chrono::Utc::now().naive_utc().with_nanosecond(0).unwrap()),
        "jsonize" => Str(json_pretty(a0, 0)),
        "parseJson" => match a0 {
            Null => Null,
            // Jackson nodes stay nodes (a double prints as "1000.0"); fields and items convert on access
            Str(s) => match serde_json::from_str::<serde_json::Value>(s) {
                Ok(serde_json::Value::Null) => Null,
                Ok(j) => Json(Box::new(j)),
                Err(e) => fail!("parseJson failed: {e}"),
            },
            _ => fail!("parseJson expects a string"),
        },
        "hasField" => match (a0, a1) {
            (Json(j), Str(k)) => Bool(j.as_object().is_some_and(|m| m.contains_key(k))),
            (Obj(f), Str(k)) => Bool(f.iter().any(|(n, _)| n == k)),
            _ => Bool(false),
        },
        "get" => get(a0, a1, a.get(2)),
        "coalesce" => {
            if a.len() < 2 {
                fail!("coalesce expects at least two arguments");
            }
            a.iter().find(|v| !v.is_null()).cloned().unwrap_or(Null)
        }
        "isNumeric" => match a0 {
            Null => fail!("isNumeric: null"),
            v => {
                let s = v.to_java_string();
                Bool(!s.is_empty() && s.chars().all(|c| c.is_numeric()))
            }
        },

        // ---------------------------------------------------------------- strings
        "toUppercase" => a0.as_str().map_or_else(|| Value::err("toUppercase expects a string"), |s| Str(s.to_uppercase())),
        "toLowercase" => a0.as_str().map_or_else(|| Value::err("toLowercase expects a string"), |s| Str(s.to_lowercase())),
        "toTitlecase" => match a0 {
            // OpenRefine 3.10 also starts a new word after '.' ("example.Com")
            Str(s) => Str(text::title_case(s, Some(a1.as_str().unwrap_or(" \t\n\r\u{b}\u{c}.")))),
            _ => fail!("toTitlecase expects one or two strings"),
        },
        "trim" | "strip" => a0.as_str().map_or_else(|| Value::err(format!("{name} expects a string")), |s| Str(text::java_strip(s).into())),
        "length" => match a0 {
            Str(s) => Int(text::utf16_len(s) as i64),
            v => list(v).map_or_else(|| Value::err("length expects an array or a string"), |l| Int(l.len() as i64)),
        },
        "chomp" => match (a0, a1) {
            (Str(s), Str(sep)) => Str(s.strip_suffix(sep.as_str()).unwrap_or(s).into()),
            _ => Null,
        },
        "contains" => match (text_of(a0), a1) {
            (Some(s), Str(sub)) => Bool(s.contains(sub.as_str())),
            (Some(s), Re(r)) => Bool(r.is_match(&s)),
            _ => Null,
        },
        "startsWith" | "endsWith" => match (a0, a1) {
            (Str(s), Str(x)) => Bool(if name == "startsWith" { s.starts_with(x.as_str()) } else { s.ends_with(x.as_str()) }),
            _ => fail!("{name} expects two strings"),
        },
        "indexOf" | "lastIndexOf" => match (a0, a1) {
            (Str(s), Str(x)) => {
                let at = if name == "indexOf" { s.find(x.as_str()) } else { s.rfind(x.as_str()) };
                Int(at.map_or(-1, |b| text::utf16_len(&s[..b]) as i64))
            }
            _ => fail!("{name} expects two strings"),
        },
        "replace" => match (text_of(a0), a1, a2) {
            (Some(s), Str(f), Str(r)) => Str(s.replace(f.as_str(), r)),
            (Some(s), Re(re), Str(r)) => Str(re.replace_all(&s, r.as_str()).into_owned()),
            _ => fail!("replace expects three strings, or one string, one regex, and one string"),
        },
        "replaceChars" => match (a0, a1, a2) {
            (Str(s), Str(from), Str(to)) => {
                let (from, to): (Vec<char>, Vec<char>) = (from.chars().collect(), to.chars().collect());
                Str(s.chars().filter_map(|c| match from.iter().position(|f| *f == c) {
                    Some(i) => to.get(i).copied(),
                    None => Some(c),
                }).collect())
            }
            _ => fail!("replaceChars expects three strings"),
        },
        "replaceEach" => match (a0, strs(a1), strs(a2)) {
            (Str(s), Some(find), Some(repl)) => {
                let repl = if repl.len() == 1 && find.len() > 1 { vec![repl[0].clone(); find.len()] } else { repl };
                Str(text::replace_each(s, &find, &repl))
            }
            _ => fail!("replaceEach expects three arguments: a string, an array of strings to find and replacements"),
        },
        "split" => {
            let Some(s) = text_of(a0) else { fail!("split expects three strings, or one string, one regex, followed by an optional boolean") };
            let keep = matches!(a2, Bool(true));
            let parts: Vec<String> = match a1 {
                Str(sep) if sep.is_empty() => s.split_whitespace().map(String::from).collect(),
                Str(sep) => s.split(sep.as_str()).map(String::from).collect(),
                Re(r) => r.split(&s).map(String::from).collect(),
                _ => fail!("split expects three strings, or one string, one regex, followed by an optional boolean"),
            };
            arr(if keep { parts } else { parts.into_iter().filter(|p| !p.is_empty()).collect() })
        }
        "smartSplit" => match a0 {
            Str(s) => {
                let sep = match a1 {
                    Str(x) => x.chars().next().unwrap_or(','),
                    _ if s.contains('\t') => '\t',
                    _ => ',',
                };
                // opencsv: an empty unquoted field is null
                Array(text::csv_split(s, sep).into_iter().map(|f| f.map_or(Null, Str)).collect())
            }
            _ => fail!("smartSplit expects a string"),
        },
        "splitByCharType" => match a0 {
            Str(s) => {
                let mut out: Vec<String> = vec![];
                let mut last = "";
                for c in s.chars() {
                    let cat = text::category(c);
                    match out.last_mut() {
                        Some(cur) if cat == last => cur.push(c),
                        _ => out.push(c.to_string()),
                    }
                    last = cat;
                }
                arr(out)
            }
            _ => fail!("splitByCharType expects two strings"),
        },
        "splitByLengths" => match a0 {
            Str(s) if a.len() > 1 => {
                let len = text::utf16_len(s);
                let mut pos = 0;
                let mut out = vec![];
                for n in &a[1..] {
                    let Some(n) = n.as_i64() else { fail!("splitByLengths expects one string and at least one number") };
                    let end = (pos + n.max(0) as usize).min(len);
                    out.push(text::utf16_slice(s, pos.min(len), end));
                    pos = end;
                }
                arr(out)
            }
            _ => fail!("splitByLengths expects one string and at least one number"),
        },
        "partition" | "rpartition" => {
            let Some(s) = text_of(a0) else { return Null };
            let found = match a1 {
                Str(f) if !f.is_empty() => if name == "partition" { s.find(f.as_str()) } else { s.rfind(f.as_str()) }.map(|i| (i, i + f.len())),
                Re(r) => {
                    let ms: Vec<_> = r.find_iter(&s).collect();
                    if name == "partition" { ms.first() } else { ms.last() }.map(|m| (m.start(), m.end()))
                }
                Str(_) => None,
                _ => fail!("{name} expects a string and a string or regex"),
            };
            let omit = matches!(a2, Bool(true));
            let (b, m, e) = match found {
                Some((i, j)) => (s[..i].to_string(), s[i..j].to_string(), s[j..].to_string()),
                None => (s.clone(), String::new(), String::new()),
            };
            arr(if omit { vec![b, e] } else { vec![b, m, e] })
        }
        "substring" | "slice" => match a0 {
            Null => Null,
            Str(s) => {
                let (Some(from), to) = (a1.as_i64(), a.get(2).and_then(Value::as_i64)) else { fail!("{name} expects a string and one or two numbers") };
                let (f, t) = slice_bounds(text::utf16_len(s) as i64, from, to);
                Str(text::utf16_slice(s, f, t))
            }
            v => match (list(v), a1.as_i64()) {
                (Some(l), Some(from)) => {
                    let (f, t) = slice_bounds(l.len() as i64, from, a.get(2).and_then(Value::as_i64));
                    Array(l[f..t].to_vec())
                }
                _ => fail!("{name} expects a string or an array and one or two numbers"),
            },
        },
        "find" => match (text_of(a0), a1) {
            (None, _) => Array(vec![]),
            (Some(s), Re(r)) => arr(r.find_iter(&s).map(|m| m.as_str().to_string()).collect()),
            (Some(s), Str(f)) if !f.is_empty() => arr(s.matches(f.as_str()).map(String::from).collect()),
            _ => fail!("find expects a string and a regex or string"),
        },
        "match" => match (text_of(a0), a1) {
            (None, _) => Null,
            (Some(s), Re(r)) => match anchored(r).captures(&s) {
                Some(c) => Array(c.iter().skip(1).map(|g| g.map_or(Null, |m| Str(m.as_str().into()))).collect()),
                None => Null,
            },
            _ => fail!("match expects a string and a regex"),
        },
        "ngram" => match (a0, a1.as_i64()) {
            (Null, _) => Null,
            (Str(s), Some(n)) if n > 0 => {
                let words: Vec<&str> = s.split_whitespace().collect();
                let n = n as usize;
                if words.len() <= n {
                    arr(vec![s.clone()])
                } else {
                    arr(words.windows(n).map(|w| w.join(" ")).collect())
                }
            }
            _ => fail!("ngram expects a string and a positive number"),
        },
        "fingerprint" => text_of(a0).map_or(Null, |s| Str(fingerprint(&s))),
        "ngramFingerprint" => match text_of(a0) {
            None => Null,
            Some(s) => Str(ngram_fingerprint(&s, a1.as_i64().unwrap_or(1).max(1) as usize)),
        },
        "phonetic" => match (a0, a1) {
            (Str(s), Str(enc)) => match enc.as_str() {
                "soundex" => text::soundex(s).map_or_else(Value::err, Str),
                // ponytail: metaphone / double metaphone / metaphone3 still to port (Commons Codec + Metaphone3)
                "metaphone" | "doublemetaphone" | "metaphone3" => fail!("phonetic encoding '{enc}' is not supported yet"),
                _ => fail!("phonetic does not know how to handle the '{enc}' encoding"),
            },
            _ => fail!("phonetic expects at least one argument"),
        },
        "unicode" => match a0 {
            Str(s) => Array(s.chars().map(|c| Int(c as i64)).collect()),
            _ => Null,
        },
        "unicodeType" => match a0 {
            Str(s) => arr(s.chars().map(|c| text::category(c).to_string()).collect()),
            _ => Null,
        },
        "escape" => {
            let Some(s) = text_of(a0) else { return Str(String::new()) };
            Str(match a1.as_str() {
                Some("html") => text::escape_html(&s),
                Some("xml") => text::escape_xml(&s),
                Some("csv") => text::escape_csv(&s),
                Some("url") => text::url_encode(&s),
                Some("javascript") => text::escape_js(&s),
                _ => fail!("escape expects a string and one of html, xml, csv, url, javascript"),
            })
        }
        "unescape" => {
            let Some(s) = text_of(a0) else { return Null };
            Str(match a1.as_str() {
                Some("html") | Some("xml") => text::unescape_html(&s),
                Some("csv") => text::unescape_csv(&s),
                Some("url") => match text::url_decode(&s) {
                    Some(d) => d,
                    None => fail!("unescape: malformed url encoding"),
                },
                Some("javascript") => text::unescape_js(&s),
                _ => fail!("unescape expects a string and one of html, xml, csv, url, javascript"),
            })
        }
        "encode" | "decode" => match (a0, a1.as_str()) {
            (Str(s), Some(enc @ ("base64" | "base64url"))) => {
                // Guava decodes lenient about padding
                let cfg = base64::engine::GeneralPurposeConfig::new().with_decode_padding_mode(base64::engine::DecodePaddingMode::Indifferent).with_decode_allow_trailing_bits(true);
                let eng = base64::engine::GeneralPurpose::new(if enc == "base64" { &base64::alphabet::STANDARD } else { &base64::alphabet::URL_SAFE }, cfg);
                if name == "encode" {
                    Str(eng.encode(s))
                } else {
                    match eng.decode(s.trim()) {
                        Ok(b) => Str(String::from_utf8_lossy(&b).into_owned()),
                        Err(e) => fail!("decode: {e}"),
                    }
                }
            }
            _ => fail!("{name} expects two arguments: a string and an encoding"),
        },
        "md5" | "sha1" => match a0 {
            Str(s) => {
                use md5::Digest;
                let hex = |b: &[u8]| b.iter().map(|x| format!("{x:02x}")).collect::<String>();
                Str(if name == "md5" { hex(&md5::Md5::digest(s.as_bytes())) } else { hex(&sha1::Sha1::digest(s.as_bytes())) })
            }
            _ => fail!("{name} expects a string"),
        },
        "reinterpret" => match a0 {
            Str(s) => Str(s.clone()),
            _ => fail!("reinterpret expects string to reinterpret, an encoding and an optional source encoding"),
        },
        // ponytail: needs a language-identification model; OpenRefine uses Optimaize language-detector
        "detectLanguage" => fail!("detectLanguage: language detection failed"),
        "levenshteinDistance" => match (a0, a1) {
            (Str(x), Str(y)) => Int(text::levenshtein(x, y) as i64),
            _ => fail!("levenshteinDistance expects two strings"),
        },
        "diff" => match (a0, a1) {
            (Date(x), Date(y)) => match dates::diff(x, y, a2.as_str().unwrap_or("day")) {
                Some(n) => Int(n),
                None => fail!("diff: unknown time unit"),
            },
            (Str(x), Str(y)) => {
                let common = x.chars().zip(y.chars()).take_while(|(p, q)| p == q).count();
                Str(y.chars().skip(common).collect())
            }
            _ => fail!("diff expects two strings or two dates and a unit"),
        },
        "range" => {
            let nums: Vec<i64> = match a {
                [Str(s)] => s.split(',').filter_map(|p| p.trim().parse().ok()).collect(),
                _ => a.iter().filter_map(Value::as_i64).collect(),
            };
            let (from, to, step) = match nums.as_slice() {
                [n] => (0, *n, 1),
                [f, t] => (*f, *t, 1),
                [f, t, s] => (*f, *t, *s),
                _ => fail!("range expects one to three numbers"),
            };
            if step == 0 {
                fail!("range step must not be zero");
            }
            let mut out = vec![];
            let mut x = from;
            while (step > 0 && x < to) || (step < 0 && x > to) {
                out.push(Int(x));
                x += step;
            }
            Array(out)
        }
        "parseUri" => match a0 {
            Str(s) => match url::Url::parse(s) {
                Ok(u) if u.has_host() => {
                    let mut m = serde_json::Map::new();
                    m.insert("scheme".into(), u.scheme().into());
                    m.insert("host".into(), u.host_str().unwrap_or("").into());
                    m.insert("port".into(), u.port().map_or(serde_json::Value::from(-1), serde_json::Value::from));
                    m.insert("path".into(), u.path().into());
                    m.insert("query".into(), u.query().unwrap_or("").into());
                    m.insert("authority".into(), s.split("://").nth(1).and_then(|r| r.split(['/', '?', '#']).next()).unwrap_or("").into());
                    m.insert("fragment".into(), u.fragment().unwrap_or("").into());
                    // OpenRefine 3.10 returns an empty query_params object
                    m.insert("query_params".into(), serde_json::Value::Object(Default::default()));
                    Json(Box::new(serde_json::Value::Object(m)))
                }
                _ => fail!("Invalid URI {s}"),
            },
            _ => fail!("parseUri expects a string"),
        },

        // ---------------------------------------------------------------- html / xml
        "parseHtml" | "parseXml" => match a0 {
            Str(s) => Html(Box::new(Markup { markup: s.clone(), document: true })),
            _ => fail!("{name} expects a single string as an argument"),
        },
        "select" => match (a0, a1) {
            (Html(h), Str(css)) => {
                let Ok(sel) = scraper::Selector::parse(css) else { fail!("select: invalid selector {css}") };
                let doc = html_doc(&h.markup, h.document);
                Array(doc.select(&sel).map(|e| Html(Box::new(Markup { markup: e.html(), document: false }))).collect())
            }
            _ => fail!("select expects an HTML element and a CSS selector"),
        },
        "htmlText" | "xmlText" | "wholeText" | "ownText" | "innerHtml" | "innerXml" | "scriptText" => match a0 {
            Html(h) => with_element(&h.markup, h.document, |e| match name {
                "htmlText" | "xmlText" => norm_ws(&e.text().collect::<String>()),
                "wholeText" | "scriptText" => e.text().collect(),
                "ownText" => norm_ws(&e.children().filter_map(|n| n.value().as_text().map(|t| t.to_string())).collect::<String>()),
                "innerXml" => e.children().filter_map(scraper::ElementRef::wrap).map(|c| c.html()).collect(),
                _ => e.inner_html().trim().to_string(),
            })
            .map_or(Str(String::new()), Str),
            _ => fail!("{name} expects an HTML element"),
        },
        "htmlAttr" | "xmlAttr" => match (a0, a1) {
            (Html(h), Str(attr)) => Str(with_element(&h.markup, h.document, |e| e.value().attr(attr).unwrap_or("").to_string()).unwrap_or_default()),
            _ => fail!("{name} expects an HTML element and an attribute name"),
        },
        "parent" => fail!("parent is not supported yet"),

        // ---------------------------------------------------------------- arrays
        "join" => match (list(a0), a1) {
            (Some(l), Str(sep)) => Str(l.iter().map(|v| v.to_java_string()).collect::<Vec<_>>().join(sep)),
            _ => fail!("join expects an array and a string"),
        },
        "reverse" => match list(a0) {
            Some(mut l) => {
                l.reverse();
                Array(l)
            }
            None => fail!("reverse expects an array"),
        },
        "sort" => match list(a0) {
            Some(mut l) => {
                if l.iter().all(|v| matches!(v, Str(_))) {
                    l.sort_by(|x, y| x.as_str().unwrap().encode_utf16().cmp(y.as_str().unwrap().encode_utf16()));
                } else if l.iter().all(|v| v.as_f64().is_some()) {
                    l.sort_by(|x, y| x.as_f64().unwrap().total_cmp(&y.as_f64().unwrap()));
                } else if l.iter().all(|v| matches!(v, Date(_))) {
                    l.sort_by_key(|v| match v {
                        Date(d) => *d,
                        _ => unreachable!(),
                    });
                } else {
                    fail!("sort expects an array of uniform type");
                }
                Array(l)
            }
            None => fail!("sort expects an array"),
        },
        "uniques" => match list(a0) {
            Some(l) => {
                let mut out: Vec<Value> = vec![];
                for v in l {
                    if !out.contains(&v) {
                        out.push(v);
                    }
                }
                Array(out)
            }
            None => fail!("uniques expects an array"),
        },
        "inArray" => match (list(a0), a1) {
            (Some(l), Str(s)) => Bool(l.iter().any(|v| v.as_str() == Some(s))),
            _ => fail!("inArray expects an array and a string"),
        },

        // ---------------------------------------------------------------- dates
        "inc" => match (a0, a1.as_i64(), a2) {
            (Date(d), Some(n), Str(unit)) => dates::inc(d, n, unit).map_or_else(|| Value::err("inc: unknown unit or out of range"), Date),
            _ => fail!("inc expects a date, a number and a string"),
        },
        "datePart" => match (a0, a1) {
            (Date(d), Str(p)) => match dates::part(d, p) {
                Some(dates::Part::Num(n)) => Int(n),
                Some(dates::Part::Text(t)) => Str(t.into()),
                None => fail!("datePart: unknown part {p}"),
            },
            _ => fail!("datePart expects a date and a string"),
        },

        // ---------------------------------------------------------------- math
        "round" | "floor" | "ceil" => match a0 {
            Int(i) => Int(*i),
            Num(x) => int_result(match name {
                "round" => (x + 0.5).floor(),
                "floor" => x.floor(),
                _ => x.ceil(),
            }),
            _ => fail!("{name} expects a number"),
        },
        "abs" => match a0 {
            Int(i) => Int(i.wrapping_abs()),
            Num(x) => Num(x.abs()),
            _ => fail!("abs expects a number"),
        },
        "mod" | "quotient" => match (a0.as_i64(), a1.as_i64()) {
            (Some(_), Some(0)) => fail!("{name}: division by zero"),
            (Some(x), Some(y)) => Int(if name == "mod" { x % y } else { x / y }),
            _ => fail!("{name} expects two numbers"),
        },
        "gcd" | "lcm" => match (a0.as_i64(), a1.as_i64()) {
            (Some(x), Some(y)) => Int(if name == "gcd" { gcd(x, y) } else if x == 0 || y == 0 { 0 } else { (x / gcd(x, y) * y).abs() }),
            _ => fail!("{name} expects two numbers"),
        },
        "fact" => match a0.as_i64() {
            Some(n) if n >= 0 => factorial(n).map_or_else(|| Value::err("java.lang.ArithmeticException: Integer overflow computing factorial"), Int),
            _ => fail!("fact expects a non-negative number"),
        },
        "factn" => match (a0.as_i64(), a1.as_i64()) {
            (Some(n), Some(k)) if k > 0 => {
                let mut p: i64 = 1;
                let mut x = n;
                while x > 0 {
                    p = p.wrapping_mul(x);
                    x -= k;
                }
                Int(p)
            }
            _ => fail!("factn expects two numbers"),
        },
        "combin" => match (a0.as_i64(), a1.as_i64()) {
            (Some(n), Some(k)) if (0..=n).contains(&k) => {
                let mut r: i64 = 1;
                for i in 0..k.min(n - k) {
                    r = r * (n - i) / (i + 1);
                }
                Int(r)
            }
            _ => fail!("combin expects two numbers, n >= k >= 0"),
        },
        "multinomial" => {
            let Some(ks) = a.iter().map(Value::as_i64).collect::<Option<Vec<i64>>>() else { fail!("multinomial expects numbers") };
            let (mut r, mut total) = (1i64, 0i64);
            for k in ks {
                for i in 1..=k {
                    total += 1;
                    r = r * total / i;
                }
            }
            Int(r)
        }
        "even" | "odd" => match a0.as_f64() {
            Some(x) => {
                let sign = if x < 0.0 { -1.0 } else { 1.0 };
                let mut n = x.abs().ceil();
                let want_even = name == "even";
                if (n as i64 % 2 == 0) != want_even {
                    n += 1.0;
                }
                if !want_even && n == 0.0 {
                    n = 1.0;
                }
                Int((sign * n) as i64)
            }
            None => fail!("{name} expects a number"),
        },
        "sum" => match list(a0) {
            Some(l) => match num_args(&l, "sum") {
                Ok(n) => Num(n.iter().sum()),
                Err(e) => e,
            },
            None => fail!("sum expects an array of numbers"),
        },
        "pow" | "max" | "min" | "atan2" | "log" | "random" | "randomNumber" | "exp" | "ln" | "sin" | "cos" | "tan" | "asin" | "acos" | "atan" | "sinh"
        | "cosh" | "tanh" | "degrees" | "radians" => {
            let n = match num_args(a, name) {
                Ok(n) => n,
                Err(e) => return e,
            };
            let x = n.first().copied().unwrap_or(f64::NAN);
            let y = n.get(1).copied().unwrap_or(f64::NAN);
            Num(match name {
                "pow" => x.powf(y),
                "max" => x.max(y),
                "min" => x.min(y),
                "atan2" => x.atan2(y),
                "log" => x.log10(),
                "random" | "randomNumber" => {
                    let r = std::collections::hash_map::RandomState::new();
                    let seed = std::hash::BuildHasher::hash_one(&r, 0u8) as f64 / u64::MAX as f64;
                    return if n.len() == 2 { Int((x + seed * (y - x)).floor() as i64) } else { Num(seed) };
                }
                "exp" => x.exp(),
                "ln" => x.ln(),
                "sin" => x.sin(),
                "cos" => x.cos(),
                "tan" => x.tan(),
                "asin" => x.asin(),
                "acos" => x.acos(),
                "atan" => x.atan(),
                "sinh" => x.sinh(),
                // fdlibm (Java StrictMath) formula, which differs from libm in the last digit
                "cosh" => {
                    let t = x.abs().exp();
                    0.5 * t + 0.5 / t
                }
                "tanh" => x.tanh(),
                "degrees" => x.to_degrees(),
                _ => x.to_radians(),
            })
        }

        // ---------------------------------------------------------------- booleans
        "and" | "or" | "xor" => {
            let Some(b) = a.iter().map(|v| if let Bool(b) = v { Some(*b) } else { None }).collect::<Option<Vec<bool>>>() else { fail!("{name} expects booleans") };
            Bool(match name {
                "and" => b.iter().all(|x| *x),
                "or" => b.iter().any(|x| *x),
                _ => b.iter().filter(|x| **x).count() % 2 == 1,
            })
        }
        "not" => match a0 {
            Bool(b) => Bool(!b),
            _ => fail!("not expects a boolean"),
        },
        // ponytail: cross() and facetCount() need the project (other columns' value index); wire through Row later
        "facetCount" => fail!("facetCount needs a project"),
        "cross" => fail!("cross is not supported yet"),
        _ => fail!("Unknown function {name}"),
    }
}

/// get(o, from[, to]) and o[from] / o[from, to].
fn get(o: &Value, from: &Value, to: Option<&Value>) -> Value {
    match (o, from) {
        (Null, _) => Null,
        (Obj(_), Str(k)) => super::eval::field(o.clone(), k),
        (Json(j), Str(k)) if j.is_object() => super::eval::field(o.clone(), k),
        (Str(s), _) => {
            let Some(i) = from.as_i64() else { fail!("get expects a number index") };
            let len = text::utf16_len(s) as i64;
            match to.and_then(Value::as_i64) {
                Some(t) => {
                    let (f, t) = slice_bounds(len, i, Some(t));
                    Str(text::utf16_slice(s, f, t))
                }
                None => {
                    let i = if i < 0 { (len + i).max(0) } else { i };
                    if i >= len {
                        fail!("java.lang.StringIndexOutOfBoundsException: index {i}, length {len}");
                    }
                    Str(text::utf16_slice(s, i as usize, i as usize + 1))
                }
            }
        }
        _ => match (list(o), from.as_i64()) {
            (Some(l), Some(i)) => {
                let len = l.len() as i64;
                match to.and_then(Value::as_i64) {
                    Some(t) => {
                        let (f, t) = slice_bounds(len, i, Some(t));
                        Array(l[f..t].to_vec())
                    }
                    None => {
                        let i = if i < 0 { (len + i).max(0) } else { i };
                        if i >= len {
                            fail!("java.lang.ArrayIndexOutOfBoundsException: Index {i} out of bounds for length {len}");
                        }
                        l[i as usize].clone()
                    }
                }
            }
            _ => Null,
        },
    }
}

use chrono::Timelike;
