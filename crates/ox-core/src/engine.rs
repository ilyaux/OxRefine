//! Row filtering with OpenRefine's `engineConfig` JSON (list + text facets, row mode).
use crate::{err, table::Table, Result};
use regex::RegexBuilder;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashSet;

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
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Choice {
    pub v: ChoiceValue,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ChoiceValue {
    pub v: Value,
    #[serde(default)]
    pub l: String,
}

pub(crate) fn value() -> String {
    "value".into()
}
fn text() -> String {
    "text".into()
}

/// (column index, pass flag per dict code), or None when the facet selects nothing.
fn compile(t: &Table, f: &Facet) -> Result<Option<(usize, Vec<bool>)>> {
    match f {
        Facet::List { column_name, expression, selection, select_blank, invert, .. } => {
            if selection.is_empty() && !select_blank {
                return Ok(None);
            }
            if expression != "value" {
                return Err(format!("list facet expression `{expression}` is not supported (only `value`)"));
            }
            let ci = t.col(column_name)?;
            let set: HashSet<String> = selection
                .iter()
                .map(|c| match &c.v.v {
                    Value::String(s) => s.clone(),
                    v => v.to_string(),
                })
                .collect();
            let ok = t.columns[ci].dict.iter().map(|s| (set.contains(&**s) || (*select_blank && s.is_empty())) != *invert);
            Ok(Some((ci, ok.collect())))
        }
        Facet::Text { column_name, query, mode, case_sensitive, invert, .. } => {
            if query.is_empty() {
                return Ok(None);
            }
            let ci = t.col(column_name)?;
            let pat = if mode == "regex" { query.clone() } else { regex::escape(query) };
            let re = RegexBuilder::new(&pat).case_insensitive(!case_sensitive).build().map_err(err)?;
            Ok(Some((ci, t.columns[ci].dict.iter().map(|s| re.is_match(s) != *invert).collect())))
        }
    }
}

/// Row indices passing every facet except `skip` (list-facet counts ignore their own facet).
pub fn filter(t: &Table, cfg: &EngineConfig, skip: Option<usize>) -> Result<Vec<u32>> {
    if cfg.mode == "record-based" {
        return Err("records mode is not supported yet; switch to rows".into());
    }
    let mut active = vec![];
    for (i, f) in cfg.facets.iter().enumerate() {
        if Some(i) != skip {
            active.extend(compile(t, f)?);
        }
    }
    let cols: Vec<_> = active.iter().map(|(ci, ok)| (&t.columns[*ci].codes, ok)).collect();
    Ok((0..t.rows as u32).filter(|&r| cols.iter().all(|(codes, ok)| ok[codes[r as usize] as usize])).collect())
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

/// Distinct values of a column over `rows`, most frequent first.
pub fn value_counts<'a>(t: &'a Table, ci: usize, rows: &[u32]) -> Vec<(&'a str, u32)> {
    let col = &*t.columns[ci];
    let mut n = vec![0u32; col.dict.len()];
    for &r in rows {
        n[col.codes[r as usize] as usize] += 1;
    }
    let mut v: Vec<_> = n.iter().enumerate().filter(|(_, &c)| c > 0).map(|(i, &c)| (&*col.dict[i], c)).collect();
    v.sort_unstable_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
    v
}

/// Choices for each list facet (null for text facets), capped at 2000 like OpenRefine.
pub fn facet_counts(t: &Table, cfg: &EngineConfig) -> Result<Vec<Value>> {
    cfg.facets
        .iter()
        .enumerate()
        .map(|(i, f)| match f {
            Facet::List { column_name, .. } => {
                let rows = filter(t, cfg, Some(i))?;
                let vc = value_counts(t, t.col(column_name)?, &rows);
                Ok(json!({ "distinct": vc.len(), "choices": vc.iter().take(2000).collect::<Vec<_>>() }))
            }
            Facet::Text { .. } => Ok(Value::Null),
        })
        .collect()
}
