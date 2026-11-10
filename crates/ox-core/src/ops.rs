//! Operations in OpenRefine's operation-history JSON format ("Extract…" / "Apply…").
//! Behaviour is pinned to OpenRefine 3.10.1 by tests/ops_conformance.rs.
use crate::{
    engine::{self, EngineConfig},
    grel::{self, funcs::parse_number, parse::java_regex, Value},
    table::{Column, Table},
    Result,
};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::{cmp::Ordering, collections::HashMap, collections::HashSet, sync::Arc};

/// One recorded operation: OpenRefine's `{"op": ..., "description": ..., ...}`.
#[derive(Clone, Serialize, Deserialize)]
pub struct Op {
    #[serde(flatten)]
    pub kind: OpKind,
    #[serde(default)]
    pub description: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "op")]
pub enum OpKind {
    // ------------------------------------------------------------ cells
    #[serde(rename = "core/text-transform", rename_all = "camelCase")]
    TextTransform {
        #[serde(default)]
        engine_config: EngineConfig,
        column_name: String,
        expression: String,
        #[serde(default = "keep_original")]
        on_error: String,
        #[serde(default)]
        repeat: bool,
        #[serde(default = "ten")]
        repeat_count: u32,
    },
    #[serde(rename = "core/mass-edit", rename_all = "camelCase")]
    MassEdit {
        #[serde(default)]
        engine_config: EngineConfig,
        column_name: String,
        #[serde(default = "engine::value")]
        expression: String,
        edits: Vec<Edit>,
    },
    #[serde(rename = "core/fill-down", rename_all = "camelCase")]
    FillDown {
        #[serde(default)]
        engine_config: EngineConfig,
        column_name: String,
    },
    #[serde(rename = "core/blank-down", rename_all = "camelCase")]
    BlankDown {
        #[serde(default)]
        engine_config: EngineConfig,
        column_name: String,
    },
    #[serde(rename = "core/multivalued-cell-split", rename_all = "camelCase")]
    MultiValuedCellSplit {
        column_name: String,
        key_column_name: String,
        #[serde(default = "separator_mode")]
        mode: String,
        #[serde(default)]
        separator: String,
        #[serde(default)]
        regex: bool,
        #[serde(default)]
        field_lengths: Vec<usize>,
    },
    #[serde(rename = "core/multivalued-cell-join", rename_all = "camelCase")]
    MultiValuedCellJoin {
        column_name: String,
        key_column_name: String,
        separator: String,
    },

    // ------------------------------------------------------------ columns
    #[serde(rename = "core/column-addition", rename_all = "camelCase")]
    ColumnAddition {
        #[serde(default)]
        engine_config: EngineConfig,
        base_column_name: String,
        new_column_name: String,
        #[serde(default)]
        column_insert_index: usize,
        expression: String,
        #[serde(default = "keep_original")]
        on_error: String,
    },
    #[serde(rename = "core/column-split", rename_all = "camelCase")]
    ColumnSplit {
        #[serde(default)]
        engine_config: EngineConfig,
        column_name: String,
        #[serde(default)]
        guess_cell_type: bool,
        #[serde(default)]
        remove_original_column: bool,
        #[serde(default = "separator_mode")]
        mode: String,
        #[serde(default)]
        separator: String,
        #[serde(default)]
        regex: bool,
        #[serde(default)]
        max_columns: usize,
        #[serde(default)]
        field_lengths: Vec<usize>,
    },
    #[serde(rename = "core/column-rename", rename_all = "camelCase")]
    ColumnRename { old_column_name: String, new_column_name: String },
    #[serde(rename = "core/column-removal", rename_all = "camelCase")]
    ColumnRemoval { column_name: String },
    #[serde(rename = "core/column-multi-removal", rename_all = "camelCase")]
    ColumnMultiRemoval { column_names: Vec<String> },
    #[serde(rename = "core/column-move", rename_all = "camelCase")]
    ColumnMove { column_name: String, index: usize },
    #[serde(rename = "core/column-move-left", rename_all = "camelCase")]
    ColumnMoveLeft { column_name: String },
    #[serde(rename = "core/column-move-right", rename_all = "camelCase")]
    ColumnMoveRight { column_name: String },
    #[serde(rename = "core/column-move-first", rename_all = "camelCase")]
    ColumnMoveFirst { column_name: String },
    #[serde(rename = "core/column-move-last", rename_all = "camelCase")]
    ColumnMoveLast { column_name: String },
    #[serde(rename = "core/column-reorder", rename_all = "camelCase")]
    ColumnReorder { column_names: Vec<String> },

    // ------------------------------------------------------------ rows
    #[serde(rename = "core/row-removal", rename_all = "camelCase")]
    RowRemoval {
        #[serde(default)]
        engine_config: EngineConfig,
    },
    #[serde(rename = "core/row-keep-matched", rename_all = "camelCase")]
    RowKeepMatched {
        #[serde(default)]
        engine_config: EngineConfig,
    },
    #[serde(rename = "core/row-star", rename_all = "camelCase")]
    RowStar {
        #[serde(default)]
        engine_config: EngineConfig,
        starred: bool,
    },
    #[serde(rename = "core/row-flag", rename_all = "camelCase")]
    RowFlag {
        #[serde(default)]
        engine_config: EngineConfig,
        flagged: bool,
    },
    #[serde(rename = "core/row-duplicate-removal", rename_all = "camelCase")]
    RowDuplicateRemoval {
        #[serde(default)]
        engine_config: EngineConfig,
        criteria: Vec<String>,
    },
    #[serde(rename = "core/row-reorder", rename_all = "camelCase")]
    RowReorder {
        #[serde(default = "row_based")]
        mode: String,
        sorting: Sorting,
    },
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Edit {
    pub from: Vec<String>,
    #[serde(default)]
    pub from_blank: bool,
    #[serde(default)]
    pub from_error: bool,
    pub to: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Sorting {
    pub criteria: Vec<Criterion>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Criterion {
    pub column: String,
    #[serde(default = "string_type")]
    pub value_type: String,
    #[serde(default)]
    pub reverse: bool,
    #[serde(default = "two")]
    pub blank_position: i32,
    #[serde(default = "one")]
    pub error_position: i32,
    #[serde(default)]
    pub case_sensitive: bool,
}

fn keep_original() -> String {
    "keep-original".into()
}
fn separator_mode() -> String {
    "separator".into()
}
fn row_based() -> String {
    "row-based".into()
}
fn string_type() -> String {
    "string".into()
}
fn ten() -> u32 {
    10
}
fn one() -> i32 {
    1
}
fn two() -> i32 {
    2
}

// ---------------------------------------------------------------- expression helpers

/// ExpressionUtils.wrapStorable: arrays, JSON and HTML can't live in a cell and become an error.
fn storable(v: Value) -> Value {
    match v {
        Value::Array(_) | Value::Json(_) | Value::Html(_) | Value::Obj(_) | Value::Re(_) => Value::err(format!("{} value not storable", v.type_name())),
        v => v,
    }
}

/// Evaluate a cell expression with OpenRefine's onError and repeat semantics.
/// `fallback` is what keep-original keeps (the cell itself, or the base cell for a new column).
fn run(p: &grel::Program, v: &Value, fallback: &Value, row: &dyn grel::Row, on_error: &str, times: u32) -> Value {
    let mut cur = v.clone();
    for _ in 0..times.max(1) {
        let out = match storable(p.eval(cur.clone(), row)) {
            Value::Error(_) if on_error == "set-to-blank" => Value::Null,
            e @ Value::Error(_) if on_error == "store-error" => e,
            Value::Error(_) => fallback.clone(),
            out => out,
        };
        if out == cur {
            break;
        }
        cur = out;
    }
    cur
}

/// Apply `p` to a column: once per distinct value when it only reads `value`, else row by row.
fn eval_column(t: &Table, ci: usize, sel: Option<&[bool]>, new_column: bool, p: &grel::Program, on_error: &str, times: u32) -> Column {
    let col = &t.columns[ci];
    let fallback = |v: &Value| if new_column && on_error != "keep-original" { Value::Null } else { v.clone() };
    if p.uses_row() {
        let ctx = crate::engine::Ctx::default();
        col.map_rows(sel, new_column, |r, v| run(p, v, &fallback(v), &t.row_ctx(r, &ctx), on_error, times))
    } else {
        col.map(sel, new_column, |v| run(p, v, &fallback(v), &grel::NoRow, on_error, times))
    }
}

// ---------------------------------------------------------------- splitting

/// Pieces of `s`: StringUtils.splitByWholeSeparatorPreserveAllTokens, or Pattern.split for regexes
/// (which drops trailing empty pieces when unlimited). `limit` 0 means no limit.
fn split_text(s: &str, sep: &str, re: Option<&Regex>, limit: usize) -> Vec<String> {
    let mut parts: Vec<String> = match (re, limit) {
        (Some(r), 0) => r.split(s).map(String::from).collect(),
        (Some(r), n) => r.splitn(s, n).map(String::from).collect(),
        (None, _) if sep.is_empty() => s.split_whitespace().map(String::from).collect(),
        (None, 0) => s.split(sep).map(String::from).collect(),
        (None, n) => s.splitn(n, sep).map(String::from).collect(),
    };
    if re.is_some() && limit == 0 {
        while parts.last().is_some_and(String::is_empty) {
            parts.pop();
        }
    }
    parts
}

/// Consecutive substrings of the given lengths (UTF-16 units), stopping when the text runs out.
fn split_lengths(s: &str, lengths: &[usize]) -> Vec<String> {
    let u: Vec<u16> = s.encode_utf16().collect();
    let mut pos = 0;
    let mut out = vec![];
    for &n in lengths {
        if pos >= u.len() {
            break;
        }
        let end = (pos + n).min(u.len());
        out.push(String::from_utf16_lossy(&u[pos..end]));
        pos = end;
    }
    out
}

fn splitter(mode: &str, sep: &str, regex: bool, lengths: &[usize], limit: usize) -> Result<impl Fn(&str) -> Vec<String>> {
    let re = if regex && mode != "lengths" { Some(Regex::new(&java_regex(sep)).map_err(|e| format!("invalid separator regex: {e}"))?) } else { None };
    let (mode, sep, lengths) = (mode.to_string(), sep.to_string(), lengths.to_vec());
    Ok(move |s: &str| if mode == "lengths" { split_lengths(s, &lengths) } else { split_text(s, &sep, re.as_ref(), limit) })
}

/// ImporterUtilities.parseCellValue for "guess cell type": numbers become numbers.
fn guess_type(s: String) -> Value {
    parse_number(&s).unwrap_or(Value::Str(s))
}

// ---------------------------------------------------------------- sorting

/// Java's default Collator (en, tertiary strength), approximately: letters first, accents next,
/// case after that (lower before upper), and whitespace/hyphens only as a final tie-breaker.
fn collation_key(s: &str) -> (String, String, String, usize, String) {
    let ignorable = |c: &char| c.is_whitespace() || *c == '-' || *c == '\u{ad}';
    let core: String = s.chars().filter(|c| !ignorable(c)).collect();
    let lower = core.to_lowercase();
    let primary = crate::cluster::asciify(&lower);
    let case: String = core.chars().map(|c| if c.is_uppercase() { '1' } else { '0' }).collect();
    (primary, lower, case, s.chars().filter(ignorable).count(), s.to_string())
}

#[derive(Clone, PartialEq, PartialOrd)]
enum SortValue {
    Num(f64),
    Text((String, String, String, usize, String)),
    Date(chrono::NaiveDateTime),
    Bool(bool),
}

enum SortKey {
    Valid(SortValue),
    Error,
    Blank,
}

fn sort_key(v: &Value, c: &Criterion) -> SortKey {
    use SortKey::*;
    if v.is_blank() {
        return Blank;
    }
    if v.is_error() {
        return Error;
    }
    match c.value_type.as_str() {
        "number" => match v {
            Value::Int(_) | Value::Num(_) => Valid(SortValue::Num(v.as_f64().unwrap())),
            Value::Str(s) => parse_number(s).and_then(|n| n.as_f64()).map_or(Error, |n| Valid(SortValue::Num(n))),
            _ => Error,
        },
        "date" => match v {
            Value::Date(d) => Valid(SortValue::Date(*d)),
            _ => Error,
        },
        "boolean" => match v {
            Value::Bool(b) => Valid(SortValue::Bool(*b)),
            Value::Str(s) if s == "true" || s == "false" => Valid(SortValue::Bool(s == "true")),
            _ => Error,
        },
        _ => {
            let s = v.display();
            Valid(SortValue::Text(collation_key(&if c.case_sensitive { s } else { s.to_lowercase() })))
        }
    }
}

fn compare_keys(a: &SortKey, b: &SortKey, c: &Criterion) -> Ordering {
    let valid = 3 - c.blank_position - c.error_position;
    let rank = |k: &SortKey| match k {
        SortKey::Valid(_) => valid,
        SortKey::Error => c.error_position,
        SortKey::Blank => c.blank_position,
    };
    match (a, b) {
        (SortKey::Valid(x), SortKey::Valid(y)) => {
            let o = x.partial_cmp(y).unwrap_or(Ordering::Equal);
            if c.reverse { o.reverse() } else { o }
        }
        _ => rank(a).cmp(&rank(b)),
    }
}

// ---------------------------------------------------------------- columns

fn move_column(t: &mut Table, from: usize, to: usize) {
    let name = t.names.remove(from);
    let col = t.columns.remove(from);
    let to = to.min(t.names.len());
    t.names.insert(to, name);
    t.columns.insert(to, col);
}

fn set_column(t: &Table, ci: usize, values: Vec<Value>) -> Table {
    let mut out = t.clone();
    out.columns[ci] = Arc::new(Column::from_values(values));
    out
}

fn column_values(t: &Table, ci: usize) -> Vec<Value> {
    (0..t.rows).map(|r| t.columns[ci].get(r).clone()).collect()
}

fn unique_name(t: &Table, base: String) -> String {
    let mut name = base.clone();
    let mut k = 2;
    while t.names.contains(&name) {
        name = format!("{base} {k}");
        k += 1;
    }
    name
}

fn set_flags(t: &Table, rows: &[u32], starred: Option<bool>, flagged: Option<bool>) -> Table {
    let mut out = t.clone();
    if let Some(v) = starred {
        let mut s = (*t.starred).clone();
        rows.iter().for_each(|&r| s[r as usize] = v);
        out.starred = Arc::new(s);
    }
    if let Some(v) = flagged {
        let mut f = (*t.flagged).clone();
        rows.iter().for_each(|&r| f[r as usize] = v);
        out.flagged = Arc::new(f);
    }
    out
}

impl Op {
    pub fn description(&self) -> &str {
        &self.description
    }

    /// Default description, in OpenRefine's wording, for operations that arrive without one.
    fn describe(&self) -> String {
        use OpKind::*;
        match &self.kind {
            TextTransform { column_name, expression, .. } => format!("Text transform on cells in column {column_name} using expression {expression}"),
            MassEdit { column_name, .. } => format!("Mass edit cells in column {column_name}"),
            FillDown { column_name, .. } => format!("Fill down cells in column {column_name}"),
            BlankDown { column_name, .. } => format!("Blank down cells in column {column_name}"),
            MultiValuedCellSplit { column_name, separator, .. } => format!("Split multi-valued cells in column {column_name} by {separator}"),
            MultiValuedCellJoin { column_name, separator, .. } => format!("Join multi-valued cells in column {column_name} with {separator}"),
            ColumnAddition { new_column_name, base_column_name, expression, .. } => {
                format!("Create column {new_column_name} based on column {base_column_name} using expression {expression}")
            }
            ColumnSplit { column_name, .. } => format!("Split column {column_name}"),
            ColumnRename { old_column_name, new_column_name } => format!("Rename column {old_column_name} to {new_column_name}"),
            ColumnRemoval { column_name } => format!("Remove column {column_name}"),
            ColumnMultiRemoval { column_names } => format!("Remove {} columns", column_names.len()),
            ColumnMove { column_name, index } => format!("Move column {column_name} to position {index}"),
            ColumnMoveLeft { column_name } => format!("Move column {column_name} to the left"),
            ColumnMoveRight { column_name } => format!("Move column {column_name} to the right"),
            ColumnMoveFirst { column_name } => format!("Move column {column_name} to the beginning"),
            ColumnMoveLast { column_name } => format!("Move column {column_name} to the end"),
            ColumnReorder { .. } => "Reorder columns".into(),
            RowRemoval { .. } => "Remove rows".into(),
            RowKeepMatched { .. } => "Keep only matching rows".into(),
            RowStar { starred, .. } => if *starred { "Star rows" } else { "Unstar rows" }.into(),
            RowFlag { flagged, .. } => if *flagged { "Flag rows" } else { "Unflag rows" }.into(),
            RowDuplicateRemoval { criteria, .. } => format!("Remove duplicate rows by {}", criteria.join(", ")),
            RowReorder { .. } => "Reorder rows".into(),
        }
    }

    pub fn apply(&mut self, t: &Table) -> Result<Table> {
        if self.description.is_empty() {
            self.description = self.describe();
        }
        use OpKind::*;
        Ok(match &self.kind {
            TextTransform { engine_config, column_name, expression, on_error, repeat, repeat_count } => {
                let p = grel::compile(expression)?;
                let ci = t.col(column_name)?;
                let sel = engine::mask(t, engine_config)?;
                let times = if *repeat { (*repeat_count).max(1) } else { 1 };
                let mut out = t.clone();
                out.columns[ci] = Arc::new(eval_column(t, ci, sel.as_deref(), false, &p, on_error, times));
                out
            }
            MassEdit { engine_config, column_name, expression, edits } => {
                if expression != "value" {
                    return Err(format!("mass edit expression `{expression}` is not supported (only `value`)"));
                }
                let ci = t.col(column_name)?;
                let sel = engine::mask(t, engine_config)?;
                let map: HashMap<&str, &str> = edits.iter().flat_map(|e| e.from.iter().map(move |f| (f.as_str(), e.to.as_str()))).collect();
                let blank_to = edits.iter().find(|e| e.from_blank).map(|e| e.to.as_str());
                let mut out = t.clone();
                out.columns[ci] = Arc::new(t.columns[ci].map(sel.as_deref(), false, |v| {
                    let to = if v.is_blank() { blank_to.or_else(|| map.get("").copied()) } else { map.get(v.display().as_str()).copied() };
                    to.map_or_else(|| v.clone(), |s| Value::Str(s.to_string()))
                }));
                out
            }
            FillDown { engine_config, column_name } | BlankDown { engine_config, column_name } => {
                let fill = matches!(self.kind, FillDown { .. });
                let ci = t.col(column_name)?;
                let rows = engine::filter(t, engine_config, None)?;
                let starts = (engine_config.mode == "record-based").then(|| t.record_starts(t.key_index()));
                let mut vals = column_values(t, ci);
                let mut last: Option<Value> = None;
                for &r in &rows {
                    let r = r as usize;
                    if starts.as_ref().is_some_and(|s| s[r]) {
                        last = None;
                    }
                    let v = vals[r].clone();
                    if fill {
                        if v.is_blank() {
                            if let Some(l) = &last {
                                vals[r] = l.clone();
                            }
                        } else {
                            last = Some(v);
                        }
                    } else if !v.is_blank() && last.as_ref() == Some(&v) {
                        vals[r] = Value::Null;
                    } else {
                        last = Some(v);
                    }
                }
                set_column(t, ci, vals)
            }
            MultiValuedCellSplit { column_name, key_column_name, mode, separator, regex, field_lengths } => {
                let ci = t.col(column_name)?;
                let starts = t.record_starts(t.col(key_column_name)?);
                let split = splitter(mode, separator, *regex, field_lengths, 0)?;
                let col = &t.columns[ci];
                // (source row or a new empty row, value for the split column)
                let mut plan: Vec<(Option<u32>, Value)> = Vec::with_capacity(t.rows);
                let mut r = 0;
                while r < t.rows {
                    let v = col.get(r);
                    let pieces = match v {
                        Value::Str(s) => split(s),
                        _ => vec![],
                    };
                    if pieces.len() < 2 {
                        plan.push((Some(r as u32), v.clone()));
                        r += 1;
                        continue;
                    }
                    let mut pieces = pieces.into_iter();
                    plan.push((Some(r as u32), Value::Str(pieces.next().unwrap())));
                    let mut rest: Vec<String> = pieces.collect();
                    let mut k = r + 1;
                    // later pieces first fill blank cells of the following rows in the same record
                    while k < t.rows && !rest.is_empty() && !starts[k] && col.get(k).is_blank() {
                        plan.push((Some(k as u32), Value::Str(rest.remove(0))));
                        k += 1;
                    }
                    plan.extend(rest.into_iter().map(|p| (None, Value::Str(p))));
                    r = k;
                }
                let (rows, vals): (Vec<_>, Vec<_>) = plan.into_iter().unzip();
                let mut out = t.pick_rows(&rows);
                out.columns[ci] = Arc::new(Column::from_values(vals));
                out
            }
            MultiValuedCellJoin { column_name, key_column_name, separator } => {
                let ci = t.col(column_name)?;
                let starts = t.record_starts(t.col(key_column_name)?);
                let mut vals = column_values(t, ci);
                let mut r = 0;
                while r < t.rows {
                    let end = (r + 1..t.rows).find(|&k| starts[k]).unwrap_or(t.rows);
                    let parts: Vec<String> = (r..end).filter(|&k| !vals[k].is_blank()).map(|k| vals[k].to_java_string()).collect();
                    if !parts.is_empty() {
                        vals[r] = Value::Str(parts.join(separator));
                    }
                    vals[r + 1..end].iter_mut().for_each(|v| *v = Value::Null);
                    r = end;
                }
                let joined = set_column(t, ci, vals);
                // rows left with nothing in them disappear
                let keep: Vec<Option<u32>> = (0..t.rows).filter(|&r| joined.columns.iter().any(|c| !c.get(r).is_blank())).map(|r| Some(r as u32)).collect();
                joined.pick_rows(&keep)
            }
            ColumnAddition { engine_config, base_column_name, new_column_name, column_insert_index, expression, on_error } => {
                if t.col(new_column_name).is_ok() {
                    return Err(format!("column '{new_column_name}' already exists"));
                }
                let p = grel::compile(expression)?;
                let base = t.col(base_column_name)?;
                let sel = engine::mask(t, engine_config)?;
                let at = (*column_insert_index).min(t.names.len());
                let mut out = t.clone();
                out.columns.insert(at, Arc::new(eval_column(t, base, sel.as_deref(), true, &p, on_error, 1)));
                out.names.insert(at, new_column_name.clone());
                out
            }
            ColumnSplit { engine_config, column_name, guess_cell_type, remove_original_column, mode, separator, regex, max_columns, field_lengths } => {
                let ci = t.col(column_name)?;
                let sel = engine::mask(t, engine_config)?;
                let split = splitter(mode, separator, *regex, field_lengths, *max_columns)?;
                let col = &t.columns[ci];
                let pieces: Vec<Vec<Value>> = (0..t.rows)
                    .map(|r| match col.get(r) {
                        Value::Str(s) if sel.as_ref().map_or(true, |m| m[r]) => {
                            split(s).into_iter().map(|p| if *guess_cell_type { guess_type(p) } else { Value::Str(p) }).collect()
                        }
                        _ => vec![],
                    })
                    .collect();
                let n = pieces.iter().map(Vec::len).max().unwrap_or(0);
                let mut out = t.clone();
                for k in 0..n {
                    let name = unique_name(&out, format!("{column_name} {}", k + 1));
                    out.names.insert(ci + 1 + k, name);
                    out.columns.insert(ci + 1 + k, Arc::new(Column::from_values(pieces.iter().map(|p| p.get(k).cloned().unwrap_or(Value::Null)))));
                }
                if *remove_original_column {
                    out.names.remove(ci);
                    out.columns.remove(ci);
                }
                out
            }
            ColumnRename { old_column_name, new_column_name } => {
                let ci = t.col(old_column_name)?;
                if t.col(new_column_name).is_ok() {
                    return Err(format!("column '{new_column_name}' already exists"));
                }
                let mut out = t.clone();
                out.names[ci] = new_column_name.clone();
                if out.key.as_deref() == Some(old_column_name) {
                    out.key = Some(new_column_name.clone());
                }
                out
            }
            ColumnRemoval { column_name } => {
                let ci = t.col(column_name)?;
                let mut out = t.clone();
                out.names.remove(ci);
                out.columns.remove(ci);
                out
            }
            ColumnMultiRemoval { column_names } => {
                let mut out = t.clone();
                for n in column_names {
                    let ci = out.col(n)?;
                    out.names.remove(ci);
                    out.columns.remove(ci);
                }
                out
            }
            ColumnMove { column_name, index } => {
                let mut out = t.clone();
                move_column(&mut out, t.col(column_name)?, *index);
                out
            }
            ColumnMoveLeft { column_name } | ColumnMoveRight { column_name } | ColumnMoveFirst { column_name } | ColumnMoveLast { column_name } => {
                let ci = t.col(column_name)?;
                let to = match self.kind {
                    ColumnMoveLeft { .. } => ci.saturating_sub(1),
                    ColumnMoveRight { .. } => ci + 1,
                    ColumnMoveFirst { .. } => 0,
                    _ => t.names.len(),
                };
                let mut out = t.clone();
                move_column(&mut out, ci, to);
                out
            }
            ColumnReorder { column_names } => {
                let idx = column_names.iter().map(|n| t.col(n)).collect::<Result<Vec<_>>>()?;
                let mut out = t.clone();
                out.names = column_names.clone();
                out.columns = idx.iter().map(|&i| t.columns[i].clone()).collect();
                out
            }
            RowRemoval { engine_config } | RowKeepMatched { engine_config } => {
                let keep_matched = matches!(self.kind, RowKeepMatched { .. });
                let hit: HashSet<u32> = engine::filter(t, engine_config, None)?.into_iter().collect();
                let rows: Vec<Option<u32>> = (0..t.rows as u32).filter(|r| hit.contains(r) == keep_matched).map(Some).collect();
                t.pick_rows(&rows)
            }
            RowStar { engine_config, starred } => set_flags(t, &engine::filter(t, engine_config, None)?, Some(*starred), None),
            RowFlag { engine_config, flagged } => set_flags(t, &engine::filter(t, engine_config, None)?, None, Some(*flagged)),
            RowDuplicateRemoval { engine_config, criteria } => {
                let cols = criteria.iter().map(|n| t.col(n)).collect::<Result<Vec<_>>>()?;
                let mut seen: HashSet<Vec<Value>> = HashSet::new();
                let dup: HashSet<u32> = engine::filter(t, engine_config, None)?
                    .into_iter()
                    .filter(|&r| !seen.insert(cols.iter().map(|&c| t.columns[c].get(r as usize).clone()).collect()))
                    .collect();
                let rows: Vec<Option<u32>> = (0..t.rows as u32).filter(|r| !dup.contains(r)).map(Some).collect();
                t.pick_rows(&rows)
            }
            RowReorder { mode, sorting } => {
                let cols = sorting.criteria.iter().map(|c| t.col(&c.column)).collect::<Result<Vec<_>>>()?;
                let keys: Vec<Vec<SortKey>> = (0..t.rows)
                    .map(|r| sorting.criteria.iter().zip(&cols).map(|(c, &ci)| sort_key(t.columns[ci].get(r), c)).collect())
                    .collect();
                let cmp = |a: usize, b: usize| {
                    sorting.criteria.iter().enumerate().map(|(i, c)| compare_keys(&keys[a][i], &keys[b][i], c)).find(|o| o.is_ne()).unwrap_or(Ordering::Equal)
                };
                // records mode keeps each record together, ordered by its first row
                let groups: Vec<(usize, usize)> = if mode == "record-based" {
                    let starts = t.record_starts(t.key_index());
                    let s: Vec<usize> = (0..t.rows).filter(|&r| starts[r]).collect();
                    s.iter().enumerate().map(|(i, &a)| (a, s.get(i + 1).copied().unwrap_or(t.rows))).collect()
                } else {
                    (0..t.rows).map(|r| (r, r + 1)).collect()
                };
                let mut order: Vec<usize> = (0..groups.len()).collect();
                order.sort_by(|&a, &b| cmp(groups[a].0, groups[b].0));
                let rows: Vec<Option<u32>> = order.iter().flat_map(|&g| (groups[g].0..groups[g].1).map(|r| Some(r as u32))).collect();
                t.pick_rows(&rows)
            }
        })
    }
}
