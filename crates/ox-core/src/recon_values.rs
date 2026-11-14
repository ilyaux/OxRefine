//! Read-only reconciliation results grouped by the same text used by judge-similar-cells.
use crate::{grel::Value, recon::Recon, table::Table, Result};
use serde_json::{json, Value as J};
use std::{collections::HashMap, sync::Arc};

const PAGE: usize = 20;

struct Variant { row: usize, count: usize, state: usize }
struct Group {
    row: usize,
    value: String,
    count: usize,
    eligible: usize,
    missing: usize,
    types: Vec<(String, usize)>,
    contexts: Vec<J>,
    variants: Vec<Variant>,
    by_state: HashMap<usize, usize>,
}

impl Group {
    fn summary(&self) -> J {
        let reason = if self.eligible == 0 { Some("Blank and error cells cannot be matched.") }
            else if self.missing > 0 { Some("Some matching cells have no reconciliation data. Reconcile them first, or match one cell.") }
            else if self.contexts.len() > 1 { Some("Matching cells use different reconciliation services or identifier spaces. Match individual cells.") }
            else { None };
        json!({ "row": self.row, "value": self.value, "count": self.count, "eligible": self.eligible,
            "missing": self.missing, "typeCounts": self.types, "variantTotal": self.variants.len(),
            "bulkSafe": reason.is_none(), "bulkReason": reason })
    }
}

fn state(r: &Recon) -> J {
    let mut out = r.to_json(true);
    out.as_object_mut().unwrap().retain(|k, _| !matches!(k.as_str(), "id" | "judgmentHistoryEntry"));
    out
}

fn without_candidates(state: &J) -> J {
    state.as_object().map(|m| J::Object(m.iter().filter(|(k, _)| k.as_str() != "c").map(|(k, v)| (k.clone(), v.clone())).collect())).unwrap_or(J::Null)
}

fn kind(value: &Value) -> &'static str {
    match value { Value::Str(_) => "text", Value::Null => "blank", _ => value.type_name() }
}

/// Group only the engine-selected rows. Page both variants and candidates to bound response size.
/// `params.row` and `variantRow` are representative row indices, never a client-provided row set.
pub fn page(t: &Table, column: &str, rows: &[u32], start: usize, limit: usize, params: &J) -> Result<J> {
    let col = &t.columns[t.col(column)?];
    let mut groups: Vec<Group> = vec![];
    let mut by_text: HashMap<(String, String), usize> = HashMap::new();
    let mut by_code: HashMap<u32, usize> = HashMap::new();
    let mut states = vec![J::Null];
    let mut state_ids = HashMap::<String, usize>::new();
    let mut by_recon = HashMap::<usize, usize>::new();
    for &r in rows {
        crate::jobs::tick(0)?;
        let row = r as usize;
        let value = col.get(row);
        let eligible = crate::grel::value::is_non_blank(value);
        let group = *by_code.entry(col.codes[row]).or_insert_with(|| {
            let text = value.to_java_string();
            // Non-matchable cells are separate from genuine text such as "null" or an error message.
            let category = if eligible { "" } else { kind(value) };
            *by_text.entry((text.clone(), category.into())).or_insert_with(|| {
                groups.push(Group { row, value: text, count: 0, eligible: 0, missing: 0, types: vec![], contexts: vec![], variants: vec![], by_state: HashMap::new() });
                groups.len() - 1
            })
        });
        let recon = col.recon(row);
        let state_id = recon.map_or(0, |rc| {
            *by_recon.entry(Arc::as_ptr(rc) as usize).or_insert_with(|| {
                let value = state(rc);
                let key = value.to_string();
                *state_ids.entry(key).or_insert_with(|| { states.push(value); states.len() - 1 })
            })
        });
        let g = &mut groups[group];
        g.count += 1;
        g.eligible += usize::from(eligible);
        if eligible && recon.is_none() { g.missing += 1; }
        if let Some((_, count)) = g.types.iter_mut().find(|(ty, _)| ty == kind(value)) { *count += 1; }
        else { g.types.push((kind(value).into(), 1)); }
        if eligible {
            if let Some(rc) = recon {
                let context = json!([rc.service, rc.identifier_space, rc.schema_space]);
                if g.contexts.len() < 2 && !g.contexts.contains(&context) { g.contexts.push(context); }
            }
        }
        let variant = *g.by_state.entry(state_id).or_insert_with(|| {
            g.variants.push(Variant { row, count: 0, state: state_id });
            g.variants.len() - 1
        });
        g.variants[variant].count += 1;
    }
    let limit = if limit == 0 { PAGE } else { limit.min(100) };
    let start = if groups.is_empty() { 0 } else { start.min((groups.len() - 1) / limit * limit) };
    let selected_row = params["row"].as_u64().map(|n| n as usize);
    let selected = selected_row.and_then(|row| groups.iter().find(|g| g.variants.iter().any(|v| v.row == row)))
        .or_else(|| groups.get(start));
    let detail = selected.map(|g| {
        let mut detail = g.summary();
        let variant_start = params["variantStart"].as_u64().unwrap_or(0) as usize;
        let variant_start = variant_start.min(g.variants.len().saturating_sub(1) / PAGE * PAGE);
        let variants = &g.variants[variant_start..g.variants.len().min(variant_start + PAGE)];
        detail["variantStart"] = json!(variant_start);
        detail["variants"] = json!(variants.iter().map(|v| {
            json!({ "row": v.row, "count": v.count, "recon": without_candidates(&states[v.state]) })
        }).collect::<Vec<_>>());
        let variant_row = params["variantRow"].as_u64().map(|n| n as usize);
        let v = variant_row.and_then(|row| variants.iter().find(|v| v.row == row)).unwrap_or(&variants[0]);
        let mut recon = without_candidates(&states[v.state]);
        let candidates = states[v.state]["c"].as_array().map(Vec::as_slice).unwrap_or(&[]);
        let candidate_start = (params["candidateStart"].as_u64().unwrap_or(0) as usize).min(candidates.len().saturating_sub(1) / PAGE * PAGE);
        if let Some(m) = recon.as_object_mut() { m.insert("c".into(), json!(&candidates[candidate_start..candidates.len().min(candidate_start + PAGE)])); }
        detail["selected"] = json!({ "row": v.row, "count": v.count, "recon": recon,
            "candidateTotal": candidates.len(), "candidateStart": candidate_start });
        detail
    });
    Ok(json!({ "total": groups.len(), "rows": rows.len(), "start": start, "limit": limit,
        "values": groups.iter().skip(start).take(limit).map(Group::summary).collect::<Vec<_>>(), "detail": detail }))
}
