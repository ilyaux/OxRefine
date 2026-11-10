//! Operations in OpenRefine's operation-history JSON format ("Extract…" / "Apply…").
use crate::{
    engine::{self, EngineConfig},
    grel,
    table::Table,
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

// ponytail: string bridge until columns hold typed values (model-typed)
struct Text(grel::Program);
impl Text {
    fn eval(&self, v: &str) -> String {
        let input = if v.is_empty() { grel::Value::Null } else { grel::Value::Str(v.into()) };
        match self.0.eval(input, &grel::NoRow) {
            grel::Value::Error(_) => v.to_string(),
            out => out.display(),
        }
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
            Op::TextTransform { description, engine_config, column_name, expression, repeat, repeat_count, .. } => {
                let e = Text(grel::compile(expression)?);
                let ci = t.col(column_name)?;
                let sel = engine::mask(t, engine_config)?;
                let times = if *repeat { (*repeat_count).max(1) } else { 1 };
                out.columns[ci] = Arc::new(t.columns[ci].map(sel.as_deref(), false, |v| {
                    let mut s = e.eval(v);
                    for _ in 1..times {
                        let next = e.eval(&s);
                        if next == s {
                            break;
                        }
                        s = next;
                    }
                    s
                }));
                describe(description, || format!("Text transform on cells in column {column_name} using expression {expression}"));
            }
            Op::MassEdit { description, engine_config, column_name, expression, edits } => {
                if expression != "value" {
                    return Err(format!("mass edit expression `{expression}` is not supported (only `value`)"));
                }
                let ci = t.col(column_name)?;
                let sel = engine::mask(t, engine_config)?;
                let map: HashMap<&str, &str> = edits
                    .iter()
                    .flat_map(|e| {
                        let to = e.to.as_str();
                        e.from.iter().map(move |f| (f.as_str(), to)).chain(e.from_blank.then_some(("", to)))
                    })
                    .collect();
                out.columns[ci] = Arc::new(t.columns[ci].map(sel.as_deref(), false, |v| map.get(v).copied().unwrap_or(v).to_string()));
                describe(description, || format!("Mass edit cells in column {column_name}"));
            }
            Op::ColumnAddition { description, engine_config, base_column_name, new_column_name, column_insert_index, expression, .. } => {
                if t.col(new_column_name).is_ok() {
                    return Err(format!("column '{new_column_name}' already exists"));
                }
                let e = Text(grel::compile(expression)?);
                let base = t.col(base_column_name)?;
                let sel = engine::mask(t, engine_config)?;
                let at = (*column_insert_index).min(t.names.len());
                out.columns.insert(at, Arc::new(t.columns[base].map(sel.as_deref(), true, |v| e.eval(v))));
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
