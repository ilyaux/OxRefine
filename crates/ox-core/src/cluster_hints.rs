//! Default fingerprint suggestions over selected text values, with dictionary-sized membership bits.
use crate::{cluster, grel::Value, jobs, table::Table, Result};
use serde_json::{json, Value as Json};
use std::collections::HashMap;

#[derive(Default)]
struct ColumnHints {
    marked: Vec<bool>,
    values: usize,
    groups: usize,
    cells: usize,
}

pub(crate) struct Hints {
    columns: Vec<ColumnHints>,
    rows: usize,
}

impl Hints {
    pub(crate) fn build(t: &Table, rows: &[u32]) -> Result<Self> {
        let mut columns = Vec::with_capacity(t.columns.len());
        for col in &t.columns {
            if !col.dict.iter().any(|v| matches!(v, Value::Str(s) if !s.is_empty())) {
                columns.push(ColumnHints::default());
                continue;
            }
            jobs::progress("Finding fingerprint variants", col.dict.len() as u64);
            let mut counts = vec![0usize; col.dict.len()];
            for (i, &row) in rows.iter().enumerate() {
                if i % 4096 == 0 { jobs::tick(0)?; }
                counts[col.codes[row as usize] as usize] += 1;
            }
            let mut hint = ColumnHints { marked: vec![false; col.dict.len()], ..Default::default() };
            // A singleton keeps only its first code; most unique values need no member list.
            let mut keys = HashMap::<String, (usize, usize)>::new();
            for (chunk_no, chunk) in col.dict.chunks(1024).enumerate() {
                for (offset, value) in chunk.iter().enumerate() {
                    let code = chunk_no * 1024 + offset;
                    let Value::Str(text) = value else { continue };
                    if counts[code] == 0 || text.is_empty() { continue; }
                    let (first, distinct) = keys.entry(cluster::fingerprint(text)).or_insert((code, 0));
                    if *distinct == 1 {
                        hint.marked[*first] = true;
                        hint.values += 1;
                        hint.cells += counts[*first];
                        hint.groups += 1;
                    }
                    if *distinct > 0 {
                        hint.marked[code] = true;
                        hint.values += 1;
                        hint.cells += counts[code];
                    }
                    *distinct += 1;
                }
                jobs::tick(chunk.len() as u64)?;
            }
            columns.push(hint);
        }
        Ok(Self { columns, rows: rows.len() })
    }

    pub(crate) fn summary(&self, t: &Table) -> Json {
        json!({ "method": "fingerprint", "textOnly": true, "rows": self.rows,
            "columns": t.names.iter().zip(&self.columns).map(|(column, c)| json!({
                "column": column, "values": c.values, "groups": c.groups, "cells": c.cells,
            })).collect::<Vec<_>>() })
    }

    pub(crate) fn columns_for_row(&self, t: &Table, row: usize) -> Vec<usize> {
        self.columns.iter().enumerate().filter_map(|(i, hint)| {
            hint.marked.get(t.columns[i].codes[row] as usize).copied().unwrap_or(false).then_some(i)
        }).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{dispatch, engine::EngineConfig, ops::Op, table::Column, Project, Session};
    use std::sync::Arc;

    fn table(columns: Vec<(&str, Vec<Value>)>) -> Table {
        let rows = columns.first().map_or(0, |(_, values)| values.len());
        Table { names: columns.iter().map(|(name, _)| (*name).into()).collect(),
            columns: columns.into_iter().map(|(_, values)| Arc::new(Column::from_values(values))).collect(),
            rows, starred: Arc::new(vec![false; rows]), flagged: Arc::new(vec![false; rows]), key: None }
    }

    #[test]
    fn default_fingerprint_hints_match_real_openrefine_clusters() {
        let fixture: Json = serde_json::from_str(include_str!("../../../tests/cluster/fixtures.json")).unwrap();
        let values: Vec<Value> = fixture["values"].as_array().unwrap().iter().map(|v| Value::Str(v.as_str().unwrap().into())).collect();
        let t = table(vec![("v", values)]);
        let hints = Hints::build(&t, &(0..t.rows as u32).collect::<Vec<_>>()).unwrap();
        let clusters = fixture["clusters"][0]["clusters"].as_array().unwrap();
        let members: std::collections::HashSet<&str> = clusters.iter().flat_map(|g| g.as_array().unwrap()).map(|v| v[0].as_str().unwrap()).collect();
        let cells: usize = clusters.iter().flat_map(|g| g.as_array().unwrap()).map(|v| v[1].as_u64().unwrap() as usize).sum();
        let summary = hints.summary(&t);
        assert_eq!(summary["columns"][0]["groups"], clusters.len());
        assert_eq!(summary["columns"][0]["values"], members.len());
        assert_eq!(summary["columns"][0]["cells"], cells);
        for row in 0..t.rows {
            assert_eq!(!hints.columns_for_row(&t, row).is_empty(), members.contains(t.columns[0].get(row).as_str().unwrap()), "row {row}");
        }
    }

    #[test]
    fn repeats_nontext_cells_and_filtered_out_variants_are_not_marked() {
        let t = table(vec![("mixed", vec![Value::Str("A".into()), Value::Str("a".into()), Value::Str("A".into()), Value::Int(1), Value::Str("1".into()), Value::Null, Value::Str("".into()), Value::Error("a".into()), Value::Str(" 1 ".into())])]);
        let hints = Hints::build(&t, &(0..t.rows as u32).collect::<Vec<_>>()).unwrap();
        assert_eq!(hints.summary(&t)["columns"][0], json!({ "column": "mixed", "values": 4, "groups": 2, "cells": 5 }));
        for row in 0..t.rows { assert_eq!(!hints.columns_for_row(&t, row).is_empty(), [0, 1, 2, 4, 8].contains(&row)); }
        let hints = Hints::build(&t, &[0, 2]).unwrap();
        assert_eq!(hints.summary(&t)["columns"][0]["values"], 0);
        assert!(hints.columns_for_row(&t, 0).is_empty(), "identical repeats are not variants");
    }

    #[test]
    fn cache_rows_record_scope_and_history_invalidate_together() {
        let mut t = table(vec![("key", vec![Value::Str("one".into()), Value::Null, Value::Str("two".into()), Value::Null]),
            ("value", ["A", "a", "B", "b"].map(|s| Value::Str(s.into())).to_vec()),
            ("tag", ["keep", "drop", "drop", "drop"].map(|s| Value::Str(s.into())).to_vec())]);
        t.key = Some("key".into());
        let mut p = Project::new("hints", t);
        let cfg: EngineConfig = serde_json::from_value(json!({ "mode": "record-based", "facets": [{ "type": "text", "columnName": "tag", "query": "keep" }] })).unwrap();
        let cached = p.cluster_hints(&cfg).unwrap();
        assert!(Arc::ptr_eq(&cached, &p.cluster_hints(&cfg).unwrap()), "paging uses the cached dictionary membership");
        assert_eq!(cached.summary(p.table())["columns"][1]["cells"], 2);
        let row_cfg = EngineConfig { mode: "row-based".into(), facets: cfg.facets.clone() };
        assert_eq!(p.cluster_hints(&row_cfg).unwrap().summary(p.table())["columns"][1]["values"], 0);
        let op: Op = serde_json::from_value(json!({ "op": "core/text-transform", "columnName": "value", "expression": "grel:value.toLowercase()" })).unwrap();
        p.apply(op).unwrap();
        assert_eq!(p.cluster_hints(&cfg).unwrap().summary(p.table())["columns"][1]["values"], 0);
        p.goto(0).unwrap();
        let cached = p.cluster_hints(&cfg).unwrap();
        assert_eq!(cached.summary(p.table())["columns"][1]["values"], 2);
        let mut session = Session { project: Some(p), store: None };
        let plain = dispatch(&mut session, "rows", json!({ "engine": cfg, "limit": 2 })).unwrap();
        assert_eq!(plain["data"]["rows"][0].as_array().unwrap().len(), 6);
        let flagged = dispatch(&mut session, "rows", json!({ "engine": cfg, "limit": 2, "params": { "clusterHints": true } })).unwrap();
        assert_eq!(flagged["data"]["rows"][0][7], json!([1]));
        assert_eq!(flagged["data"]["rows"][1][7], json!([1]));
        let page = dispatch(&mut session, "rows", json!({ "engine": cfg, "start": 1, "limit": 1, "params": { "clusterHints": true } })).unwrap();
        assert_eq!(page["data"]["rows"][0][7], json!([1]));
        assert!(Arc::ptr_eq(&cached, &session.project.as_mut().unwrap().cluster_hints(&cfg).unwrap()), "paged rows reuse the same hint cache");
        assert_eq!(session.project.unwrap().steps().len(), 1, "hints never add history");
    }
}
