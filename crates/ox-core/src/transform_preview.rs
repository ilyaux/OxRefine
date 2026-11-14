//! Exact, bounded transform previews using the same operation path as Apply.
use crate::{engine, grel::Value, jobs, ops::{Op, OpKind}, table::Table, Result};
use serde_json::{json, Value as Json};
use std::collections::{HashMap, HashSet};

fn cell(value: &Value) -> Json {
    let kind = match value {
        Value::Null => "null",
        Value::Str(_) => "text",
        Value::Int(_) => "integer",
        Value::Num(_) => "number",
        Value::Error(_) => "error",
        value => value.type_name(),
    };
    json!({ "text": value.render(), "type": kind })
}

/// Counts cover all selected rows; only the returned pairs/row examples are limited.
pub(crate) fn preview(t: &Table, mut op: Op, start: usize, limit: usize, only_changed: bool) -> Result<Json> {
    let (column, cfg) = match &op.kind {
        OpKind::TextTransform { column_name, engine_config, .. } => (column_name.clone(), engine_config.clone()),
        _ => return Err("transform preview only accepts core/text-transform".into()),
    };
    let ci = t.col(&column)?;
    let rows = engine::filter(t, &cfg, None)?;
    jobs::progress("Previewing transform", 0);
    jobs::tick(0)?;
    let transformed = op.apply(t)?;
    jobs::tick(0)?;
    let (before, after) = (&t.columns[ci], &transformed.columns[ci]);
    let (mut originals, mut results, mut changed_values) = (HashSet::new(), HashSet::new(), HashSet::new());
    let mut pairs = Vec::<(u32, u32, usize)>::new();
    let mut index = HashMap::new();
    let mut changed_rows = 0;
    let mut row_preview = vec![];
    for &row in &rows {
        if row % 1024 == 0 { jobs::tick(0)?; }
        let row = row as usize;
        let (a, b) = (before.codes[row], after.codes[row]);
        let changed = before.dict[a as usize] != after.dict[b as usize];
        originals.insert(a);
        results.insert(b);
        if changed { changed_rows += 1; changed_values.insert(a); }
        let i = *index.entry((a, b)).or_insert_with(|| {
            pairs.push((a, b, 0));
            pairs.len() - 1
        });
        pairs[i].2 += 1;
        if row_preview.len() < 20 && (!only_changed || changed) {
            row_preview.push(json!({ "before": cell(&before.dict[a as usize]), "after": cell(&after.dict[b as usize]), "changed": changed, "row": row }));
        }
    }
    let multiple_results = pairs.len() > originals.len();
    let matching: Vec<_> = pairs.iter().filter(|&&(a, b, _)| !only_changed || before.dict[a as usize] != after.dict[b as usize]).collect();
    let limit = if limit == 0 { 200 } else { limit.min(200) };
    let items: Vec<_> = matching.iter().skip(start).take(limit).map(|&&(a, b, count)| {
        let (a, b) = (&before.dict[a as usize], &after.dict[b as usize]);
        json!({ "before": cell(a), "after": cell(b), "rows": count, "changed": a != b })
    }).collect();
    Ok(json!({
        "rows": rows.len(), "changedRows": changed_rows,
        "distinctBefore": originals.len(), "distinctAfter": results.len(), "changedValues": changed_values.len(),
        "columnDistinctAfter": after.codes.iter().copied().collect::<HashSet<_>>().len(),
        "multipleResults": multiple_results, "totalPairs": pairs.len(), "matchingPairs": matching.len(),
        "start": start, "items": items, "rowPreview": row_preview,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{dispatch, engine::EngineConfig, table::Column, Project, Session};
    use std::sync::Arc;

    fn table(values: Vec<Value>) -> Table {
        let rows = values.len();
        Table { names: vec!["value".into()], columns: vec![Arc::new(Column::from_values(values))], rows,
            starred: Arc::new(vec![false; rows]), flagged: Arc::new(vec![false; rows]), key: Some("value".into()) }
    }
    fn strings(values: &[&str]) -> Table { table(values.iter().map(|s| Value::Str((*s).into())).collect()) }
    fn op(expression: &str, on_error: &str, repeat: bool, engine: EngineConfig) -> Op {
        serde_json::from_value(json!({ "op": "core/text-transform", "columnName": "value", "expression": expression,
            "onError": on_error, "repeat": repeat, "repeatCount": 10, "engineConfig": engine })).unwrap()
    }

    #[test]
    fn distinct_counts_merges_paging_and_preview_is_read_only() {
        let t = strings(&[" a ", "a", "b", " a "]);
        let operation = op("value.trim()", "keep-original", false, EngineConfig::default());
        let out = preview(&t, operation.clone(), 0, 200, false).unwrap();
        assert_eq!((out["rows"].as_u64(), out["changedRows"].as_u64(), out["changedValues"].as_u64()), (Some(4), Some(2), Some(1)));
        assert_eq!((out["distinctBefore"].as_u64(), out["distinctAfter"].as_u64(), out["columnDistinctAfter"].as_u64()), (Some(3), Some(2), Some(2)));
        assert_eq!(out["items"][0]["rows"], 2);
        assert_eq!(out["items"][0]["before"]["text"], " a ");
        assert_eq!(out["items"][0]["after"]["text"], "a");
        assert_eq!(out["multipleResults"], false);
        let page = preview(&t, operation.clone(), 1, 1, false).unwrap();
        assert_eq!(page["items"].as_array().unwrap().len(), 1);
        assert_eq!(page["items"][0]["before"]["text"], "a");
        let changed = preview(&t, operation.clone(), 0, 200, true).unwrap();
        assert_eq!(changed["matchingPairs"], 1);
        assert_eq!(changed["rowPreview"].as_array().unwrap().len(), 2);
        let mut s = Session { project: Some(Project::new("preview", t)), store: None };
        let before = dispatch(&mut s, "summary", json!({})).unwrap()["data"].clone();
        assert_eq!(dispatch(&mut s, "preview_transform", json!({ "op": operation })).unwrap()["data"], out);
        assert_eq!(dispatch(&mut s, "summary", json!({})).unwrap()["data"], before);
        assert_eq!(s.project.unwrap().table().columns[0].get(0), &Value::Str(" a ".into()));
    }

    #[test]
    fn row_context_and_records_count_every_selected_outcome() {
        let t = strings(&["a", "a", "b", "a"]);
        let out = preview(&t, op("value + row.index", "keep-original", false, EngineConfig::default()), 0, 200, false).unwrap();
        assert_eq!(out["distinctBefore"], 2);
        assert_eq!(out["distinctAfter"], 4);
        assert_eq!(out["totalPairs"], 4);
        assert_eq!(out["multipleResults"], true);
        assert!(out["items"].as_array().unwrap().iter().all(|p| p["rows"] == 1));

        let mut t = strings(&[" x ", "x", " z ", "z"]);
        t.names.insert(0, "key".into());
        t.columns.insert(0, Arc::new(Column::from_values([Value::Str("r1".into()), Value::Null, Value::Str("r2".into()), Value::Null])));
        t.names.push("tag".into());
        t.columns.push(Arc::new(Column::from_values(["keep", "drop", "drop", "drop"].map(|s| Value::Str(s.into())))));
        t.key = Some("key".into());
        let cfg = serde_json::from_value(json!({ "mode": "record-based", "facets": [{ "type": "text", "columnName": "tag", "query": "keep" }] })).unwrap();
        let out = preview(&t, op("value.trim()", "keep-original", false, cfg), 0, 200, false).unwrap();
        assert_eq!(out["rows"], 2);
        assert_eq!(out["changedRows"], 1);
        assert_eq!(out["distinctBefore"], 2);
        assert_eq!(out["distinctAfter"], 1);
        assert_eq!(out["columnDistinctAfter"], 3, "excluded record values remain in the full column");
    }

    #[test]
    fn typed_values_on_error_and_repeat_match_the_applied_transform() {
        let t = table(vec![Value::Str("1".into()), Value::Int(1), Value::Num(1.0), Value::Null, Value::Str("bad".into()), Value::Error("bad".into())]);
        let identity = preview(&t, op("value", "store-error", false, EngineConfig::default()), 0, 200, false).unwrap();
        assert_eq!(identity["distinctBefore"], 6);
        assert_eq!(identity["changedRows"], 0);
        let kinds: Vec<_> = identity["items"].as_array().unwrap().iter().map(|p| p["before"]["type"].as_str().unwrap()).collect();
        assert_eq!(kinds, ["text", "integer", "number", "null", "text", "error"]);
        let t = table(vec![Value::Str("x".into()), Value::Str("2".into()), Value::Str("x".into()), Value::Null]);
        for (policy, changed, distinct) in [("keep-original", 1, 3), ("set-to-blank", 3, 2), ("store-error", 4, 3)] {
            let out = preview(&t, op("value.toNumber()", policy, false, EngineConfig::default()), 0, 200, false).unwrap();
            assert_eq!(out["changedRows"], changed, "{policy}");
            assert_eq!(out["distinctAfter"], distinct, "{policy}");
        }
        let t = strings(&["aaaa", "aa"]);
        for (repeat, distinct) in [(false, 2), (true, 1)] {
            let out = preview(&t, op("value.replace('aa', 'a')", "keep-original", repeat, EngineConfig::default()), 0, 200, false).unwrap();
            assert_eq!(out["distinctAfter"], distinct);
        }
        let bad: Op = serde_json::from_value(json!({ "op": "core/column-removal", "columnName": "value" })).unwrap();
        assert!(preview(&t, bad, 0, 200, false).unwrap_err().contains("only accepts core/text-transform"));
        assert!(preview(&t, op("value.noSuchFunction()", "keep-original", false, EngineConfig::default()), 0, 200, false).unwrap_err().contains("Unknown function"));
        let empty = preview(&strings(&[]), op("value", "keep-original", false, EngineConfig::default()), 0, 200, true).unwrap();
        assert_eq!(empty["rows"], 0);
        assert_eq!(empty["distinctAfter"], 0);
    }
}
