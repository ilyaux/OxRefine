use ox_core::{dispatch, engine::{self, EngineConfig}, grel::Value, ops::Op, recon::{Candidate, Judgment, Recon, ReconCol}, recon_values, table::{Column, Table}, Project, Session};
use serde_json::{json, Value as J};
use std::sync::Arc;

fn answer() -> Recon {
    let fixtures: J = serde_json::from_str(include_str!("../../../tests/recon/fixtures.json")).unwrap();
    assert_eq!(fixtures["openrefine"], "3.10.1");
    let source = &fixtures["cases"][0]["rows"][4]["cells"][0]["r"];
    assert_eq!(source["j"], "none");
    assert!(!source["c"].as_array().unwrap().is_empty());
    Recon::from_json(source)
}

fn table(values: Vec<Value>, recons: Vec<Option<Recon>>) -> Table {
    let rows = values.len();
    let mut column = Column::from_values(values);
    column.recon = Some(Arc::new(ReconCol::new(recons.into_iter().map(|r| r.map(Arc::new)).collect())));
    Table { names: vec!["name".into()], columns: vec![Arc::new(column)], rows, starred: Arc::new(vec![false; rows]), flagged: Arc::new(vec![false; rows]), key: None }
}

fn page(t: &Table, params: J) -> J {
    recon_values::page(t, "name", &(0..t.rows as u32).collect::<Vec<_>>(), 0, 20, &params).unwrap()
}

#[test]
fn same_text_groups_show_types_and_preserve_conflicting_results() {
    let base = answer();
    let mut volatile = base.clone();
    volatile.id += 17;
    volatile.history_entry += 3;
    let values = vec![Value::Str("1".into()), Value::Int(1), Value::Str("1".into()), Value::Null, Value::Str("null".into()), Value::Error("bad".into()), Value::Str("bad".into())];
    let recons = vec![Some(base.clone()), Some(volatile.clone()), Some(base.clone()), None, Some(base.clone()), None, Some(base.clone())];
    let t = table(values.clone(), recons.clone());
    let out = page(&t, J::Null);
    let group = &out["values"][0];
    assert_eq!(group["count"], 3);
    assert_eq!(group["typeCounts"], json!([["text", 2], ["number", 1]]));
    assert_eq!(group["variantTotal"], 1, "volatile ids/history entries do not invent conflicts");
    assert_eq!(group["bulkSafe"], true);
    assert_eq!(out["total"], 5, "blanks/errors are separate from real text with the same spelling");
    assert_eq!(out["values"][1]["bulkSafe"], false);
    let candidate = out["detail"]["selected"]["recon"]["c"][0].clone();
    let mut p = Project::new("typed matching", t.clone());
    p.apply(serde_json::from_value(json!({ "op": "core/recon-judge-similar-cells", "columnName": "name", "similarValue": "1", "judgment": "matched", "match": candidate })).unwrap()).unwrap();
    assert!((0..3).all(|row| p.table().columns[0].recon(row).unwrap().judgment == Judgment::Matched), "shown Match all count includes number 1 and text 1, as the existing operation does");
    assert_eq!(p.table().columns[0].recon(4).unwrap().judgment, Judgment::None);

    let mut different = base.clone();
    different.features.name_match = Some(true);
    let mut changed = recons.clone();
    changed[2] = Some(different);
    let out = page(&table(values.clone(), changed), J::Null);
    assert_eq!(out["values"][0]["variantTotal"], 2, "features remain observable as a different state");
    assert_eq!(out["detail"]["variants"][1]["recon"]["f"][1], true);

    volatile.service = "http://other.example/reconcile".into();
    let mut changed = recons;
    changed[1] = Some(volatile);
    let out = page(&table(values, changed), J::Null);
    assert_eq!(out["values"][0]["bulkSafe"], false);
    assert!(out["values"][0]["bulkReason"].as_str().unwrap().contains("different reconciliation services"));
    assert_eq!(out["detail"]["variantTotal"], 2);
}

#[test]
fn result_states_and_candidate_pages_are_bounded_without_losing_totals() {
    let base = answer();
    let candidates: Vec<_> = (0..45).map(|i| Candidate { id: format!("Q{i}"), name: format!("Candidate {i}"), types: vec![], score: i as f64 }).collect();
    let values = vec![Value::Str("Paris".into()); 45];
    let recons = (0..45).map(|i| {
        let mut r = base.clone();
        r.features.name_levenshtein = Some(i);
        r.candidates = Some(candidates.clone());
        Some(r)
    }).collect();
    let t = table(values, recons);
    let out = page(&t, json!({ "row": 0, "variantStart": 20, "variantRow": 23, "candidateStart": 40 }));
    let detail = &out["detail"];
    assert_eq!(detail["variantTotal"], 45);
    assert_eq!(detail["variantStart"], 20);
    assert_eq!(detail["variants"].as_array().unwrap().len(), 20);
    assert!(detail["variants"][0]["recon"].get("c").is_none());
    assert_eq!(detail["selected"]["row"], 23);
    assert_eq!(detail["selected"]["candidateTotal"], 45);
    assert_eq!(detail["selected"]["candidateStart"], 40);
    assert_eq!(detail["selected"]["recon"]["c"].as_array().unwrap().len(), 5);
    assert_eq!(detail["selected"]["recon"]["c"][0]["id"], "Q40");
    let last = page(&t, json!({ "variantStart": u64::MAX, "candidateStart": u64::MAX }));
    assert_eq!(last["detail"]["variants"].as_array().unwrap().len(), 5);
    assert_eq!(last["detail"]["selected"]["recon"]["c"].as_array().unwrap().len(), 5);
    assert!(page(&t, J::Null)["detail"]["selected"]["recon"]["c"].to_string().len() < 3000);
}

#[test]
fn existing_cell_and_bulk_judgments_follow_the_shown_scope_and_history() {
    let base = answer();
    let values = vec![Value::Str("Paris".into()), Value::Str("Paris".into()), Value::Str("Paris".into()), Value::Str("Other".into())];
    let mut t = table(values, vec![Some(base.clone()); 4]);
    t.names.insert(0, "key".into());
    t.columns.insert(0, Arc::new(Column::from_values(vec![Value::Str("record1".into()), Value::Null, Value::Str("record2".into()), Value::Null])));
    let cfg: EngineConfig = serde_json::from_value(json!({ "mode": "record-based", "facets": [{ "type": "text", "columnName": "key", "query": "record1", "mode": "text" }] })).unwrap();
    let rows = engine::filter(&t, &cfg, None).unwrap();
    assert_eq!(rows, vec![0, 1]);
    let out = recon_values::page(&t, "name", &rows, 0, 20, &J::Null).unwrap();
    assert_eq!(out["values"][0]["eligible"], 2);
    let candidate = out["detail"]["selected"]["recon"]["c"][0].clone();
    let mut p = Project::new("test", t);
    let op = |v| serde_json::from_value::<Op>(v).unwrap();
    p.apply(op(json!({ "op": "ox/recon-judge-one-cell", "row": 0, "columnName": "name", "judgment": "matched", "match": candidate }))).unwrap();
    assert_eq!(p.table().columns[1].recon(0).unwrap().judgment, Judgment::Matched);
    assert_eq!(p.table().columns[1].recon(1).unwrap().judgment, Judgment::None);
    p.goto(0).unwrap();
    p.apply(op(json!({ "op": "core/recon-judge-similar-cells", "engineConfig": cfg, "columnName": "name", "similarValue": "Paris", "judgment": "matched", "match": candidate }))).unwrap();
    assert_eq!(p.table().columns[1].recon(0).unwrap().judgment, Judgment::Matched);
    assert_eq!(p.table().columns[1].recon(1).unwrap().judgment, Judgment::Matched);
    assert_eq!(p.table().columns[1].recon(2).unwrap().judgment, Judgment::None);
    p.goto(0).unwrap();
    assert_eq!(p.table().columns[1].recon(0).unwrap().judgment, Judgment::None);
    p.goto(1).unwrap();
    assert_eq!(p.table().columns[1].recon(1).unwrap().matched.as_ref().unwrap().id, candidate["id"]);
    let mut s = Session { project: Some(p), store: None };
    let before = s.project.as_ref().unwrap().steps().len();
    let result = dispatch(&mut s, "recon_values", json!({ "column": "name", "engine": cfg, "limit": 20 })).unwrap();
    assert_eq!(result["data"]["rows"], 2);
    assert_eq!(s.project.as_ref().unwrap().steps().len(), before, "displaying results is read-only and does not call the recorded service");
}

#[test]
fn missing_recons_and_empty_facets_do_not_offer_unsafe_bulk_matching() {
    let t = table(vec![Value::Str("Paris".into()); 2], vec![Some(answer()), None]);
    let out = page(&t, J::Null);
    assert_eq!(out["values"][0]["count"], 2);
    assert_eq!(out["values"][0]["missing"], 1);
    assert_eq!(out["values"][0]["bulkSafe"], false);
    let empty = recon_values::page(&t, "name", &[], 99, 0, &J::Null).unwrap();
    assert_eq!(empty["total"], 0);
    assert_eq!(empty["detail"], J::Null);
    assert_eq!(empty["values"], json!([]));
    assert!(recon_values::page(&t, "missing", &[0], 0, 20, &J::Null).unwrap_err().contains("no column"));
}
