//! Operations in OpenRefine's operation-history JSON format ("Extract…" / "Apply…").
use crate::{
    engine::{self, EngineConfig},
    grel::{self, Value},
    table::{Column, Table},
    Result,
};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, sync::Arc};

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "op")]
pub enum Op {
    #[serde(rename = "core/text-transform", rename_all = "camelCase")]
    TextTransform {
        #[serde(default)]
        description: String,
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
        description: String,
        #[serde(default)]
        engine_config: EngineConfig,
        column_name: String,
        #[serde(default = "engine::value")]
        expression: String,
        edits: Vec<Edit>,
    },
    #[serde(rename = "core/column-addition", rename_all = "camelCase")]
    ColumnAddition {
        #[serde(default)]
        description: String,
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
    #[serde(rename = "core/column-rename", rename_all = "camelCase")]
    ColumnRename {
        #[serde(default)]
        description: String,
        old_column_name: String,
        new_column_name: String,
    },
    #[serde(rename = "core/column-removal", rename_all = "camelCase")]
    ColumnRemoval {
        #[serde(default)]
        description: String,
        column_name: String,
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

fn keep_original() -> String {
    "keep-original".into()
}
fn ten() -> u32 {
    10
}

/// ExpressionUtils.wrapStorable: arrays, JSON and HTML can't live in a cell and become an error.
fn storable(v: Value) -> Value {
    match v {
        Value::Array(_) | Value::Json(_) | Value::Html(_) | Value::Obj(_) | Value::Re(_) => {
            Value::err(format!("{} value not storable", v.type_name()))
        }
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
        col.map_rows(sel, new_column, |r, v| run(p, v, &fallback(v), &t.row(r), on_error, times))
    } else {
        col.map(sel, new_column, |v| run(p, v, &fallback(v), &grel::NoRow, on_error, times))
    }
}

fn describe(d: &mut String, f: impl FnOnce() -> String) {
    if d.is_empty() {
        *d = f();
    }
}

impl Op {
    pub fn description(&self) -> &str {
        match self {
            Op::TextTransform { description, .. }
            | Op::MassEdit { description, .. }
            | Op::ColumnAddition { description, .. }
            | Op::ColumnRename { description, .. }
            | Op::ColumnRemoval { description, .. } => description,
        }
    }

    pub fn apply(&mut self, t: &Table) -> Result<Table> {
        let mut out = t.clone();
        match self {
            Op::TextTransform { description, engine_config, column_name, expression, on_error, repeat, repeat_count } => {
                let p = grel::compile(expression)?;
                let ci = t.col(column_name)?;
                let sel = engine::mask(t, engine_config)?;
                let times = if *repeat { (*repeat_count).max(1) } else { 1 };
                out.columns[ci] = Arc::new(eval_column(t, ci, sel.as_deref(), false, &p, on_error, times));
                describe(description, || format!("Text transform on cells in column {column_name} using expression {expression}"));
            }
            Op::MassEdit { description, engine_config, column_name, expression, edits } => {
                if expression != "value" {
                    return Err(format!("mass edit expression `{expression}` is not supported (only `value`)"));
                }
                let ci = t.col(column_name)?;
                let sel = engine::mask(t, engine_config)?;
                let map: HashMap<&str, &str> = edits.iter().flat_map(|e| e.from.iter().map(move |f| (f.as_str(), e.to.as_str()))).collect();
                let blank_to = edits.iter().find(|e| e.from_blank).map(|e| e.to.as_str());
                out.columns[ci] = Arc::new(t.columns[ci].map(sel.as_deref(), false, |v| {
                    let to = if v.is_blank() { blank_to.or_else(|| map.get("").copied()) } else { map.get(v.display().as_str()).copied() };
                    to.map_or_else(|| v.clone(), |s| Value::Str(s.to_string()))
                }));
                describe(description, || format!("Mass edit cells in column {column_name}"));
            }
            Op::ColumnAddition { description, engine_config, base_column_name, new_column_name, column_insert_index, expression, on_error } => {
                if t.col(new_column_name).is_ok() {
                    return Err(format!("column '{new_column_name}' already exists"));
                }
                let p = grel::compile(expression)?;
                let base = t.col(base_column_name)?;
                let sel = engine::mask(t, engine_config)?;
                let at = (*column_insert_index).min(t.names.len());
                out.columns.insert(at, Arc::new(eval_column(t, base, sel.as_deref(), true, &p, on_error, 1)));
                out.names.insert(at, new_column_name.clone());
                describe(description, || {
                    format!("Create column {new_column_name} at index {at} based on column {base_column_name} using expression {expression}")
                });
            }
            Op::ColumnRename { description, old_column_name, new_column_name } => {
                let ci = t.col(old_column_name)?;
                if t.col(new_column_name).is_ok() {
                    return Err(format!("column '{new_column_name}' already exists"));
                }
                out.names[ci] = new_column_name.clone();
                describe(description, || format!("Rename column {old_column_name} to {new_column_name}"));
            }
            Op::ColumnRemoval { description, column_name } => {
                let ci = t.col(column_name)?;
                out.names.remove(ci);
                out.columns.remove(ci);
                describe(description, || format!("Remove column {column_name}"));
            }
        }
        Ok(out)
    }
}
