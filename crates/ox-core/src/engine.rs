//! Facets and row filtering with OpenRefine's `engineConfig` JSON: list facets on any GREL expression,
//! text facets, numeric range facets, rows and records modes, and facetCount().
//! Behaviour is pinned to OpenRefine 3.10.1 by tests/facets_conformance.rs.
use crate::{
    err,
    grel::{self, value::fmt_date, value::render_num, Value},
    table::Table,
    Result,
};
use regex::RegexBuilder;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value as J};
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
};

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct EngineConfig {
    pub facets: Vec<Facet>,
    pub mode: String,
}

impl Default for EngineConfig {
    fn default() -> Self {
        EngineConfig { facets: vec![], mode: "row-based".into() }
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Facet {
    #[serde(rename = "list", rename_all = "camelCase")]
    List {
        #[serde(default)]
        name: String,
        column_name: String,
        #[serde(default = "value")]
        expression: String,
        #[serde(default)]
        selection: Vec<Choice>,
        #[serde(default)]
        select_blank: bool,
        #[serde(default)]
        select_error: bool,
        #[serde(default)]
        invert: bool,
    },
    #[serde(rename = "text", rename_all = "camelCase")]
    Text {
        #[serde(default)]
        name: String,
        column_name: String,
        #[serde(default)]
        query: String,
        #[serde(default = "text")]
        mode: String,
        #[serde(default)]
        case_sensitive: bool,
        #[serde(default)]
        invert: bool,
    },
    #[serde(rename = "range", rename_all = "camelCase")]
    Range {
        #[serde(default)]
        name: String,
        column_name: String,
        #[serde(default = "value")]
        expression: String,
        from: f64,
        to: f64,
        #[serde(default = "yes")]
        select_numeric: bool,
        #[serde(default = "yes")]
        select_non_numeric: bool,
        #[serde(default = "yes")]
        select_blank: bool,
        #[serde(default = "yes")]
        select_error: bool,
    },
    /// OpenRefine's timeline facet: dates between `from` and `to` (milliseconds since 1970, both included).
    #[serde(rename = "timerange", rename_all = "camelCase")]
    TimeRange {
        #[serde(default)]
        name: String,
        column_name: String,
        #[serde(default = "value")]
        expression: String,
        from: f64,
        to: f64,
        #[serde(default = "yes")]
        select_time: bool,
        #[serde(default = "yes")]
        select_non_time: bool,
        #[serde(default = "yes")]
        select_blank: bool,
        #[serde(default = "yes")]
        select_error: bool,
    },
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Choice {
    pub v: ChoiceValue,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ChoiceValue {
    pub v: J,
    #[serde(default)]
    pub l: String,
}

pub(crate) fn value() -> String {
    "value".into()
}
fn text() -> String {
    "text".into()
}
fn yes() -> bool {
    true
}

// ---------------------------------------------------------------- what an expression yields per row

/// One facet item: a value (by its text key), a blank or an error. Arrays give several items.
#[derive(Clone, PartialEq)]
enum Item {
    Blank,
    Error,
    Val { key: String, json: J, num: Option<f64>, time: Option<i64> },
}

/// The text a choice is known by: what OpenRefine shows for it.
fn key_of(v: &Value) -> String {
    match v {
        Value::Str(s) => s.clone(),
        Value::Int(i) => i.to_string(),
        Value::Num(n) => render_num(*n),
        Value::Date(d) => fmt_date(d),
        v => v.display(),
    }
}

fn json_key(j: &J) -> String {
    match j {
        J::String(s) => s.clone(),
        J::Number(n) => n.as_i64().map_or_else(|| render_num(n.as_f64().unwrap_or(f64::NAN)), |i| i.to_string()),
        J::Bool(b) => b.to_string(),
        J::Null => String::new(),
        other => other.to_string(),
    }
}

fn items_of(v: Value) -> Vec<Item> {
    let one = |v: Value| -> Item {
        match v {
            Value::Error(_) => Item::Error,
            v if v.is_blank() => Item::Blank,
            v => {
                let num = match &v {
                    Value::Int(_) | Value::Num(_) => v.as_f64(),
                    Value::Json(j) if j.is_number() => j.as_f64(),
                    _ => None,
                };
                let json = match &v {
                    Value::Str(s) => json!(s),
                    Value::Int(i) => json!(i),
                    Value::Num(n) => json!(n),
                    Value::Bool(b) => json!(b),
                    Value::Json(j) => (**j).clone(),
                    v => json!(key_of(v)),
                };
                let time = match &v {
                    Value::Date(d) => Some(d.and_utc().timestamp_millis()),
                    _ => None,
                };
                Item::Val { key: key_of(&v), json, num, time }
            }
        }
    };
    match v {
        Value::Array(a) => dedupe(a.into_iter().map(one)),
        Value::Json(j) if j.is_array() => dedupe(j.as_array().unwrap().iter().cloned().map(|x| one(grel::value::from_json(x)))),
        v => vec![one(v)],
    }
}

/// A row counts once per distinct item, however often it repeats in an array.
fn dedupe(items: impl Iterator<Item = Item>) -> Vec<Item> {
    let mut out: Vec<Item> = vec![];
    for i in items {
        if !out.contains(&i) {
            out.push(i);
        }
    }
    out
}

/// Per-request state: facetCount() results, computed once per (expression, column).
#[derive(Default)]
pub struct Ctx {
    counts: RefCell<HashMap<(String, String), std::result::Result<HashMap<String, i64>, String>>>,
    /// the first row of each record, for `row.record`
    record_starts: std::cell::OnceCell<Vec<usize>>,
}

const BLANK_KEY: &str = "\u{0}blank";

impl Ctx {
    /// The first row of each record in `t`, computed once per request.
    pub fn record_starts(&self, t: &Table) -> &[usize] {
        self.record_starts.get_or_init(|| record_starts(t))
    }

    /// facetCount(choice, expression, column): rows of `column` whose expression gives `choice`.
    pub fn facet_count(&self, t: &Table, choice: &Value, expr: &str, column: &str) -> Value {
        let k = (expr.to_string(), column.to_string());
        if !self.counts.borrow().contains_key(&k) {
            let computed = (|| -> std::result::Result<HashMap<String, i64>, String> {
                let ev = Evaluated::new(t, column, expr, None)?;
                let mut m: HashMap<String, i64> = HashMap::new();
                for r in 0..t.rows {
                    for it in ev.of(t, r) {
                        match it {
                            Item::Val { key, .. } => *m.entry(key.clone()).or_default() += 1,
                            Item::Blank => *m.entry(BLANK_KEY.into()).or_default() += 1,
                            Item::Error => {}
                        }
                    }
                }
                Ok(m)
            })();
            self.counts.borrow_mut().insert(k.clone(), computed);
        }
        let counts = self.counts.borrow();
        match &counts[&k] {
            Err(e) => Value::err(e.clone()),
            Ok(m) => {
                let key = match items_of(choice.clone()).first() {
                    Some(Item::Val { key, .. }) => key.clone(),
                    _ => BLANK_KEY.into(),
                };
                Value::Int(m.get(&key).copied().unwrap_or(0))
            }
        }
    }
}

/// A facet expression evaluated over a column: per distinct value when it only reads `value`, else per row.
struct Evaluated {
    col: usize,
    per_code: bool,
    items: Vec<Vec<Item>>,
}

impl Evaluated {
    fn new(t: &Table, column: &str, expr: &str, ctx: Option<&Ctx>) -> Result<Evaluated> {
        let col = t.col(column)?;
        let c = &t.columns[col];
        if expr.trim() == "value" || expr.trim() == "grel:value" {
            return Ok(Evaluated { col, per_code: true, items: c.dict.iter().cloned().map(items_of).collect() });
        }
        let p = grel::compile(expr)?;
        if !p.uses_row_of(c) {
            return Ok(Evaluated { col, per_code: true, items: c.dict.iter().map(|v| items_of(p.eval(v.clone(), &grel::NoRow))).collect() });
        }
        let owned = Ctx::default();
        let ctx = ctx.unwrap_or(&owned);
        let items = (0..t.rows).map(|r| items_of(p.eval(c.get(r).clone(), &t.row_at(r, ctx, col)))).collect();
        Ok(Evaluated { col, per_code: false, items })
    }
    fn of<'a>(&'a self, t: &Table, r: usize) -> &'a [Item] {
        if self.per_code { &self.items[t.columns[self.col].codes[r] as usize] } else { &self.items[r] }
    }
}

// ---------------------------------------------------------------- filtering

/// Pass flag per dict code (cheap) or per row.
enum Pass {
    Code(usize, Vec<bool>),
    Row(Vec<bool>),
}

impl Pass {
    fn ok(&self, t: &Table, r: usize) -> bool {
        match self {
            Pass::Code(c, ok) => ok[t.columns[*c].codes[r] as usize],
            Pass::Row(ok) => ok[r],
        }
    }
    fn from_items(t: &Table, ev: &Evaluated, f: impl Fn(&[Item]) -> bool) -> Pass {
        if ev.per_code {
            Pass::Code(ev.col, ev.items.iter().map(|i| f(i)).collect())
        } else {
            Pass::Row((0..t.rows).map(|r| f(ev.of(t, r))).collect())
        }
    }
}

fn range_ok(it: &Item, from: f64, to: f64, flags: [bool; 4]) -> bool {
    let [numeric, non_numeric, blank, error] = flags;
    match it {
        Item::Val { num: Some(n), .. } => numeric && *n >= from && *n < to,
        Item::Val { .. } => non_numeric,
        Item::Blank => blank,
        Item::Error => error,
    }
}

fn time_ok(it: &Item, from: f64, to: f64, flags: [bool; 4]) -> bool {
    let [time, non_time, blank, error] = flags;
    match it {
        Item::Val { time: Some(t), .. } => time && (*t as f64) >= from && (*t as f64) <= to,
        Item::Val { .. } => non_time,
        Item::Blank => blank,
        Item::Error => error,
    }
}

/// The facet as a row filter, or None when it lets every row through.
fn compile(t: &Table, f: &Facet, ctx: &Ctx) -> Result<Option<Pass>> {
    match f {
        Facet::List { column_name, expression, selection, select_blank, select_error, invert, .. } => {
            if selection.is_empty() && !select_blank && !select_error {
                return Ok(None);
            }
            let ev = Evaluated::new(t, column_name, expression, Some(ctx))?;
            let set: HashSet<String> = selection.iter().map(|c| json_key(&c.v.v)).collect();
            Ok(Some(Pass::from_items(t, &ev, |items| {
                items.iter().any(|i| match i {
                    Item::Val { key, .. } => set.contains(key),
                    Item::Blank => *select_blank,
                    Item::Error => *select_error,
                }) != *invert
            })))
        }
        Facet::Text { column_name, query, mode, case_sensitive, invert, .. } => {
            if query.is_empty() {
                return Ok(None);
            }
            let ci = t.col(column_name)?;
            let re = if mode == "regex" { grel::parse::user_regex(query, !case_sensitive) } else { RegexBuilder::new(&regex::escape(query)).case_insensitive(!case_sensitive).build() };
            let re = re.map_err(err)?;
            Ok(Some(Pass::Code(ci, t.columns[ci].dict.iter().map(|v| re.is_match(&v.display()) != *invert).collect())))
        }
        Facet::Range { column_name, expression, from, to, select_numeric, select_non_numeric, select_blank, select_error, .. } => {
            let ev = Evaluated::new(t, column_name, expression, Some(ctx))?;
            // with no numbers at all OpenRefine reports an error and the facet filters nothing
            if !(0..t.rows).any(|r| ev.of(t, r).iter().any(|i| matches!(i, Item::Val { num: Some(_), .. }))) {
                return Ok(None);
            }
            let flags = [*select_numeric, *select_non_numeric, *select_blank, *select_error];
            Ok(Some(Pass::from_items(t, &ev, |items| items.iter().any(|i| range_ok(i, *from, *to, flags)))))
        }
        Facet::TimeRange { column_name, expression, from, to, select_time, select_non_time, select_blank, select_error, .. } => {
            let ev = Evaluated::new(t, column_name, expression, Some(ctx))?;
            // with no dates at all the facet filters nothing, as in OpenRefine
            if !(0..t.rows).any(|r| ev.of(t, r).iter().any(|i| matches!(i, Item::Val { time: Some(_), .. }))) {
                return Ok(None);
            }
            let flags = [*select_time, *select_non_time, *select_blank, *select_error];
            Ok(Some(Pass::from_items(t, &ev, |items| items.iter().any(|i| time_ok(i, *from, *to, flags)))))
        }
    }
}

/// Row indices passing every facet except `skip` (each facet's counts ignore the facet itself).
/// In records mode a record passes when each facet matches at least one of its rows, and then all its rows pass.
pub fn filter(t: &Table, cfg: &EngineConfig, skip: Option<usize>) -> Result<Vec<u32>> {
    filter_with(t, cfg, skip, &Ctx::default())
}

fn filter_with(t: &Table, cfg: &EngineConfig, skip: Option<usize>, ctx: &Ctx) -> Result<Vec<u32>> {
    let mut active = vec![];
    for (i, f) in cfg.facets.iter().enumerate() {
        if Some(i) != skip {
            active.extend(compile(t, f, ctx)?);
        }
    }
    if cfg.mode != "record-based" {
        return Ok((0..t.rows).filter(|&r| active.iter().all(|p| p.ok(t, r))).map(|r| r as u32).collect());
    }
    let mut out = vec![];
    for (a, b) in records(t) {
        if active.iter().all(|p| (a..b).any(|k| p.ok(t, k))) {
            out.extend(a as u32..b as u32);
        }
    }
    Ok(out)
}

/// The first row of each record, by the table's key column.
pub fn record_starts(t: &Table) -> Vec<usize> {
    let starts = t.record_starts(t.key_index());
    (0..t.rows).filter(|&r| starts[r]).collect()
}

/// Row ranges of the records, by the table's key column.
fn records(t: &Table) -> Vec<(usize, usize)> {
    let s = record_starts(t);
    s.iter().enumerate().map(|(i, &a)| (a, s.get(i + 1).copied().unwrap_or(t.rows))).collect()
}

/// Row mask that operations apply to; None when nothing is filtered out.
pub fn mask(t: &Table, cfg: &EngineConfig) -> Result<Option<Vec<bool>>> {
    let rows = filter(t, cfg, None)?;
    if rows.len() == t.rows {
        return Ok(None);
    }
    let mut m = vec![false; t.rows];
    for r in rows {
        m[r as usize] = true;
    }
    Ok(Some(m))
}

/// Distinct values of a column over `rows` by their text (null and "" count as one blank), most frequent first.
pub fn value_counts(t: &Table, ci: usize, rows: &[u32]) -> Vec<(String, u32)> {
    let col = &*t.columns[ci];
    let mut n = vec![0u32; col.dict.len()];
    for &r in rows {
        n[col.codes[r as usize] as usize] += 1;
    }
    let mut by_text: HashMap<String, u32> = HashMap::new();
    for (i, &c) in n.iter().enumerate().filter(|(_, &c)| c > 0) {
        *by_text.entry(col.dict[i].display()).or_default() += c;
    }
    let mut v: Vec<_> = by_text.into_iter().collect();
    v.sort_unstable_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    v
}

// ---------------------------------------------------------------- facet panels

/// OpenRefine's NumericBinIndex: a power-of-ten step (doubled past 100 bins), the top pushed one step out.
fn binning(lo: f64, hi: f64) -> (f64, f64, f64, usize) {
    if lo >= hi {
        let min = lo.min(hi);
        return (min, min + 1.0, 1.0, 1);
    }
    let diff = hi - lo;
    let mut step = 1.0;
    if diff > 10.0 {
        while step * 100.0 < diff {
            step *= 10.0;
        }
    } else {
        while step * 100.0 > diff {
            step /= 10.0;
        }
    }
    let min = (lo / step).floor() * step;
    let mut max = (hi / step).ceil() * step;
    let mut count = (max - min) / step;
    if count > 100.0 {
        step *= 2.0;
        count = (count + 1.0) / 2.0;
    }
    if max <= hi {
        max += step;
        count += 1.0;
    }
    (min, max, step, (count + 0.5).floor() as usize)
}

#[derive(Default)]
struct RangeStats {
    numeric: usize,
    non_numeric: usize,
    blank: usize,
    error: usize,
}

fn range_panel(t: &Table, ev: &Evaluated, others: &[u32]) -> J {
    let tally = |rows: &mut dyn Iterator<Item = usize>| {
        let (mut s, mut nums) = (RangeStats::default(), vec![]);
        for r in rows {
            for it in ev.of(t, r) {
                match it {
                    Item::Val { num: Some(n), .. } => {
                        s.numeric += 1;
                        nums.push(*n);
                    }
                    Item::Val { .. } => s.non_numeric += 1,
                    Item::Blank => s.blank += 1,
                    Item::Error => s.error += 1,
                }
            }
        }
        (s, nums)
    };
    let (base, base_nums) = tally(&mut (0..t.rows));
    let (cur, cur_nums) = tally(&mut others.iter().map(|&r| r as usize));
    let mut out = json!({
        "baseNumericCount": base.numeric, "baseNonNumericCount": base.non_numeric, "baseBlankCount": base.blank, "baseErrorCount": base.error,
        "numericCount": cur.numeric, "nonNumericCount": cur.non_numeric, "blankCount": cur.blank, "errorCount": cur.error,
    });
    if base_nums.is_empty() {
        out["error"] = json!("No numeric value present.");
        return out;
    }
    let lo = base_nums.iter().copied().fold(f64::INFINITY, f64::min);
    let hi = base_nums.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let (min, max, step, n) = binning(lo, hi);
    let bins = |nums: &[f64]| {
        let mut b = vec![0usize; n];
        for v in nums {
            let i = ((v - min) / step).floor() as usize;
            b[i.min(n - 1)] += 1;
        }
        b
    };
    out["min"] = json!(min);
    out["max"] = json!(max);
    out["step"] = json!(step);
    out["baseBins"] = json!(bins(&base_nums));
    out["bins"] = json!(bins(&cur_nums));
    out
}

/// OpenRefine's TimeBinIndex steps: millisecond … week, then average Gregorian months and years.
const TIME_STEPS: [i64; 11] = [1, 1_000, 60_000, 3_600_000, 86_400_000, 604_800_000, 2_629_746_000, 31_556_952_000, 315_569_520_000, 3_155_695_200_000, 31_556_952_000_000];

/// OpenRefine's timeline panel. Its quirks are kept: with fewer than two distinct dates the step is 1,
/// `max` is 1 and the base histogram stays empty; `from` and `to` come back clamped to min and max.
fn time_panel(t: &Table, ev: &Evaluated, others: &[u32], from: f64, to: f64) -> J {
    let tally = |rows: &mut dyn Iterator<Item = usize>| {
        let (mut s, mut times) = (RangeStats::default(), vec![]);
        for r in rows {
            for it in ev.of(t, r) {
                match it {
                    Item::Val { time: Some(x), .. } => {
                        s.numeric += 1;
                        times.push(*x);
                    }
                    Item::Val { .. } => s.non_numeric += 1,
                    Item::Blank => s.blank += 1,
                    Item::Error => s.error += 1,
                }
            }
        }
        (s, times)
    };
    let (base, base_times) = tally(&mut (0..t.rows));
    let (cur, cur_times) = tally(&mut others.iter().map(|&r| r as usize));
    let (mut min, mut max) = (base_times.iter().copied().min().unwrap_or(i64::MAX), base_times.iter().copied().max().unwrap_or(i64::MIN));
    let (step, n, fill_base) = if min >= max {
        min = min.min(max);
        max = 1;
        (1, 1, false)
    } else {
        let diff = max - min;
        let step = TIME_STEPS.iter().copied().find(|s| diff / s <= 100).unwrap_or(TIME_STEPS[10]);
        (step, (diff / step) as usize + 1, true)
    };
    let bins = |times: &[i64]| {
        let mut b = vec![0usize; n];
        for &x in times {
            let i = ((x.saturating_sub(min)) / step).max(0) as usize;
            b[i.min(n - 1)] += 1;
        }
        b
    };
    json!({
        "baseTimeCount": base.numeric, "baseNonTimeCount": base.non_numeric, "baseBlankCount": base.blank, "baseErrorCount": base.error,
        "timeCount": cur.numeric, "nonTimeCount": cur.non_numeric, "blankCount": cur.blank, "errorCount": cur.error,
        "min": min, "max": max, "step": step,
        "from": from.max(min as f64), "to": to.min(max as f64),
        "baseBins": if fill_base { bins(&base_times) } else { vec![0; n] },
        "bins": bins(&cur_times),
    })
}

/// Choice counts: each item `n` times.
#[derive(Default)]
struct Tally<'a> {
    counts: HashMap<&'a str, (usize, &'a J)>,
    blank: usize,
    error: usize,
}

impl<'a> Tally<'a> {
    fn add(&mut self, items: impl IntoIterator<Item = &'a Item>, n: usize) {
        for it in items {
            match it {
                Item::Val { key, json, .. } => self.counts.entry(key.as_str()).or_insert((0, json)).0 += n,
                Item::Blank => self.blank += n,
                Item::Error => self.error += n,
            }
        }
    }
}

fn list_panel(t: &Table, ev: &Evaluated, others: &[u32], selection: &[Choice]) -> J {
    // counts per choice over the rows that pass the other facets; in records mode that is every row of
    // the passing records, and each row still counts once, as in OpenRefine
    let mut tally = Tally::default();
    if ev.per_code {
        // rows by dictionary code first: one pass over the codes, then each distinct value once
        let codes = &t.columns[ev.col].codes;
        let mut n = vec![0usize; ev.items.len()];
        for &r in others {
            n[codes[r as usize] as usize] += 1;
        }
        for (code, &k) in n.iter().enumerate().filter(|(_, k)| **k > 0) {
            tally.add(&ev.items[code], k);
        }
    } else {
        for &r in others {
            tally.add(ev.of(t, r as usize), 1);
        }
    }
    let (blank, error) = (tally.blank, tally.error);
    let mut counts: HashMap<String, (usize, J)> = tally.counts.into_iter().map(|(k, (c, j))| (k.to_string(), (c, j.clone()))).collect();
    // selected choices stay listed even when nothing matches them now
    for c in selection {
        counts.entry(json_key(&c.v.v)).or_insert((0, c.v.v.clone()));
    }
    let mut choices: Vec<(String, usize, J)> = counts.into_iter().map(|(k, (c, j))| (k, c, j)).collect();
    choices.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    json!({
        "distinct": choices.len(),
        "choices": choices.iter().take(2000).map(|(k, c, j)| json!([k, c, j])).collect::<Vec<_>>(),
        "blank": blank,
        "error": error,
    })
}

/// The panel data for each facet: choices and counts for list facets, histogram for range facets,
/// null for text facets. Each facet is counted over the rows the other facets let through.
pub fn facet_counts(t: &Table, cfg: &EngineConfig) -> Result<Vec<J>> {
    let ctx = Ctx::default();
    cfg.facets
        .iter()
        .enumerate()
        .map(|(i, f)| match f {
            Facet::List { column_name, expression, selection, .. } => {
                let others = filter_with(t, cfg, Some(i), &ctx)?;
                let ev = Evaluated::new(t, column_name, expression, Some(&ctx))?;
                Ok(list_panel(t, &ev, &others, selection))
            }
            Facet::Range { column_name, expression, .. } => {
                let others = filter_with(t, cfg, Some(i), &ctx)?;
                let ev = Evaluated::new(t, column_name, expression, Some(&ctx))?;
                Ok(range_panel(t, &ev, &others))
            }
            Facet::TimeRange { column_name, expression, from, to, .. } => {
                let others = filter_with(t, cfg, Some(i), &ctx)?;
                let ev = Evaluated::new(t, column_name, expression, Some(&ctx))?;
                Ok(time_panel(t, &ev, &others, *from, *to))
            }
            Facet::Text { .. } => Ok(J::Null),
        })
        .collect()
}
