//! OpenRefine's wikibase extension (3.10.1): the schema a project keeps as its `wikibaseSchema`
//! overlay model, its evaluation on rows into entity edits, and the QuickStatements and schema
//! exports. Checked against tests/wikibase/fixtures.json (crates/ox-core/tests/wikibase.rs).
use crate::{grel::Value, recon::Judgment, table::Table, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value as J};
use std::{cell::RefCell, collections::{HashMap, HashSet}};
#[path = "wikibase_upload.rs"]
mod upload;
pub use upload::{login, login_status, logout, upload, Settings, Uploaded};

// ------------------------------------------------------------ the schema, in OpenRefine's key order

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Schema {
    #[serde(rename = "entityEdits", alias = "itemDocuments", default)]
    pub entity_edits: Vec<EntityExpr>,
    #[serde(rename = "siteIri", default, skip_serializing_if = "Option::is_none")]
    pub site_iri: Option<String>,
    #[serde(rename = "entityTypeSiteIRI", default, skip_serializing_if = "Option::is_none")]
    pub entity_type_site_iri: Option<J>,
    #[serde(rename = "mediaWikiApiEndpoint", default, skip_serializing_if = "Option::is_none")]
    pub api: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum EntityExpr {
    #[serde(rename = "wbitemeditexpr", rename_all = "camelCase")]
    Item {
        subject: Expr,
        #[serde(default)]
        name_descs: Vec<TermExpr>,
        #[serde(default)]
        statement_groups: Vec<GroupExpr>,
    },
    #[serde(rename = "wbmediainfoeditexpr", rename_all = "camelCase")]
    MediaInfo {
        subject: Expr,
        #[serde(default)]
        name_descs: Vec<TermExpr>,
        #[serde(default)]
        statement_groups: Vec<GroupExpr>,
        #[serde(default)]
        file_path: Option<Expr>,
        #[serde(default)]
        file_name: Option<Expr>,
        #[serde(default)]
        wikitext: Option<Expr>,
        #[serde(default)]
        override_wikitext: bool,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum TermType {
    #[serde(rename = "LABEL")]
    Label,
    #[serde(rename = "LABEL_IF_NEW")]
    LabelIfNew,
    #[serde(rename = "DESCRIPTION")]
    Description,
    #[serde(rename = "DESCRIPTION_IF_NEW")]
    DescriptionIfNew,
    #[serde(rename = "ALIAS")]
    Alias,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TermExpr {
    pub name_type: TermType,
    pub value: Expr,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GroupExpr {
    pub property: Expr,
    #[serde(default)]
    pub statements: Vec<StatementExpr>,
}

fn strict() -> J {
    json!({ "type": "qualifiers", "valueMatcher": { "type": "strict" }, "pids": [] })
}
fn add_or_merge() -> String {
    "add_or_merge".into()
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StatementExpr {
    #[serde(default)]
    pub value: Option<Expr>,
    #[serde(default)]
    pub qualifiers: Vec<SnakExpr>,
    #[serde(default)]
    pub references: Vec<ReferenceExpr>,
    #[serde(default = "strict")]
    pub merging_strategy: J,
    #[serde(default = "add_or_merge")]
    pub mode: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SnakExpr {
    pub prop: Expr,
    pub value: Expr,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReferenceExpr {
    #[serde(default)]
    pub snaks: Vec<SnakExpr>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Expr {
    #[serde(rename = "wbentityvariable", rename_all = "camelCase")]
    EntityVariable { column_name: String },
    #[serde(rename = "wbentityidvalueconstant")]
    EntityConstant { id: String, #[serde(default)] label: String },
    #[serde(rename = "wbitemconstant")]
    ItemConstant { qid: String, #[serde(default)] label: String },
    #[serde(rename = "wbpropconstant")]
    PropConstant { pid: String, #[serde(default)] label: String, #[serde(default)] datatype: String },
    /// OpenRefine trims a string constant when it reads the schema
    #[serde(rename = "wbstringconstant")]
    StringConstant { #[serde(deserialize_with = "trimmed")] value: String },
    #[serde(rename = "wbstringvariable", rename_all = "camelCase")]
    StringVariable { column_name: String },
    #[serde(rename = "wblanguageconstant")]
    LanguageConstant { id: String, #[serde(default)] label: String },
    #[serde(rename = "wblanguagevariable", rename_all = "camelCase")]
    LanguageVariable { column_name: String },
    #[serde(rename = "wbdateconstant")]
    DateConstant { value: String },
    #[serde(rename = "wbdatevariable", rename_all = "camelCase")]
    DateVariable { column_name: String },
    #[serde(rename = "wblocationconstant")]
    LocationConstant { value: String },
    #[serde(rename = "wblocationvariable", rename_all = "camelCase")]
    LocationVariable { column_name: String },
    #[serde(rename = "wbmonolingualexpr")]
    Monolingual { language: Box<Expr>, value: Box<Expr> },
    #[serde(rename = "wbquantityexpr")]
    Quantity { amount: Box<Expr>, #[serde(default)] unit: Option<Box<Expr>> },
}

fn trimmed<'de, D: serde::Deserializer<'de>>(d: D) -> std::result::Result<String, D::Error> {
    Ok(String::deserialize(d)?.trim().to_string())
}

impl Expr {
    fn walk<'a>(&'a self, out: &mut Vec<&'a Expr>) {
        out.push(self);
        match self {
            Expr::Monolingual { language, value } => { language.walk(out); value.walk(out); }
            Expr::Quantity { amount, unit } => { amount.walk(out); if let Some(u) = unit { u.walk(out); } }
            _ => {}
        }
    }
}

impl Schema {
    pub fn parse(schema: &J) -> Result<Schema> {
        serde_json::from_value(schema.clone()).map_err(|e| format!("Invalid Wikibase schema: {e}"))
    }

    fn exprs(&self) -> Vec<&Expr> {
        let mut out = vec![];
        for e in &self.entity_edits {
            let (subject, terms, groups) = match e {
                EntityExpr::Item { subject, name_descs, statement_groups } | EntityExpr::MediaInfo { subject, name_descs, statement_groups, .. } => (subject, name_descs, statement_groups),
            };
            subject.walk(&mut out);
            if let EntityExpr::MediaInfo { file_path, file_name, wikitext, .. } = e {
                for x in [file_path, file_name, wikitext].into_iter().flatten() { x.walk(&mut out); }
            }
            for t in terms { t.value.walk(&mut out); }
            for g in groups {
                g.property.walk(&mut out);
                for s in &g.statements {
                    if let Some(v) = &s.value { v.walk(&mut out); }
                    for snak in s.qualifiers.iter().chain(s.references.iter().flat_map(|r| &r.snaks)) {
                        snak.prop.walk(&mut out);
                        snak.value.walk(&mut out);
                    }
                }
            }
        }
        out
    }

    /// The constants OpenRefine refuses when it saves a schema, with its messages.
    fn errors(&self) -> Vec<String> {
        self.exprs().into_iter().filter_map(|e| match e {
            Expr::DateConstant { value } if parse_date(value).is_none() => Some(format!("Invalid date provided: '{value}'")),
            Expr::LocationConstant { value } if parse_location(value).is_none() => Some(format!("Invalid geographical coordinates: '{value}'")),
            _ => None,
        }).collect()
    }
}

/// A schema as OpenRefine stores it: its own key order and defaults. Invalid constants are refused.
pub fn normalize(schema: &J) -> Result<J> {
    let s = Schema::parse(schema)?;
    let errors = s.errors();
    if !errors.is_empty() {
        return Err(format!("Invalid Wikibase schema: {}", errors.join("; ")));
    }
    serde_json::to_value(&s).map_err(crate::err)
}

// ------------------------------------------------------------ values

#[derive(Clone, Debug, PartialEq)]
pub struct Date { pub year: i64, pub month: u32, pub day: u32, pub precision: u8, pub calendar: Option<String> }

/// OpenRefine's date formats: YYYY, YYYY-MM, YYYY-MM-DD, a time of day (dropped: day precision),
/// a decade as 1950D, a calendar as a `_Q…` suffix. Negative years are BCE.
pub fn parse_date(text: &str) -> Option<Date> {
    let text = text.trim();
    let (text, calendar) = match text.rsplit_once("_Q") {
        Some((d, q)) if !q.is_empty() && q.bytes().all(|b| b.is_ascii_digit()) => (d, Some(format!("Q{q}"))),
        _ => (text, None),
    };
    let (negative, body) = text.strip_prefix('-').map_or((false, text), |b| (true, b));
    let year = |s: &str| -> Option<i64> { (!s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())).then(|| s.parse::<i64>().ok()).flatten().map(|y| if negative { -y } else { y }) };
    let two = |s: &str, max: u32| -> Option<u32> { (s.len() == 2 && s.bytes().all(|b| b.is_ascii_digit())).then(|| s.parse().ok()).flatten().filter(|&n| (1..=max).contains(&n)) };
    if let Some(decade) = body.strip_suffix('D') {
        return year(decade).map(|year| Date { year, month: 0, day: 0, precision: 8, calendar });
    }
    let (date, time) = body.split_once('T').map_or((body, None), |(d, t)| (d, Some(t)));
    if let Some(t) = time {
        let t = t.strip_suffix('Z').unwrap_or(t);
        let parts: Vec<&str> = t.split(':').collect();
        let ok = (2..=3).contains(&parts.len()) && parts.iter().enumerate().all(|(i, p)| {
            let p = if i == 2 { p.split('.').next().unwrap_or("") } else { p };
            p.len() == 2 && p.bytes().all(|b| b.is_ascii_digit())
        });
        if !ok { return None; }
    }
    let parts: Vec<&str> = date.split('-').collect();
    let (y, m, d, precision) = match parts.as_slice() {
        [y] if time.is_none() => (year(y)?, 0, 0, 9),
        [y, m] if time.is_none() => (year(y)?, two(m, 12)?, 0, 10),
        [y, m, d] => (year(y)?, two(m, 12)?, two(d, 31)?, 11),
        _ => return None,
    };
    Some(Date { year: y, month: m, day: d, precision, calendar })
}

/// "lat,lng" or "lat/lng".
pub fn parse_location(text: &str) -> Option<(f64, f64)> {
    let (a, b) = text.split_once(',').or_else(|| text.split_once('/'))?;
    let (lat, lng) = (a.trim().parse::<f64>().ok()?, b.trim().parse::<f64>().ok()?);
    ((-90.0..=90.0).contains(&lat) && (-180.0..=180.0).contains(&lng)).then_some((lat, lng))
}

/// A decimal as Java's BigDecimal holds it: digits and a scale (1e3 has scale -3).
#[derive(Clone, Debug, PartialEq)]
pub struct Decimal { unscaled: i128, scale: i32 }

impl Decimal {
    pub fn parse(text: &str) -> Option<Decimal> {
        let text = text.trim();
        let (mantissa, exponent) = match text.find(['e', 'E']) {
            Some(i) => (&text[..i], text[i + 1..].strip_prefix('+').unwrap_or(&text[i + 1..]).parse::<i32>().ok()?),
            None => (text, 0),
        };
        let (negative, digits) = match mantissa.strip_prefix('-') { Some(d) => (true, d), None => (false, mantissa.strip_prefix('+').unwrap_or(mantissa)) };
        let (int, frac) = digits.split_once('.').unwrap_or((digits, ""));
        if int.is_empty() && frac.is_empty() || !int.bytes().chain(frac.bytes()).all(|b| b.is_ascii_digit()) { return None; }
        let unscaled: i128 = format!("{int}{frac}").parse().ok()?;
        Some(Decimal { unscaled: if negative { -unscaled } else { unscaled }, scale: frac.len() as i32 - exponent })
    }

    pub fn is_integer(&self) -> bool {
        self.scale <= 0 || 10i128.checked_pow(self.scale as u32).is_some_and(|p| self.unscaled % p == 0)
    }

    /// BigDecimal.toPlainString
    pub fn plain(&self) -> String {
        let digits = self.unscaled.unsigned_abs().to_string();
        let sign = if self.unscaled < 0 { "-" } else { "" };
        if self.scale <= 0 {
            return format!("{sign}{digits}{}", "0".repeat((-self.scale) as usize));
        }
        let scale = self.scale as usize;
        let padded = format!("{digits:0>width$}", width = scale + 1);
        let (int, frac) = padded.split_at(padded.len() - scale);
        format!("{sign}{int}.{frac}")
    }

    /// Numbers written with an exponent get bounds of half their last digit, as OpenRefine sets them.
    fn bounds(&self) -> Option<(Decimal, Decimal)> {
        if self.scale >= 0 { return None; }
        let value = self.unscaled.checked_mul(10i128.checked_pow((-self.scale) as u32)?)?;
        let half = 5i128.checked_mul(10i128.checked_pow((-self.scale - 1) as u32)?)?;
        Some((Decimal { unscaled: value - half, scale: 0 }, Decimal { unscaled: value + half, scale: 0 }))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Val {
    /// an entity: an existing id, or a new item by its Recon id
    Entity(EntityId),
    Str(String),
    Mono { text: String, lang: String },
    Date(Date),
    Location(f64, f64),
    Quantity { amount: Decimal, bounds: Option<(Decimal, Decimal)>, unit: Option<String> },
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum EntityId {
    Existing(String),
    New(i64),
}

/// What a schema needs from the outside: the language codes its Wikibase accepts. Values it had
/// to leave out are noted, as OpenRefine's evaluation warnings (kind, example).
pub struct Context<'a> {
    pub site_iri: &'a str,
    pub languages: &'a dyn Fn(&str) -> bool,
    pub warnings: RefCell<Vec<(&'static str, String)>>,
}

impl Context<'_> {
    fn warn(&self, kind: &'static str, example: &str) {
        self.warnings.borrow_mut().push((kind, example.to_string()));
    }
}

fn cell_text(t: &Table, row: usize, column: &str) -> Option<String> {
    let c = t.col(column).ok()?;
    let v = t.columns[c].get(row);
    if matches!(v, Value::Null) { return None; }
    let s = v.to_java_string().trim().to_string();
    (!s.is_empty()).then_some(s)
}

impl Expr {
    /// The value on a row; None skips the part of the edit that uses it, as OpenRefine does.
    fn eval(&self, t: &Table, row: usize, cx: &Context) -> Option<Val> {
        Some(match self {
            Expr::EntityVariable { column_name } => {
                let c = t.col(column_name).ok()?;
                let r = t.columns[c].recon(row)?;
                if r.identifier_space.as_deref() != Some(cx.site_iri) { return None; }
                match r.judgment {
                    Judgment::Matched => Val::Entity(EntityId::Existing(r.matched.as_ref()?.id.clone())),
                    Judgment::New => Val::Entity(EntityId::New(r.id)),
                    _ => return None,
                }
            }
            Expr::EntityConstant { id, .. } => Val::Entity(EntityId::Existing(id.clone())),
            Expr::ItemConstant { qid, .. } => Val::Entity(EntityId::Existing(qid.clone())),
            Expr::PropConstant { pid, .. } => Val::Entity(EntityId::Existing(pid.clone())),
            Expr::StringConstant { value } => Val::Str(Some(value.trim().to_string()).filter(|s| !s.is_empty())?),
            Expr::StringVariable { column_name } => Val::Str(cell_text(t, row, column_name)?),
            Expr::LanguageConstant { id, .. } => Val::Str(Some(id.clone()).filter(|l| (cx.languages)(l))?),
            Expr::LanguageVariable { column_name } => Val::Str(cell_text(t, row, column_name).filter(|l| (cx.languages)(l))?),
            Expr::DateConstant { value } => Val::Date(parse_date(value)?),
            Expr::DateVariable { column_name } => {
                let text = date_text(t, row, column_name)?;
                Val::Date(parse_date(&text).or_else(|| { cx.warn("ignored-date", &text); None })?)
            }
            Expr::LocationConstant { value } => { let (a, b) = parse_location(value)?; Val::Location(a, b) }
            Expr::LocationVariable { column_name } => {
                let text = cell_text(t, row, column_name)?;
                let (a, b) = parse_location(&text).or_else(|| { cx.warn("ignored-coordinates", &text); None })?;
                Val::Location(a, b)
            }
            Expr::Monolingual { language, value } => {
                let Val::Str(text) = value.eval(t, row, cx)? else { return None };
                let Some(Val::Str(lang)) = language.eval(t, row, cx) else {
                    cx.warn("monolingual-text-without-language", &text);
                    return None;
                };
                Val::Mono { text, lang }
            }
            Expr::Quantity { amount, unit } => {
                let Val::Str(text) = amount.eval(t, row, cx)? else { return None };
                // OpenRefine reads the amount upper-cased (1e3 as 1E3) and shows it so when it can't
                let Some(amount) = Decimal::parse(&text) else { cx.warn("ignored-amount", &text.to_uppercase()); return None };
                let unit = match unit {
                    Some(u) => match u.eval(t, row, cx)? { Val::Entity(EntityId::Existing(id)) => Some(id), _ => return None },
                    None => None,
                };
                Val::Quantity { bounds: amount.bounds(), amount, unit }
            }
        })
    }

    fn pid(&self) -> Option<&str> {
        match self { Expr::PropConstant { pid, .. } => Some(pid), _ => None }
    }
}

/// A date cell reads as its ISO date; text as written.
fn date_text(t: &Table, row: usize, column: &str) -> Option<String> {
    let c = t.col(column).ok()?;
    match t.columns[c].get(row) {
        Value::Date(d) => Some(d.format("%Y-%m-%dT%H:%M:%SZ").to_string()),
        _ => cell_text(t, row, column),
    }
}

// ------------------------------------------------------------ entity edits

#[derive(Clone, Debug, PartialEq)]
pub struct Statement { pub value: Option<Val>, pub qualifiers: Vec<(String, Val)>, pub references: Vec<Vec<(String, Val)>>, pub mode: String }

#[derive(Clone, Debug)]
pub struct Edit {
    pub subject: EntityId,
    pub media_info: bool,
    /// media info: the file's name and where to upload it from
    pub file_name: Option<String>,
    pub file_path: Option<String>,
    /// (term type, language, text) in the schema's order
    pub terms: Vec<(TermType, String, String)>,
    /// (property, statements) in the schema's order
    pub groups: Vec<(String, Vec<Statement>)>,
    pub rows: Vec<usize>,
}

impl Edit {
    fn is_empty(&self) -> bool {
        self.terms.is_empty() && self.groups.iter().all(|(_, s)| s.is_empty())
    }

    fn merge(&mut self, other: Edit) {
        for term in other.terms {
            if !self.terms.contains(&term) { self.terms.push(term); }
        }
        for (pid, statements) in other.groups {
            let at = match self.groups.iter().position(|(p, _)| *p == pid) {
                Some(at) => at,
                None => { self.groups.push((pid, vec![])); self.groups.len() - 1 }
            };
            let group = &mut self.groups[at].1;
            for s in statements {
                // the same claim from another row: its references join the first one
                match group.iter_mut().find(|g| g.value == s.value && g.qualifiers == s.qualifiers && g.mode == s.mode) {
                    Some(g) => for r in s.references { if !g.references.contains(&r) { g.references.push(r); } },
                    None => group.push(s),
                }
            }
        }
        self.rows.extend(other.rows);
        if self.file_name.is_none() { self.file_name = other.file_name; }
        if self.file_path.is_none() { self.file_path = other.file_path; }
    }
}

fn snaks(list: &[SnakExpr], t: &Table, row: usize, cx: &Context) -> Vec<(String, Val)> {
    list.iter().filter_map(|s| Some((s.prop.pid()?.to_string(), s.value.eval(t, row, cx)?))).collect()
}

/// Every non-empty entity edit of the rows, the same entity's edits merged into the first, as
/// OpenRefine's schema evaluation groups them.
pub fn evaluate(schema: &Schema, t: &Table, rows: &[u32], languages: &dyn Fn(&str) -> bool) -> Vec<Edit> {
    evaluate_noting(schema, t, rows, languages).0
}

/// The edits, and the evaluation warnings on the way.
fn evaluate_noting(schema: &Schema, t: &Table, rows: &[u32], languages: &dyn Fn(&str) -> bool) -> (Vec<Edit>, Vec<(&'static str, String)>) {
    let site = schema.site_iri.clone().unwrap_or_else(|| "http://www.wikidata.org/entity/".into());
    let cx = Context { site_iri: &site, languages, warnings: RefCell::new(vec![]) };
    let mut edits: Vec<Edit> = vec![];
    let mut index: HashMap<(bool, EntityId), usize> = HashMap::new();
    for &row in rows {
        let row = row as usize;
        for expr in &schema.entity_edits {
            let (subject, terms, groups, media_info) = match expr {
                EntityExpr::Item { subject, name_descs, statement_groups } => (subject, name_descs, statement_groups, false),
                EntityExpr::MediaInfo { subject, name_descs, statement_groups, .. } => (subject, name_descs, statement_groups, true),
            };
            let Some(Val::Entity(id)) = subject.eval(t, row, &cx) else { continue };
            let text = |e: &Option<Expr>| match e.as_ref().and_then(|e| e.eval(t, row, &cx)) { Some(Val::Str(s)) => Some(s), _ => None };
            let (file_name, file_path) = match expr {
                EntityExpr::MediaInfo { file_name, file_path, .. } => (text(file_name), text(file_path)),
                _ => (None, None),
            };
            let mut edit = Edit { subject: id.clone(), media_info, file_name, file_path, terms: vec![], groups: vec![], rows: vec![row] };
            for term in terms {
                if let Some(Val::Mono { text, lang }) = term.value.eval(t, row, &cx) {
                    edit.terms.push((term.name_type, lang, text));
                }
            }
            for g in groups {
                let Some(pid) = g.property.pid() else { continue };
                let statements: Vec<Statement> = g.statements.iter().filter_map(|s| {
                    // deleting every statement of a property has no value
                    let value = match &s.value {
                        Some(v) => Some(v.eval(t, row, &cx)?),
                        None if s.mode == "delete" => None,
                        None => return None,
                    };
                    let references = s.references.iter().map(|r| snaks(&r.snaks, t, row, &cx)).filter(|r| !r.is_empty()).collect();
                    Some(Statement { value, qualifiers: snaks(&s.qualifiers, t, row, &cx), references, mode: s.mode.clone() })
                }).collect();
                if !statements.is_empty() {
                    match edit.groups.iter_mut().find(|(p, _)| p == pid) {
                        Some((_, existing)) => existing.extend(statements),
                        None => edit.groups.push((pid.to_string(), statements)),
                    }
                }
            }
            // an edit with nothing in it changes nothing, but a new entity is still created
            if edit.is_empty() && edit.file_name.is_none() && !matches!(id, EntityId::New(_)) { continue; }
            match index.get(&(media_info, id.clone())) {
                Some(&at) => edits[at].merge(edit),
                None => {
                    index.insert((media_info, id), edits.len());
                    edits.push(edit);
                }
            }
        }
    }
    let warnings = cx.warnings.into_inner();
    (edits, warnings)
}

// ------------------------------------------------------------ issues

/// An issue OpenRefine's edit inspector reports, aggregated over the batch.
#[derive(Clone, Debug, PartialEq)]
pub struct Warning {
    pub kind: &'static str,
    /// some issues are grouped further (OpenRefine's bucketId); the aggregation id is kind_bucket
    pub bucket: Option<String>,
    pub severity: &'static str,
    pub count: usize,
    pub example: String,
}

impl Warning {
    pub fn aggregation_id(&self) -> String {
        match &self.bucket { Some(b) => format!("{}_{b}", self.kind), None => self.kind.to_string() }
    }
}

/// The issues OxRefine checks, in the order it lists them, with OpenRefine's severities.
const ISSUES: &[(&str, &str)] = &[
    ("monolingual-text-without-language", "WARNING"), ("ignored-date", "WARNING"), ("ignored-coordinates", "WARNING"), ("ignored-amount", "WARNING"),
    ("missing-file-name-extension", "CRITICAL"), ("invalid-mediainfo-subject", "CRITICAL"),
    ("new-item-without-labels-or-aliases", "CRITICAL"), ("new-item-without-descriptions", "WARNING"), ("new-media-without-file-path", "CRITICAL"),
    ("unsourced-statements", "WARNING"), ("early-gregorian-date", "WARNING"), ("item-description-begin-with-article", "WARNING"),
    ("new-item-with-deleted-statements", "WARNING"), ("new-item-without-instance-of-or-subclass-of", "WARNING"),
    ("new-item-created", "INFO"), ("new-media-created", "INFO"), ("no-edit-generated", "INFO"),
    // property constraints
    ("add-statements-with-invalid-format", "IMPORTANT"), ("forbidden-value", "IMPORTANT"), ("identical-values-for-distinct-valued-property", "IMPORTANT"),
    ("property-found-in-mainsnak", "IMPORTANT"), ("property-found-in-qualifier", "IMPORTANT"), ("property-found-in-reference", "IMPORTANT"),
    ("values-should-be-integers", "IMPORTANT"), ("bounds-disallowed", "IMPORTANT"), ("no-unit-provided", "IMPORTANT"), ("invalid-unit", "IMPORTANT"),
    ("missing-inverse-statements", "IMPORTANT"), ("single-valued-property-added-more-than-once", "WARNING"), ("having-conflicts-with-statements", "WARNING"),
    ("disallowed-qualifiers", "WARNING"), ("missing-mandatory-qualifiers", "WARNING"), ("invalid-entity-type", "WARNING"),
    ("multi-valued-property-is-required-for-new-item", "WARNING"), ("multi-valued-property-is-required-for-existing-item", "INFO"),
];

/// A property constraint (a P2302 statement): the constraint item and its parameters (qualifiers),
/// with the ids of Wikidata's manifest, which OpenRefine uses for Wikidata.
#[derive(Clone, Debug, Default)]
pub struct Constraint { pub kind: String, pub params: Vec<(String, String)> }

impl Constraint {
    fn values(&self, pid: &str) -> Vec<&str> {
        self.params.iter().filter(|(p, _)| p == pid).map(|(_, v)| v.as_str()).collect()
    }
}

/// The constraints in a property's claims, as wbgetentities gives them.
pub fn constraints_of(claims: &J) -> Vec<Constraint> {
    claims["P2302"].as_array().into_iter().flatten().filter_map(|c| {
        let kind = c["mainsnak"]["datavalue"]["value"]["id"].as_str()?.to_string();
        let params = c["qualifiers"].as_object().into_iter().flatten().flat_map(|(pid, snaks)| snaks.as_array().into_iter().flatten().filter_map(move |s| {
            let v = &s["datavalue"]["value"];
            Some((pid.clone(), v["id"].as_str().or(v.as_str())?.to_string()))
        })).collect();
        Some(Constraint { kind, params })
    }).collect()
}

/// The constraints of the schema's properties on its Wikibase, fetched once per property; none when
/// the Wikibase can't be asked.
pub fn constraint_check(schema: &Schema) -> impl Fn(&str) -> Vec<Constraint> {
    use std::sync::{Mutex, OnceLock};
    static CACHE: OnceLock<Mutex<HashMap<(String, String), Vec<Constraint>>>> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    let api = schema.api.clone().unwrap_or_default();
    let mut pids: Vec<String> = schema.exprs().into_iter().filter_map(|e| e.pid().map(String::from)).collect();
    pids.sort();
    pids.dedup();
    let missing: Vec<String> = { let c = cache.lock().unwrap_or_else(|e| e.into_inner()); pids.into_iter().filter(|p| !c.contains_key(&(api.clone(), p.clone()))).collect() };
    if url::Url::parse(&api).is_ok_and(|u| matches!(u.scheme(), "http" | "https")) {
        for chunk in missing.chunks(50) {
            let ids = chunk.join("|");
            let reply = ureq::get(&api).query_pairs([("action", "wbgetentities"), ("ids", ids.as_str()), ("props", "claims|datatype"), ("format", "json")]).call().ok()
                .and_then(|mut r| serde_json::from_reader::<_, J>(r.body_mut().as_reader()).ok());
            let Some(reply) = reply else { break };
            let mut c = cache.lock().unwrap_or_else(|e| e.into_inner());
            for pid in chunk { c.insert((api.clone(), pid.clone()), constraints_of(&reply["entities"][pid]["claims"])); }
        }
    }
    move |pid: &str| cache.lock().unwrap_or_else(|e| e.into_inner()).get(&(api.clone(), pid.to_string())).cloned().unwrap_or_default()
}

/// The edits of the rows and their issues, as OpenRefine's schema preview reports them.
pub fn inspect(schema: &Schema, t: &Table, rows: &[u32], languages: &dyn Fn(&str) -> bool) -> (Vec<Edit>, Vec<Warning>) {
    inspect_with(schema, t, rows, languages, &constraint_check(schema))
}

/// inspect with the properties' constraints from `constraints`.
pub fn inspect_with(schema: &Schema, t: &Table, rows: &[u32], languages: &dyn Fn(&str) -> bool, constraints: &dyn Fn(&str) -> Vec<Constraint>) -> (Vec<Edit>, Vec<Warning>) {
    let (edits, noted) = evaluate_noting(schema, t, rows, languages);
    let mut found: Vec<Warning> = vec![];
    let mut add = |kind: &'static str, bucket: Option<&str>, example: &str| {
        match found.iter_mut().find(|w| w.kind == kind && w.bucket.as_deref() == bucket) {
            Some(w) => w.count += 1,
            None => {
                let severity = ISSUES.iter().find(|(k, _)| *k == kind).map_or("WARNING", |(_, s)| s);
                found.push(Warning { kind, bucket: bucket.map(String::from), severity, count: 1, example: example.to_string() });
            }
        }
    };
    for (kind, example) in &noted { add(kind, None, example); }
    for e in &edits {
        let new = matches!(e.subject, EntityId::New(_));
        let shown = match &e.subject { EntityId::Existing(id) => id.clone(), EntityId::New(n) => n.to_string() };
        if e.media_info {
            if let EntityId::Existing(id) = &e.subject {
                if !id.starts_with('M') { add("invalid-mediainfo-subject", Some(""), id); continue; }
            }
            if let Some(name) = &e.file_name {
                if !name.rsplit_once('.').is_some_and(|(stem, ext)| !stem.is_empty() && !ext.is_empty()) { add("missing-file-name-extension", None, name); }
            }
        }
        let has = |kinds: &[TermType]| e.terms.iter().any(|(k, _, _)| kinds.contains(k));
        if new && !e.media_info {
            if !has(&[TermType::Label, TermType::LabelIfNew, TermType::Alias]) { add("new-item-without-labels-or-aliases", None, &shown); }
            if !has(&[TermType::Description, TermType::DescriptionIfNew]) { add("new-item-without-descriptions", None, &shown); }
        }
        if new && e.media_info && e.file_path.is_none() { add("new-media-without-file-path", None, &shown); }
        for (_, statements) in &e.groups {
            for s in statements.iter().filter(|s| s.mode != "delete") {
                if s.value.is_some() && s.references.is_empty() { add("unsourced-statements", None, ""); }
                let dates = s.value.iter().chain(s.qualifiers.iter().map(|(_, v)| v)).chain(s.references.iter().flatten().map(|(_, v)| v));
                for v in dates {
                    // the Gregorian calendar, proleptic before 1582
                    if let Val::Date(d) = v {
                        if d.year < 1582 && d.calendar.as_deref().is_none_or(|c| c == "Q1985727") { add("early-gregorian-date", None, &d.year.to_string()); }
                    }
                }
            }
        }
        if e.terms.iter().any(|(k, lang, text)| matches!(k, TermType::Description | TermType::DescriptionIfNew) && lang == "en" && {
            let lower = text.to_lowercase();
            ["a ", "an ", "the "].iter().any(|article| lower.starts_with(article))
        }) {
            add("item-description-begin-with-article", None, &shown);
        }
        if new && !e.media_info {
            if e.groups.iter().any(|(_, s)| s.iter().any(|s| s.mode == "delete")) { add("new-item-with-deleted-statements", None, &shown); }
            let typed = e.groups.iter().any(|(p, s)| (p == "P31" || p == "P279") && s.iter().any(|s| s.mode != "delete"));
            if !typed { add("new-item-without-instance-of-or-subclass-of", None, &shown); }
            add("new-item-created", None, "");
        }
        if new && e.media_info { add("new-media-created", None, ""); }
        // property constraints, per statement and per entity
        let subject = match &e.subject { EntityId::Existing(id) => Some(id.as_str()), _ => None };
        let applies = |c: &Constraint| !subject.is_some_and(|id| c.values("P2303").contains(&id));
        for (pid, statements) in &e.groups {
            let cs: Vec<Constraint> = constraints(pid).into_iter().filter(applies).collect();
            let live: Vec<&Statement> = statements.iter().filter(|s| s.mode != "delete" && s.value.is_some()).collect();
            if live.is_empty() { continue; }
            for c in &cs {
                match c.kind.as_str() {
                    "Q19474404" if live.len() > 1 => add("single-valued-property-added-more-than-once", Some(pid), &shown),
                    "Q21510857" if live.len() == 1 => add(if new { "multi-valued-property-is-required-for-new-item" } else { "multi-valued-property-is-required-for-existing-item" }, Some(pid), &shown),
                    "Q21502838" => for other in c.values("P2306") {
                        if e.groups.iter().any(|(p, s)| p == other && s.iter().any(|s| s.mode != "delete" && s.value.is_some())) { add("having-conflicts-with-statements", Some(&format!("{pid}{other}")), &shown); }
                    },
                    "Q52004125" if !c.values("P2305").contains(&if e.media_info { "Q59712033" } else { "Q29934200" }) => add("invalid-entity-type", None, &shown),
                    _ => {}
                }
            }
            for s in &live {
                let v = s.value.as_ref().unwrap();
                for c in &cs {
                    match (c.kind.as_str(), v) {
                        ("Q21502404", _) => {
                            let text = match v { Val::Str(t) | Val::Mono { text: t, .. } => Some(t.as_str()), Val::Entity(EntityId::Existing(id)) => Some(id.as_str()), _ => None };
                            for pattern in c.values("P1793") {
                                let whole = regex::Regex::new(&format!("^(?:{pattern})$"));
                                if let (Some(text), Ok(re)) = (text, whole) { if !re.is_match(text) { add("add-statements-with-invalid-format", Some(pid), text); } }
                            }
                        }
                        ("Q21510859", Val::Entity(EntityId::Existing(id))) if !c.values("P2305").contains(&id.as_str()) => add("forbidden-value", Some(pid), id),
                        ("Q52558054", Val::Entity(EntityId::Existing(id))) if c.values("P2305").contains(&id.as_str()) => add("forbidden-value", Some(pid), id),
                        ("Q52848401", Val::Quantity { amount, .. }) if !amount.is_integer() => add("values-should-be-integers", Some(pid), &amount.plain()),
                        ("Q51723761", Val::Quantity { amount, bounds: Some(_), .. }) => add("bounds-disallowed", Some(pid), &amount.plain()),
                        ("Q21514353", Val::Quantity { amount, unit, .. }) => match unit {
                            None => add("no-unit-provided", Some(pid), &amount.plain()),
                            Some(u) if !c.values("P2305").contains(&u.as_str()) => add("invalid-unit", Some(pid), u),
                            _ => {}
                        },
                        ("Q21510851", _) => for (q, _) in &s.qualifiers {
                            if !c.values("P2306").contains(&q.as_str()) { add("disallowed-qualifiers", Some(&format!("{pid}-{q}")), &shown); }
                        },
                        ("Q21510856", _) => for m in c.values("P2306") {
                            if !s.qualifiers.iter().any(|(q, _)| q == m) { add("missing-mandatory-qualifiers", Some(&format!("{pid}-{m}")), &shown); }
                        },
                        ("Q53869507", _) if !c.values("P5314").contains(&"Q54828448") => add("property-found-in-mainsnak", Some(pid), pid),
                        _ => {}
                    }
                }
                // where the properties of qualifiers and references may be used
                for (place, kind, snaks) in [("Q54828449", "property-found-in-qualifier", s.qualifiers.iter().collect::<Vec<_>>()), ("Q54828450", "property-found-in-reference", s.references.iter().flatten().collect())] {
                    for (q, _) in snaks {
                        if constraints(q).iter().any(|c| c.kind == "Q53869507" && !c.values("P5314").contains(&place)) { add(kind, Some(q), q); }
                    }
                }
            }
        }
    }
    // across the batch: distinct values, and inverse statements
    let mut first: HashMap<(String, String), EntityId> = HashMap::new();
    for e in &edits {
        for (pid, statements) in &e.groups {
            let cs = constraints(pid);
            for s in statements.iter().filter(|s| s.mode != "delete") {
                let Some(v) = &s.value else { continue };
                if cs.iter().any(|c| c.kind == "Q21502410") {
                    match first.get(&(pid.clone(), format!("{v:?}"))) {
                        Some(other) if *other != e.subject => add("identical-values-for-distinct-valued-property", Some(pid), ""),
                        Some(_) => {}
                        None => { first.insert((pid.clone(), format!("{v:?}")), e.subject.clone()); }
                    }
                }
                let Val::Entity(target) = v else { continue };
                for c in cs.iter().filter(|c| c.kind == "Q21510855" || c.kind == "Q21510862") {
                    let inverse = if c.kind == "Q21510862" { vec![pid.as_str()] } else { c.values("P2306") };
                    let back = edits.iter().filter(|t| t.subject == *target).any(|t| t.groups.iter().any(|(p, ss)| inverse.contains(&p.as_str()) && ss.iter().any(|x| x.value == Some(Val::Entity(e.subject.clone())))));
                    if !back { add("missing-inverse-statements", Some(pid), ""); }
                }
            }
        }
    }
    if edits.is_empty() { add("no-edit-generated", None, ""); }
    found.sort_by_key(|w| ISSUES.iter().position(|(k, _)| *k == w.kind).unwrap_or(usize::MAX));
    (edits, found)
}

thread_local! {
    /// the schema of the project whose expressions are being evaluated (wikibaseIssues())
    static SCHEMA: RefCell<Option<J>> = const { RefCell::new(None) };
}

/// Make a project's schema the one wikibaseIssues() sees until the guard is dropped.
pub struct SchemaGuard(Option<J>);
impl SchemaGuard {
    pub fn set(schema: Option<J>) -> SchemaGuard {
        SchemaGuard(SCHEMA.with(|s| std::mem::replace(&mut *s.borrow_mut(), schema)))
    }
}
impl Drop for SchemaGuard {
    fn drop(&mut self) {
        let previous = self.0.take();
        SCHEMA.with(|s| *s.borrow_mut() = previous);
    }
}

/// The schema of the project being worked on (see SchemaGuard).
pub fn current_schema() -> Option<J> {
    SCHEMA.with(|s| s.borrow().clone())
}

/// GREL's wikibaseIssues(): the issues of this row's edits alone, as aggregation ids.
pub fn row_issues(t: &Table, row: usize) -> Value {
    let Some(schema) = SCHEMA.with(|s| s.borrow().clone()) else { return Value::err("No wikibase schema associated with this project") };
    let schema = match Schema::parse(&schema) { Ok(s) => s, Err(e) => return Value::err(e) };
    let (_, warnings) = inspect(&schema, t, &[row as u32], &language_check(&schema));
    let ids: Vec<Value> = if warnings.is_empty() { vec![Value::Str("no-issue-detected".into())] } else { warnings.iter().map(|w| Value::Str(w.aggregation_id().into())).collect() };
    Value::Array(ids.into())
}

/// What the schema editor shows for a schema: the edits as QuickStatements (or why they can't
/// be), how many entities change and are created, and the issues.
pub fn preview(schema: &Schema, t: &Table, rows: &[u32]) -> Result<J> {
    let languages = language_check(schema);
    let (edits, warnings) = inspect(schema, t, rows, &languages);
    let quickstatements = quickstatements(schema, t, rows, &languages);
    Ok(json!({
        "edits": edits.len(),
        "newEntities": edits.iter().filter(|e| matches!(e.subject, EntityId::New(_))).count(),
        "quickstatements": quickstatements.as_ref().ok(),
        "quickstatementsError": quickstatements.err(),
        "warnings": warnings.iter().map(|w| json!({ "type": w.kind, "aggregationId": w.aggregation_id(), "severity": w.severity, "count": w.count, "example": w.example })).collect::<Vec<_>>(),
    }))
}

/// wbsearchentities on a Wikibase: properties (with their datatype) or items matching `text`.
pub fn search(api: &str, kind: &str, text: &str) -> Result<J> {
    let url = url::Url::parse(api.trim()).map_err(|e| format!("{api}: {e}"))?;
    if !matches!(url.scheme(), "http" | "https") { return Err(format!("{api}: not a web address")); }
    let kind = if kind == "property" { "property" } else { "item" };
    let mut res = ureq::get(url.as_str())
        .query_pairs([("action", "wbsearchentities"), ("format", "json"), ("language", "en"), ("uselang", "en"), ("type", kind), ("limit", "10"), ("search", text)])
        .call().map_err(|e| format!("{api}: {e}"))?;
    let body: J = serde_json::from_reader(res.body_mut().as_reader()).map_err(|e| format!("{api}: {e}"))?;
    Ok(J::Array(body["search"].as_array().map(|items| items.iter().map(|i| json!({
        "id": i["id"], "label": i["label"], "description": i["description"], "datatype": i["datatype"],
    })).collect()).unwrap_or_default()))
}

// ------------------------------------------------------------ QuickStatements

fn qs_date(d: &Date) -> String {
    // OpenRefine prints year -44 as "+-044"; QuickStatements reads "-0044"
    let year = if d.year < 0 { format!("-{:04}", -d.year) } else { format!("+{:04}", d.year) };
    format!("{year}-{:02}-{:02}T00:00:00Z/{}", d.month, d.day, d.precision)
}

fn qs_value(v: &Val) -> Option<String> {
    Some(match v {
        Val::Entity(EntityId::Existing(id)) => id.clone(),
        // a new item made elsewhere in the batch can't be named on a QuickStatements line
        Val::Entity(EntityId::New(_)) => return None,
        Val::Str(s) => format!("\"{s}\""),
        Val::Mono { text, lang } => format!("{lang}:\"{text}\""),
        Val::Date(d) => qs_date(d),
        Val::Location(lat, lng) => format!("@{lat:.6}/{lng:.6}"),
        Val::Quantity { amount, bounds, unit } => {
            let mut out = amount.plain();
            if let Some((lower, upper)) = bounds { out += &format!("[{},{}]", lower.plain(), upper.plain()); }
            if let Some(u) = unit { out += &format!("U{}", u.trim_start_matches('Q')); }
            out
        }
    })
}

/// The QuickStatements (v1) export of the rows, as OpenRefine's exporter writes it.
pub fn quickstatements(schema: &Schema, t: &Table, rows: &[u32], languages: &dyn Fn(&str) -> bool) -> Result<String> {
    let edits = evaluate(schema, t, rows, languages);
    if edits.iter().any(|e| e.media_info) {
        return Err("QuickStatements can't express media info edits; export the Wikibase schema or upload the edits instead.".into());
    }
    let mut out = String::new();
    for e in &edits {
        let subject = match &e.subject {
            EntityId::Existing(id) => id.clone(),
            EntityId::New(_) => { out += "CREATE\n"; "LAST".into() }
        };
        // overriding terms first; a term "if new" gives way to an overriding one in its language
        for (overriding, if_new, letter) in [(TermType::Label, TermType::LabelIfNew, 'L'), (TermType::Description, TermType::DescriptionIfNew, 'D')] {
            let mut seen = HashSet::new();
            for kind in [overriding, if_new] {
                for (_, lang, text) in e.terms.iter().filter(|(k, _, _)| *k == kind) {
                    if seen.insert(lang.clone()) { out += &format!("{subject}\t{letter}{lang}\t\"{text}\"\n"); }
                }
            }
        }
        for (_, lang, text) in e.terms.iter().filter(|(k, _, _)| *k == TermType::Alias) {
            out += &format!("{subject}\tA{lang}\t\"{text}\"\n");
        }
        for (pid, statements) in &e.groups {
            for s in statements {
                let value = match &s.value { Some(v) => match qs_value(v) { Some(v) => v, None => continue }, None => "novalue".into() };
                // only "delete" removes; OpenRefine also writes "add" statements as removals
                let prefix = if s.mode == "delete" { "- " } else { "" };
                let qualifiers: String = s.qualifiers.iter().filter_map(|(p, v)| Some(format!("\t{p}\t{}", qs_value(v)?))).collect();
                let line = format!("{prefix}{subject}\t{pid}\t{value}{qualifiers}");
                if s.references.is_empty() {
                    out += &line;
                    out.push('\n');
                }
                for r in &s.references {
                    let sources: String = r.iter().filter_map(|(p, v)| Some(format!("\tS{}\t{}", p.trim_start_matches('P'), qs_value(v)?))).collect();
                    out += &format!("{line}{sources}\n");
                }
            }
        }
    }
    Ok(out)
}

/// The language codes a Wikibase accepts for monolingual text and terms (its API's
/// wbcontentlanguages), fetched once per API; None when it can't be asked.
pub fn content_languages(api: &str) -> Option<HashSet<String>> {
    use std::sync::{Mutex, OnceLock};
    static CACHE: OnceLock<Mutex<HashMap<String, HashSet<String>>>> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    if let Some(found) = cache.lock().unwrap_or_else(|e| e.into_inner()).get(api) { return Some(found.clone()); }
    let url = format!("{api}?action=query&meta=wbcontentlanguages&wbclprop=code&wbclcontext=monolingualtext&format=json");
    let mut res = ureq::get(&url).call().ok()?;
    let body: J = serde_json::from_reader(res.body_mut().as_reader()).ok()?;
    let codes: HashSet<String> = body["query"]["wbcontentlanguages"].as_object()?.keys().cloned().collect();
    cache.lock().unwrap_or_else(|e| e.into_inner()).insert(api.to_string(), codes.clone());
    Some(codes)
}

/// The language check for a schema: its Wikibase's languages, or (offline) any well-formed code.
pub fn language_check(schema: &Schema) -> impl Fn(&str) -> bool {
    let known = schema.api.as_deref().and_then(content_languages);
    move |code: &str| match &known {
        Some(codes) => codes.contains(code),
        None => !code.is_empty() && code.split('-').all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())),
    }
}
