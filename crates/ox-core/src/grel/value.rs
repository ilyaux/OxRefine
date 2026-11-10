//! OpenRefine's value model: what a cell can hold and what a GREL expression can produce.
use chrono::NaiveDateTime;
use std::hash::{Hash, Hasher};

#[derive(Clone, Debug)]
pub enum Value {
    Null,
    Bool(bool),
    /// Java Long: integer literals, toNumber of integral text, round/floor/ceil.
    Int(i64),
    /// Java Double.
    Num(f64),
    Str(String),
    /// OffsetDateTime in OpenRefine, always shown in UTC.
    Date(NaiveDateTime),
    Array(Vec<Value>),
    /// Objects and arrays from parseJson (Jackson nodes); scalars are converted to native values.
    Json(serde_json::Value),
    /// A parsed HTML/XML element or document, kept as markup (re-parsed on demand).
    Html { markup: String, document: bool },
    /// cell / row / cells objects exposed to expressions.
    Obj(Vec<(String, Value)>),
    /// A regex literal passed as a function argument.
    Re(regex::Regex),
    Error(String),
}

pub use Value::*;

impl Value {
    pub fn err(msg: impl Into<String>) -> Value {
        Error(msg.into())
    }
    pub fn is_error(&self) -> bool {
        matches!(self, Error(_))
    }
    pub fn is_null(&self) -> bool {
        matches!(self, Null)
    }
    /// ExpressionUtils.isBlank: null or the empty string.
    pub fn is_blank(&self) -> bool {
        matches!(self, Null) || matches!(self, Str(s) if s.is_empty())
    }
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Str(s) | Json(serde_json::Value::String(s)) => Some(s),
            _ => None,
        }
    }
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Int(i) => Some(*i as f64),
            Num(n) => Some(*n),
            Json(serde_json::Value::Number(n)) => n.as_f64(),
            _ => None,
        }
    }
    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Int(i) => Some(*i),
            Num(n) => Some(*n as i64),
            _ => None,
        }
    }
    pub fn type_name(&self) -> &'static str {
        match self {
            Null => "undefined",
            Bool(_) => "boolean",
            Int(_) | Num(_) => "number",
            Str(_) => "string",
            Date(_) => "date",
            Array(_) => "array",
            Json(serde_json::Value::Array(_)) => "array",
            Json(serde_json::Value::Number(_)) => "number",
            Json(serde_json::Value::String(_)) => "string",
            Json(serde_json::Value::Bool(_)) => "boolean",
            Json(_) => "object",
            Html { .. } | Obj(_) => "object",
            Re(_) => "regex",
            Error(_) => "error",
        }
    }

    /// What OpenRefine shows for a result (expression preview, grid): `None` means null.
    pub fn render(&self) -> Option<String> {
        Some(match self {
            Null => return None,
            Str(s) => s.clone(),
            Num(n) => render_num(*n),
            Array(_) => json_pretty(self, 0),
            Date(d) => format!("[date {}]", fmt_date(d)),
            _ => self.to_java_string(),
        })
    }

    /// Java `toString()` / GREL `toString(x)`: Doubles keep Java formatting ("1.0", "1.0E-6").
    pub fn to_java_string(&self) -> String {
        match self {
            Null => String::new(),
            Bool(b) => b.to_string(),
            Int(i) => i.to_string(),
            Num(n) => java_double(*n),
            Str(s) => s.clone(),
            Date(d) => fmt_date(d),
            Array(a) => format!("[{}]", a.iter().map(|v| v.to_java_string()).collect::<Vec<_>>().join(", ")),
            Json(j) => j.to_string(),
            Html { markup, .. } => markup.clone(),
            Obj(f) => format!("{{{}}}", f.iter().map(|(k, v)| format!("{k}={}", v.to_java_string())).collect::<Vec<_>>().join(", ")),
            Re(r) => r.as_str().to_string(),
            Error(e) => e.clone(),
        }
    }

    /// Text a cell shows and exports: like render, with null as "".
    pub fn display(&self) -> String {
        self.render().unwrap_or_default()
    }
}

pub fn fmt_date(d: &NaiveDateTime) -> String {
    let ms = d.and_utc().timestamp_subsec_millis();
    if ms == 0 {
        d.format("%Y-%m-%dT%H:%M:%SZ").to_string()
    } else {
        d.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()
    }
}

/// Preview/grid rendering of a Double: whole values print as the Long they cast to.
pub fn render_num(n: f64) -> String {
    if n.is_finite() && n == n.trunc() {
        (n as i64).to_string()
    } else {
        java_double(n)
    }
}

/// Java's Double.toString (shortest round-trip digits, as in JDK 19+).
pub fn java_double(x: f64) -> String {
    if x.is_nan() {
        return "NaN".into();
    }
    if x.is_infinite() {
        return if x > 0.0 { "Infinity" } else { "-Infinity" }.into();
    }
    if x == 0.0 {
        return if x.is_sign_negative() { "-0.0" } else { "0.0" }.into();
    }
    let a = x.abs();
    if (1e-3..1e7).contains(&a) {
        let s = x.to_string();
        if s.contains('.') { s } else { s + ".0" }
    } else {
        let s = format!("{x:e}");
        let (mant, exp) = s.split_once('e').unwrap();
        let mant = if mant.contains('.') { mant.to_string() } else { format!("{mant}.0") };
        format!("{mant}E{exp}")
    }
}

fn json_str(s: &str) -> String {
    serde_json::to_string(s).unwrap()
}

/// Jackson DefaultPrettyPrinter, which OpenRefine uses for arrays in previews and for jsonize().
pub fn json_pretty(v: &Value, depth: usize) -> String {
    match v {
        Null => "null".into(),
        Bool(b) => b.to_string(),
        Int(i) => i.to_string(),
        Num(n) => render_num(*n),
        Str(s) => json_str(s),
        Date(d) => json_str(&fmt_date(d)),
        Array(a) if a.is_empty() => "[  ]".into(),
        Array(a) => format!("[ {} ]", a.iter().map(|x| json_pretty(x, depth)).collect::<Vec<_>>().join(", ")),
        Json(j) => json_node_pretty(j, depth),
        Html { markup, .. } => json_str(markup),
        Obj(f) => {
            let pad = "  ".repeat(depth + 1);
            let items: Vec<_> = f.iter().map(|(k, v)| format!("{pad}{} : {}", json_str(k), json_pretty(v, depth + 1))).collect();
            format!("{{
{}
{}}}", items.join(",
"), "  ".repeat(depth))
        }
        Re(r) => json_str(r.as_str()),
        Error(e) => json_str(e),
    }
}

fn json_node_pretty(j: &serde_json::Value, depth: usize) -> String {
    use serde_json::Value as J;
    match j {
        J::Array(a) if a.is_empty() => "[ ]".into(),
        J::Array(a) => format!("[ {} ]", a.iter().map(|x| json_node_pretty(x, depth)).collect::<Vec<_>>().join(", ")),
        J::Object(o) if o.is_empty() => "{ }".into(),
        J::Object(o) => {
            let pad = "  ".repeat(depth + 1);
            let items: Vec<_> = o.iter().map(|(k, v)| format!("{pad}{} : {}", json_str(k), json_node_pretty(v, depth + 1))).collect();
            format!("{{\n{}\n{}}}", items.join(",\n"), "  ".repeat(depth))
        }
        other => other.to_string(),
    }
}

/// Native value for a JSON node: scalars become GREL values, containers stay JSON.
pub fn from_json(j: serde_json::Value) -> Value {
    use serde_json::Value as J;
    match j {
        J::Null => Null,
        J::Bool(b) => Bool(b),
        J::Number(n) => n.as_i64().map(Int).unwrap_or_else(|| Num(n.as_f64().unwrap_or(f64::NAN))),
        J::String(s) => Str(s),
        other => Json(other),
    }
}

// Equality and hashing let typed values live in a column dictionary.
impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Null, Null) => true,
            (Bool(a), Bool(b)) => a == b,
            (Int(a), Int(b)) => a == b,
            (Num(a), Num(b)) => a.to_bits() == b.to_bits(),
            (Str(a), Str(b)) | (Error(a), Error(b)) => a == b,
            (Date(a), Date(b)) => a == b,
            (Array(a), Array(b)) => a == b,
            (Json(a), Json(b)) => a == b,
            (Html { markup: a, document: x }, Html { markup: b, document: y }) => a == b && x == y,
            (Obj(a), Obj(b)) => a == b,
            (Re(a), Re(b)) => a.as_str() == b.as_str(),
            _ => false,
        }
    }
}
impl Eq for Value {}
impl Hash for Value {
    fn hash<H: Hasher>(&self, h: &mut H) {
        std::mem::discriminant(self).hash(h);
        match self {
            Null => {}
            Bool(b) => b.hash(h),
            Int(i) => i.hash(h),
            Num(n) => n.to_bits().hash(h),
            Str(s) | Error(s) => s.hash(h),
            Date(d) => d.hash(h),
            Array(a) => a.hash(h),
            Json(j) => j.to_string().hash(h),
            Html { markup, document } => (markup, document).hash(h),
            Obj(f) => f.hash(h),
            Re(r) => r.as_str().hash(h),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn java_number_formats() {
        assert_eq!(java_double(1.0), "1.0");
        assert_eq!(java_double(123456789.0), "1.23456789E8");
        assert_eq!(java_double(0.000001), "1.0E-6");
        assert_eq!(java_double(0.1 + 0.2), "0.30000000000000004");
        assert_eq!(render_num(3.0), "3");
        assert_eq!(render_num(0.000001), "1.0E-6");
        assert_eq!(render_num(7.766279631452242e18), "7766279631452241920");
        assert_eq!(json_pretty(&Array(vec![Int(1), Num(2.0), Str("x".into())]), 0), "[ 1, 2, \"x\" ]");
    }
}
