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
    // ------------------------------------------------------------ transposing
    #[serde(rename = "core/transpose-columns-into-rows", rename_all = "camelCase")]
    TransposeColumnsIntoRows {
        start_column_name: String,
        /// how many columns from the start one; -1 for all the rest
        column_count: i64,
        #[serde(default)]
        ignore_blank_cells: bool,
        #[serde(default)]
        fill_down: bool,
        /// one new column holding the cells (optionally "<column><separator><cell>")...
        #[serde(default, skip_serializing_if = "Option::is_none")]
        combined_column_name: Option<String>,
        #[serde(default)]
        prepend_column_name: bool,
        /// OpenRefine writes null in key/value mode
        #[serde(default)]
        separator: Option<String>,
        /// ...or a key column (the old column names) and a value column
        #[serde(default, skip_serializing_if = "Option::is_none")]
        key_column_name: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        value_column_name: Option<String>,
    },
    #[serde(rename = "core/transpose-rows-into-columns", rename_all = "camelCase")]
    TransposeRowsIntoColumns { column_name: String, row_count: usize },
    #[serde(rename = "core/key-value-columnize", rename_all = "camelCase")]
    KeyValueColumnize {
        key_column_name: String,
        value_column_name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        note_column_name: Option<String>,
    },
    // ------------------------------------------------------------ fetching
    #[serde(rename = "core/column-addition-by-fetching-urls", rename_all = "camelCase")]
    ColumnAdditionByFetchingUrls {
        #[serde(default)]
        engine_config: EngineConfig,
        base_column_name: String,
        url_expression: String,
        #[serde(default = "set_to_blank")]
        on_error: String,
        new_column_name: String,
        #[serde(default)]
        column_insert_index: usize,
        /// milliseconds between requests
        #[serde(default)]
        delay: u64,
        #[serde(default)]
        cache_responses: bool,
        #[serde(default)]
        http_headers_json: Vec<HttpHeader>,
        /// What was fetched, row by row (null | text | {"e": message}), so replaying the history
        /// doesn't fetch again. Saved in OxRefine's history; left out of recipes.
        #[serde(rename = "ox:fetched", default, skip_serializing_if = "Option::is_none")]
        fetched: Option<Vec<serde_json::Value>>,
    },

    // ------------------------------------------------------------ reconciliation
    #[serde(rename = "core/recon", rename_all = "camelCase")]
    Recon {
        #[serde(default)]
        engine_config: EngineConfig,
        column_name: String,
        /// StandardReconConfig JSON, kept as given
        config: serde_json::Value,
        /// The Recons the service gave, so replaying the history doesn't ask again (see
        /// recon::save_cells). Saved in OxRefine's history; left out of recipes.
        #[serde(rename = "ox:recon", default, skip_serializing_if = "Option::is_none")]
        results: Option<serde_json::Value>,
    },

    #[serde(rename = "core/recon-match-best-candidates", rename_all = "camelCase")]
    ReconMatchBestCandidates {
        #[serde(default)]
        engine_config: EngineConfig,
        column_name: String,
    },
    #[serde(rename = "core/recon-mark-new-topics", rename_all = "camelCase")]
    ReconMarkNewTopics {
        #[serde(default)]
        engine_config: EngineConfig,
        column_name: String,
        #[serde(default)]
        share_new_topics: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        service: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        identifier_space: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        schema_space: Option<String>,
    },
    #[serde(rename = "core/recon-match-specific-topic-to-cells", rename_all = "camelCase")]
    ReconMatchSpecificTopic {
        #[serde(default)]
        engine_config: EngineConfig,
        column_name: String,
        /// {id, name, types}
        r#match: serde_json::Value,
        #[serde(default)]
        identifier_space: Option<String>,
        #[serde(default)]
        schema_space: Option<String>,
    },
    #[serde(rename = "core/recon-discard-judgments", rename_all = "camelCase")]
    ReconDiscardJudgments {
        #[serde(default)]
        engine_config: EngineConfig,
        column_name: String,
        #[serde(default)]
        clear_data: bool,
    },
    #[serde(rename = "core/recon-clear-similar-cells", rename_all = "camelCase")]
    ReconClearSimilarCells {
        #[serde(default)]
        engine_config: EngineConfig,
        column_name: String,
        similar_value: String,
    },
    #[serde(rename = "core/recon-judge-similar-cells", rename_all = "camelCase")]
    ReconJudgeSimilarCells {
        #[serde(default)]
        engine_config: EngineConfig,
        column_name: String,
        similar_value: String,
        /// matched | new | none
        judgment: String,
        /// {id, name, types, score} for "matched"
        #[serde(default, skip_serializing_if = "Option::is_none")]
        r#match: Option<serde_json::Value>,
        #[serde(default)]
        share_new_topics: bool,
    },
    #[serde(rename = "core/recon-use-values-as-identifiers", rename_all = "camelCase")]
    ReconUseValuesAsIdentifiers {
        #[serde(default)]
        engine_config: EngineConfig,
        column_name: String,
        #[serde(default)]
        service: Option<String>,
        identifier_space: String,
        schema_space: String,
    },
    #[serde(rename = "core/recon-copy-across-columns", rename_all = "camelCase")]
    ReconCopyAcrossColumns {
        #[serde(default)]
        engine_config: EngineConfig,
        from_column_name: String,
        to_column_names: Vec<String>,
        judgments: Vec<String>,
        #[serde(default)]
        apply_to_judged_cells: bool,
    },

    // ------------------------------------------------------------ single-cell and single-row edits
    /// OpenRefine's recon-judge-one-cell: match a cell to an item, mark it new, or discard its judgment.
    #[serde(rename = "ox/recon-judge-one-cell", rename_all = "camelCase")]
    ReconJudgeOneCell {
        row: usize,
        column_name: String,
        judgment: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        r#match: Option<serde_json::Value>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        identifier_space: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        schema_space: Option<String>,
    },
    /// OpenRefine's recon-clear-one-cell.
    #[serde(rename = "ox/recon-clear-one-cell", rename_all = "camelCase")]
    ReconClearOneCell { row: usize, column_name: String },
    // OpenRefine records these in history but not in the operations JSON it extracts ("ox/" ops
    // stay out of recipes the same way).
    #[serde(rename = "ox/cell-edit", rename_all = "camelCase")]
    CellEdit {
        row: usize,
        column_name: String,
        value: String,
        /// text | number | boolean | date, as in OpenRefine's cell editor
        #[serde(default = "text_type", rename = "type")]
        kind: String,
    },
    #[serde(rename = "ox/row-annotate")]
    RowAnnotate {
        row: usize,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        starred: Option<bool>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        flagged: Option<bool>,
    },
}

#[derive(Clone, Serialize, Deserialize)]
pub struct HttpHeader {
    pub name: String,
    pub value: String,
}

fn set_to_blank() -> String {
    "set-to-blank".into()
}

/// GET `url` with `headers`: the body when the server answers 2xx, else what went wrong.
fn fetch(agent: &ureq::Agent, url: &str, headers: &[HttpHeader]) -> std::result::Result<String, String> {
    let mut req = agent.get(url);
    for h in headers.iter().filter(|h| !h.value.is_empty()) {
        req = req.header(h.name.as_str(), h.value.as_str());
    }
    let mut res = req.call().map_err(|e| format!("{url}: {e}"))?;
    let status = res.status();
    let body = res.body_mut().read_to_string().map_err(|e| format!("{url}: {e}"))?;
    if status.is_success() {
        Ok(body)
    } else {
        Err(format!("HTTP error {} : {} | {body}", status.as_u16(), status.canonical_reason().unwrap_or("")))
    }
}

fn saved(v: &Value) -> serde_json::Value {
    match v {
        Value::Null => serde_json::Value::Null,
        Value::Error(e) => serde_json::json!({ "e": e }),
        other => serde_json::Value::String(other.to_java_string()),
    }
}

fn unsaved(j: &serde_json::Value) -> Value {
    match j {
        serde_json::Value::String(s) => Value::Str(s.clone()),
        serde_json::Value::Object(m) => Value::Error(m.get("e").and_then(|e| e.as_str()).unwrap_or("").to_string()),
        _ => Value::Null,
    }
}

fn text_type() -> String {
    "text".into()
}

/// The value OpenRefine's edit-one-cell stores for `value` typed as `kind`.
fn typed_cell(value: &str, kind: &str) -> Result<Value> {
    Ok(match kind {
        "text" => Value::Str(value.into()),
        "number" => parse_number(value).ok_or_else(|| format!("`{value}` is not a number"))?,
        "boolean" => Value::Bool(value == "true"),
        // only a full ISO timestamp with offset parses; anything else leaves the cell empty
        "date" => chrono::DateTime::parse_from_rfc3339(value).map_or(Value::Null, |d| Value::Date(d.naive_utc())),
        k => return Err(format!("unknown cell type `{k}` (text, number, boolean, date)")),
    })
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
    if let Some((v, _)) = grel::eval::as_cell(&v) {
        return v;
    }
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
/// A transform keeps the cells' Recons; a new column gets those of the cells the expression
/// returned whole (`cell`), as OpenRefine copies a Cell result.
fn eval_column(t: &Table, ci: usize, sel: Option<&[bool]>, new_column: bool, p: &grel::Program, on_error: &str, times: u32) -> Column {
    let col = &t.columns[ci];
    let fallback = |v: &Value| if new_column && on_error != "keep-original" { Value::Null } else { v.clone() };
    if !new_column {
        let ctx = crate::engine::Ctx::default();
        let out = if p.uses_row_of(col) {
            col.map_rows(sel, false, |r, v| run(p, v, v, &t.row_at(r, &ctx, ci), on_error, times))
        } else {
            col.map(sel, false, |v| run(p, v, v, &grel::NoRow, on_error, times))
        };
        return out.keep_recon(col);
    }
    if p.uses_row_of(col) {
        let ctx = crate::engine::Ctx::default();
        let mut recons = vec![None; t.rows];
        let mut out = col.map_rows(sel, new_column, |r, v| {
            let row = t.row_at(r, &ctx, ci);
            if let Some((v, recon)) = grel::eval::as_cell(&p.eval(v.clone(), &row)) {
                recons[r] = recon;
                return v;
            }
            run(p, v, &fallback(v), &row, on_error, times)
        });
        if recons.iter().any(Option::is_some) {
            out.recon = Some(Arc::new(crate::recon::ReconCol::new(recons)));
        }
        out
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
    /// The operation as OpenRefine's JSON (what Extract gives): without OxRefine's saved results.
    pub fn recipe_json(&self) -> serde_json::Value {
        let mut j = serde_json::to_value(self).unwrap_or(serde_json::Value::Null);
        if let Some(m) = j.as_object_mut() {
            m.retain(|k, _| !k.starts_with("ox:"));
        }
        j
    }

    /// OpenRefine's ReconOperation: the selected cells of the column get Recons from the service
    /// (asked once per distinct query, in batches, as a job with progress and cancel); the column
    /// keeps the config. The Recons are kept in the operation, so replaying reuses them.
    fn reconcile(&mut self, t: &Table) -> Result<Table> {
        let OpKind::Recon { engine_config, column_name, config, results } = &mut self.kind else { unreachable!() };
        let cfg = crate::recon::StandardConfig::from_json(config)?;
        let ci = t.col(column_name)?;
        let new = match results.as_ref().and_then(|j| crate::recon::load_cells(j, t.rows)) {
            Some(cells) => cells,
            None => {
                let sel = engine::mask(t, engine_config)?;
                let history_entry = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64);
                let cells = crate::recon::reconcile(t, column_name, &cfg, sel.as_deref(), history_entry)?;
                *results = Some(crate::recon::save_cells(&cells));
                cells
            }
        };
        let old = &t.columns[ci];
        let cells: Vec<_> = (0..t.rows).map(|r| new[r].clone().or_else(|| old.recon(r).cloned())).collect();
        let edits: crate::recon::Edits = cells.into_iter().map(Some).collect();
        let col = crate::recon::apply_edits(old, &edits, Some(Some(cfg.to_json())), crate::recon::Stats::Count);
        let mut out = t.clone();
        out.columns[ci] = Arc::new(col);
        Ok(out)
    }

    /// The operations on Recons (Reconcile → Actions, judging cells), each in OpenRefine's wording.
    fn recon_action(&mut self, t: &Table) -> Option<Result<Table>> {
        use crate::recon::{self as rc, Action};
        let rows = |e: &EngineConfig| -> Result<Vec<usize>> {
            let sel = engine::mask(t, e)?;
            Ok((0..t.rows).filter(|&r| sel.as_ref().is_none_or(|s| s[r])).collect())
        };
        let column_config = |c: &Column| c.recon.as_ref().and_then(|x| x.config.as_ref()).and_then(|j| rc::StandardConfig::from_json(j).ok());
        let set_stats = |column: &str, edits: rc::Edits, config: Option<Option<serde_json::Value>>, stats: rc::Stats| -> Result<Table> {
            let ci = t.col(column)?;
            let mut out = t.clone();
            out.columns[ci] = Arc::new(rc::apply_edits(&t.columns[ci], &edits, config, stats));
            Ok(out)
        };
        // a ReconChange: the stats are counted again
        let set = |column: &str, edits: rc::Edits, config: Option<Option<serde_json::Value>>| set_stats(column, edits, config, rc::Stats::Count);
        let col = |name: &str| t.col(name).map(|i| &t.columns[i]);
        let candidate = |m: &serde_json::Value, score: f64| rc::Candidate {
            id: m.get("id").and_then(|v| v.as_str()).unwrap_or("").into(),
            name: m.get("name").and_then(|v| v.as_str()).unwrap_or("").into(),
            types: m.get("types").and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default(),
            score: m.get("score").and_then(|v| v.as_f64()).unwrap_or(score),
        };
        let run = || -> Result<Option<Table>> {
            Ok(Some(match &self.kind {
                OpKind::ReconMatchBestCandidates { engine_config, column_name } => {
                    set(column_name, rc::mass(col(column_name)?, &rows(engine_config)?, Action::MatchBest)?, None)?
                }
                OpKind::ReconMarkNewTopics { engine_config, column_name, share_new_topics, service, identifier_space, schema_space } => {
                    let c = col(column_name)?;
                    let cfg = column_config(c).unwrap_or_else(|| {
                        rc::StandardConfig::bare(service.as_deref().unwrap_or(""), identifier_space.as_deref().unwrap_or(""), schema_space.as_deref().unwrap_or(""), false)
                    });
                    let edits = rc::mass(c, &rows(engine_config)?, Action::MarkNew { shared: *share_new_topics, config: Some(&cfg) })?;
                    set(column_name, edits, Some(Some(cfg.to_json())))?
                }
                OpKind::ReconMatchSpecificTopic { engine_config, column_name, r#match, identifier_space, schema_space } => {
                    let action = Action::MatchSpecific { candidate: candidate(r#match, 100.0), identifier_space: identifier_space.as_deref(), schema_space: schema_space.as_deref() };
                    set(column_name, rc::mass(col(column_name)?, &rows(engine_config)?, action)?, None)?
                }
                OpKind::ReconDiscardJudgments { engine_config, column_name, clear_data } => {
                    set(column_name, rc::mass(col(column_name)?, &rows(engine_config)?, Action::Discard { clear: *clear_data })?, None)?
                }
                OpKind::ReconClearSimilarCells { engine_config, column_name, similar_value } => {
                    set(column_name, rc::mass(col(column_name)?, &rows(engine_config)?, Action::ClearSimilar(similar_value))?, None)?
                }
                OpKind::ReconJudgeSimilarCells { engine_config, column_name, similar_value, judgment, r#match, share_new_topics } => {
                    let c = col(column_name)?;
                    let action = Action::JudgeSimilar {
                        value: similar_value,
                        judgment: rc::judgment_of(judgment),
                        matched: r#match.as_ref().map(|m| candidate(m, 100.0)),
                        shared: *share_new_topics,
                        config: column_config(c),
                    };
                    set(column_name, rc::mass(c, &rows(engine_config)?, action)?, None)?
                }
                OpKind::ReconUseValuesAsIdentifiers { engine_config, column_name, service, identifier_space, schema_space } => {
                    let cfg = rc::StandardConfig::bare(service.as_deref().unwrap_or(""), identifier_space, schema_space, true);
                    let edits = rc::mass(col(column_name)?, &rows(engine_config)?, Action::UseValuesAsIds(&cfg))?;
                    set(column_name, edits, Some(Some(cfg.to_json())))?
                }
                OpKind::ReconCopyAcrossColumns { engine_config, from_column_name, to_column_names, judgments, apply_to_judged_cells } => {
                    let from = t.col(from_column_name)?;
                    let to: Vec<usize> = to_column_names.iter().filter_map(|n| t.col(n).ok()).collect();
                    let judgments: Vec<rc::Judgment> = judgments.iter().map(|j| rc::judgment_of(j)).collect();
                    let mut out = t.clone();
                    for (&c, edits) in to.iter().zip(rc::copy_across(t, &rows(engine_config)?, from, &to, &judgments, *apply_to_judged_cells)) {
                        out.columns[c] = Arc::new(rc::apply_edits(&t.columns[c], &edits, None, rc::Stats::Keep));
                    }
                    out
                }
                OpKind::ReconJudgeOneCell { row, column_name, judgment, r#match, identifier_space, schema_space } => {
                    let c = col(column_name)?;
                    if *row >= t.rows {
                        return Err(format!("no row {}", row + 1));
                    }
                    let recon = rc::judge_one(c, *row, rc::judgment_of(judgment), r#match.as_ref().map(|m| candidate(m, 100.0)), (identifier_space.as_deref(), schema_space.as_deref()))?;
                    let stats = rc::one_cell_stats(c, c.recon(*row).map(|r| &**r), Some(&recon));
                    let mut edits: rc::Edits = vec![None; t.rows];
                    edits[*row] = Some(Some(recon));
                    set_stats(column_name, edits, None, rc::Stats::Set(stats))?
                }
                OpKind::ReconClearOneCell { row, column_name } => {
                    if *row >= t.rows {
                        return Err(format!("no row {}", row + 1));
                    }
                    let c = col(column_name)?;
                    let stats = rc::one_cell_stats(c, c.recon(*row).map(|r| &**r), None);
                    let mut edits: rc::Edits = vec![None; t.rows];
                    edits[*row] = Some(None);
                    set_stats(column_name, edits, None, rc::Stats::Set(stats))?
                }
                _ => return Ok(None),
            }))
        };
        let out = run().transpose()?;
        // single-cell judgments are described by the cell, as OpenRefine's history shows them
        let at = |row: &usize, column_name: &str| {
            let cell = t.col(column_name).map(|c| t.columns[c].get(*row).to_java_string()).unwrap_or_default();
            format!("single cell on row {}, column {column_name}, containing \"{cell}\"", row + 1)
        };
        let described = match (&self.kind, &out) {
            (OpKind::ReconClearOneCell { row, column_name }, Ok(_)) => Some(format!("Clear recon data for {}", at(row, column_name))),
            (OpKind::ReconJudgeOneCell { row, column_name, judgment, r#match, .. }, Ok(_)) => Some(match judgment.as_str() {
                "matched" => {
                    let s = |k: &str| r#match.as_ref().and_then(|m| m.get(k)).and_then(|v| v.as_str()).unwrap_or("").to_string();
                    format!("Match {} ({}) to {}", s("name"), s("id"), at(row, column_name))
                }
                "new" => format!("Mark to create new item for {}", at(row, column_name)),
                _ => format!("Discard recon judgment for {}", at(row, column_name)),
            }),
            _ => None,
        };
        if let Some(d) = described {
            self.description = d;
        }
        Some(out)
    }

    /// OpenRefine's ColumnAdditionByFetchingURLsOperation: one request per matching row, one after
    /// another with `delay` between them, as a job that shows progress and can be cancelled. The
    /// results are kept in the operation, so replaying the history reuses them.
    fn fetch_urls(&mut self, t: &Table) -> Result<Table> {
        let OpKind::ColumnAdditionByFetchingUrls {
            engine_config, base_column_name, url_expression, on_error, new_column_name, column_insert_index, delay, cache_responses, http_headers_json, fetched,
        } = &mut self.kind
        else {
            unreachable!()
        };
        if t.col(new_column_name).is_ok() {
            return Err(format!("column '{new_column_name}' already exists"));
        }
        let base = t.col(base_column_name)?;
        let values: Vec<Value> = match fetched {
            Some(saved_rows) if saved_rows.len() == t.rows => saved_rows.iter().map(unsaved).collect(),
            _ => {
                let p = grel::compile(url_expression)?;
                let sel = engine::mask(t, engine_config)?;
                let ctx = crate::engine::Ctx::default();
                let rows: Vec<usize> = (0..t.rows).filter(|&r| sel.as_ref().is_none_or(|s| s[r])).collect();
                let agent: ureq::Agent = ureq::Agent::new_with_config(
                    ureq::config::Config::builder()
                        .http_status_as_error(false)
                        .timeout_global(Some(std::time::Duration::from_secs(60)))
                        .user_agent(concat!("OxRefine/", env!("CARGO_PKG_VERSION")))
                        .build(),
                );
                crate::jobs::progress("Fetching URLs", rows.len() as u64);
                let mut cache: HashMap<String, Value> = HashMap::new();
                let mut out = vec![Value::Null; t.rows];
                let mut first = true;
                for r in rows {
                    crate::jobs::tick(0)?;
                    let cell = t.columns[base].get(r);
                    let url = match p.eval(cell.clone(), &t.row_ctx(r, &ctx)) {
                        Value::Null | Value::Error(_) => None,
                        Value::Str(s) if s.trim().is_empty() => None,
                        v => Some(v.to_java_string()),
                    };
                    if let Some(url) = url {
                        out[r] = match cache.get(&url) {
                            Some(v) => v.clone(),
                            None => {
                                if !first && *delay > 0 {
                                    std::thread::sleep(std::time::Duration::from_millis(*delay));
                                }
                                first = false;
                                let v = match fetch(&agent, &url, http_headers_json) {
                                    Ok(body) => Value::Str(body),
                                    Err(e) => match on_error.as_str() {
                                        "store-error" => Value::Error(e),
                                        "keep-original" => cell.clone(),
                                        _ => Value::Null,
                                    },
                                };
                                if *cache_responses && !v.is_error() {
                                    cache.insert(url, v.clone());
                                }
                                v
                            }
                        };
                    }
                    crate::jobs::tick(1)?;
                }
                *fetched = Some(out.iter().map(saved).collect());
                out
            }
        };
        let at = (*column_insert_index).min(t.names.len());
        let mut out = t.clone();
        out.columns.insert(at, Arc::new(Column::from_values(values)));
        out.names.insert(at, new_column_name.clone());
        Ok(out)
    }

    pub fn description(&self) -> &str {
        &self.description
    }

    /// Whether OpenRefine would list this step in an extracted operation history.
    /// The column a step on Recons works on, written as OpenRefine's ReconChange in project archives.
    pub fn recon_column(&self) -> Option<&str> {
        use OpKind::*;
        match &self.kind {
            Recon { column_name, .. }
            | ReconMatchBestCandidates { column_name, .. }
            | ReconMarkNewTopics { column_name, .. }
            | ReconMatchSpecificTopic { column_name, .. }
            | ReconDiscardJudgments { column_name, .. }
            | ReconClearSimilarCells { column_name, .. }
            | ReconJudgeSimilarCells { column_name, .. }
            | ReconUseValuesAsIdentifiers { column_name, .. }
            | ReconJudgeOneCell { column_name, .. }
            | ReconClearOneCell { column_name, .. } => Some(column_name),
            _ => None,
        }
    }

    pub fn in_recipe(&self) -> bool {
        !matches!(self.kind, OpKind::CellEdit { .. } | OpKind::RowAnnotate { .. } | OpKind::ReconJudgeOneCell { .. } | OpKind::ReconClearOneCell { .. })
    }

    /// Default description, in OpenRefine's wording, for operations that arrive without one.
    fn describe(&self) -> String {
        use OpKind::*;
        match &self.kind {
            TextTransform { column_name, expression, .. } => format!("Text transform on cells in column {column_name} using expression {expression}"),
            OpKind::Recon { column_name, config, .. } => {
                let ty = config.get("type").and_then(|t| t.get("id")).and_then(|t| t.as_str()).unwrap_or("null");
                format!("Reconcile cells in column {column_name} to type {ty}")
            }
            ReconMatchBestCandidates { column_name, .. } => format!("Match each cell to its best recon candidate in column {column_name}"),
            ReconMarkNewTopics { column_name, share_new_topics: true, .. } => {
                format!("Mark to create new items for cells in column {column_name}, one item for each group of similar cells")
            }
            ReconMarkNewTopics { column_name, .. } => format!("Mark to create new items for cells in column {column_name}, one item for each cell"),
            ReconMatchSpecificTopic { column_name, r#match, .. } => {
                let s = |k: &str| r#match.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string();
                format!("Match specific item {} ({}) to cells in column {column_name}", s("name"), s("id"))
            }
            ReconDiscardJudgments { column_name, clear_data: true, .. } => format!("Discard recon judgments and clear recon data for cells in column {column_name}"),
            ReconDiscardJudgments { column_name, .. } => format!("Discard recon judgments for cells in column {column_name}"),
            ReconClearSimilarCells { column_name, similar_value, .. } => format!("Clear recon data for cells containing \"{similar_value}\" in column {column_name}"),
            ReconJudgeSimilarCells { column_name, similar_value, judgment, r#match, share_new_topics, .. } => match judgment.as_str() {
                "matched" => {
                    let s = |k: &str| r#match.as_ref().and_then(|m| m.get(k)).and_then(|v| v.as_str()).unwrap_or("").to_string();
                    format!("Match item {} ({}) for cells containing \"{similar_value}\" in column {column_name}", s("name"), s("id"))
                }
                "new" if *share_new_topics => format!("Mark to create one single new item for all cells containing \"{similar_value}\" in column {column_name}"),
                "new" => format!("Mark to create one new item for each cell containing {similar_value} in column {column_name}"),
                _ => format!("Discard recon judgments for cells containing \"{similar_value}\" in column {column_name}"),
            },
            ReconUseValuesAsIdentifiers { column_name, .. } => format!("Use values as reconciliation identifiers in column {column_name}"),
            ReconCopyAcrossColumns { from_column_name, to_column_names, .. } => format!("Copy recon judgments from column {from_column_name} to {}", to_column_names.join(", ")),
            ReconJudgeOneCell { row, column_name, .. } | ReconClearOneCell { row, column_name } => format!("Judge one cell's recon result (row {}, column {column_name})", row + 1),
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
            TransposeColumnsIntoRows { start_column_name, column_count, combined_column_name, key_column_name, value_column_name, .. } => {
                let cols = if *column_count > 0 { format!("{column_count} column(s)") } else { "columns".into() };
                let into = match (combined_column_name, key_column_name, value_column_name) {
                    (Some(c), _, _) => format!("one new column named {c}"),
                    (None, Some(k), Some(v)) => format!("two new columns named {k} and {v}"),
                    _ => "new columns".into(),
                };
                format!("Transpose cells in {cols} starting with {start_column_name} into rows in {into}")
            }
            TransposeRowsIntoColumns { column_name, row_count } => format!("Transpose every {row_count} cells in column {column_name} into separate columns"),
            KeyValueColumnize { key_column_name, value_column_name, note_column_name } => format!(
                "Columnize by key column {key_column_name} and value column {value_column_name} with note column {}",
                note_column_name.as_deref().unwrap_or("")
            ),
            ColumnAdditionByFetchingUrls { new_column_name, column_insert_index, base_column_name, url_expression, .. } => format!(
                "Create column {new_column_name} at index {column_insert_index} by fetching URLs based on column {base_column_name} using expression {url_expression}"
            ),
            CellEdit { row, column_name, .. } => format!("Edit single cell on row {}, column {column_name}", row + 1),
            RowAnnotate { row, starred, flagged } => {
                let what = match (starred, flagged) {
                    (Some(true), _) => "Star",
                    (Some(false), _) => "Unstar",
                    (_, Some(true)) => "Flag",
                    _ => "Unflag",
                };
                format!("{what} row {}", row + 1)
            }
        }
    }

    pub fn apply(&mut self, t: &Table) -> Result<Table> {
        if self.description.is_empty() {
            self.description = self.describe();
        }
        use OpKind::*;
        if matches!(self.kind, ColumnAdditionByFetchingUrls { .. }) {
            return self.fetch_urls(t);
        }
        if matches!(self.kind, OpKind::Recon { .. }) {
            return self.reconcile(t);
        }
        if let Some(out) = self.recon_action(t) {
            return out;
        }
        Ok(match &self.kind {
            TransposeColumnsIntoRows {
                start_column_name, column_count, ignore_blank_cells, fill_down, combined_column_name, prepend_column_name, separator, key_column_name, value_column_name,
            } => {
                let kv = key_column_name.as_deref().zip(value_column_name.as_deref());
                let combined = combined_column_name.as_deref().filter(|c| !c.is_empty());
                crate::reshape::columns_into_rows(t, start_column_name, *column_count, combined, kv, *prepend_column_name, separator.as_deref().unwrap_or(""), *ignore_blank_cells, *fill_down)?
            }
            TransposeRowsIntoColumns { column_name, row_count } => crate::reshape::rows_into_columns(t, column_name, *row_count)?,
            KeyValueColumnize { key_column_name, value_column_name, note_column_name } => {
                crate::reshape::key_value_columnize(t, key_column_name, value_column_name, note_column_name.as_deref().filter(|n| !n.is_empty()))?
            }
            ColumnAdditionByFetchingUrls { .. } | OpKind::Recon { .. } => unreachable!(),
            // the operations on Recons, in recon_action
            ReconMatchBestCandidates { .. }
            | ReconMarkNewTopics { .. }
            | ReconMatchSpecificTopic { .. }
            | ReconDiscardJudgments { .. }
            | ReconClearSimilarCells { .. }
            | ReconJudgeSimilarCells { .. }
            | ReconUseValuesAsIdentifiers { .. }
            | ReconCopyAcrossColumns { .. }
            | ReconJudgeOneCell { .. }
            | ReconClearOneCell { .. } => unreachable!(),
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
                }).keep_recon(&t.columns[ci]));
                out
            }
            FillDown { engine_config, column_name } | BlankDown { engine_config, column_name } => {
                let fill = matches!(self.kind, FillDown { .. });
                let ci = t.col(column_name)?;
                let rows = engine::filter(t, engine_config, None)?;
                let starts = (engine_config.mode == "record-based").then(|| t.record_starts(t.key_index()));
                let mut vals = column_values(t, ci);
                // OpenRefine copies the whole cell down, Recon included, and blanks the cell itself
                let mut recons: crate::recon::Edits = vec![None; t.rows];
                let mut last: Option<(Value, usize)> = None;
                for &r in &rows {
                    let r = r as usize;
                    if starts.as_ref().is_some_and(|s| s[r]) {
                        last = None;
                    }
                    let v = vals[r].clone();
                    if fill {
                        if v.is_blank() {
                            if let Some((l, from)) = &last {
                                vals[r] = l.clone();
                                recons[r] = Some(t.columns[ci].recon(*from).cloned());
                            }
                        } else {
                            last = Some((v, r));
                        }
                    } else if !v.is_blank() && last.as_ref().map(|l| &l.0) == Some(&v) {
                        vals[r] = Value::Null;
                        recons[r] = Some(None);
                    } else {
                        last = Some((v, r));
                    }
                }
                let mut out = set_column(t, ci, vals);
                if t.columns[ci].recon.is_some() {
                    let col = Column { dict: out.columns[ci].dict.clone(), codes: out.columns[ci].codes.clone(), recon: t.columns[ci].recon.clone() };
                    out.columns[ci] = Arc::new(crate::recon::apply_edits(&col, &recons, None, crate::recon::Stats::Keep));
                }
                out
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
                // like OpenRefine, names of columns that aren't there are passed over
                let mut out = t.clone();
                for n in column_names {
                    if let Ok(ci) = out.col(n) {
                        out.names.remove(ci);
                        out.columns.remove(ci);
                    }
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
            CellEdit { row, column_name, value, kind } => {
                let ci = t.col(column_name)?;
                if *row >= t.rows {
                    return Err(format!("no row {}", row + 1));
                }
                let v = typed_cell(value, kind)?;
                let old = &t.columns[ci];
                let mut col = Column { dict: old.dict.clone(), codes: old.codes.clone(), recon: old.recon.clone() };
                let code = col.dict.iter().position(|d| *d == v).unwrap_or_else(|| {
                    col.dict.push(v);
                    col.dict.len() - 1
                });
                Arc::make_mut(&mut col.codes)[*row] = code as u32;
                let mut out = t.clone();
                out.columns[ci] = Arc::new(col);
                out
            }
            RowAnnotate { row, starred, flagged } => {
                if *row >= t.rows {
                    return Err(format!("no row {}", row + 1));
                }
                set_flags(t, &[*row as u32], *starred, *flagged)
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
