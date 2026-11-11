//! Reconciliation, as OpenRefine 3.10.1 does it: the Recon object a cell can carry, and the
//! standard-service client (W3C Reconciliation API) behind the `core/recon` operation.
use crate::{
    grel::{value::java_double, Value},
    table::{Column, Table},
    Result,
};
use serde_json::{json, Map, Value as J};
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq)]
pub struct Candidate {
    pub id: String,
    pub name: String,
    pub types: Vec<String>,
    pub score: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Judgment {
    None,
    Matched,
    New,
    Error,
}

/// Recon.features: typeMatch, nameMatch, nameLevenshtein, nameWordDistance (null until computed).
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Features {
    pub type_match: Option<bool>,
    pub name_match: Option<bool>,
    pub name_levenshtein: Option<i64>,
    pub name_word_distance: Option<f64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Recon {
    pub id: i64,
    pub history_entry: i64,
    pub service: String,
    pub identifier_space: Option<String>,
    pub schema_space: Option<String>,
    pub features: Features,
    /// None until a candidate is added (GREL sees null, JSON an empty list)
    pub candidates: Option<Vec<Candidate>>,
    pub judgment: Judgment,
    pub judgment_action: String,
    pub batch_size: i64,
    pub matched: Option<Candidate>,
    pub error: Option<String>,
    pub match_rank: i64,
}

/// Recon ids are random longs in OpenRefine, so ids from separate operations never collide.
pub fn new_id() -> i64 {
    use std::hash::{BuildHasher, Hasher};
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u64(N.fetch_add(1, std::sync::atomic::Ordering::Relaxed));
    h.finish() as i64
}

impl Recon {
    pub fn new(history_entry: i64, identifier_space: &str, schema_space: &str) -> Recon {
        Recon {
            id: new_id(),
            history_entry,
            service: "unknown".into(),
            identifier_space: Some(identifier_space.into()),
            schema_space: Some(schema_space.into()),
            features: Features::default(),
            candidates: None,
            judgment: Judgment::None,
            judgment_action: "unknown".into(),
            batch_size: 0,
            matched: None,
            error: None,
            match_rank: -1,
        }
    }

    fn error(history_entry: i64, cfg: &StandardConfig, msg: impl Into<String>) -> Recon {
        let mut r = Recon::new(history_entry, &cfg.identifier_space, &cfg.schema_space);
        r.error = Some(msg.into());
        r.judgment = Judgment::Error;
        r
    }

    /// A Recon with no identifier or schema space (OpenRefine's `new Recon(id, null, null)`).
    pub fn bare(history_entry: i64) -> Recon {
        Recon { identifier_space: None, schema_space: None, ..Recon::new(history_entry, "", "") }
    }

    /// Recon.dup: the same judgment data under a new id, for a new history step.
    pub fn dup(&self, history_entry: i64) -> Recon {
        Recon { id: new_id(), history_entry, ..self.clone() }
    }

    pub fn best(&self) -> Option<&Candidate> {
        self.candidates.as_ref().and_then(|c| c.first())
    }

    /// OpenRefine's JSON: `save` is the project-file form (every field); otherwise what get-rows
    /// shows, where candidates appear only on cells still to be judged.
    pub fn to_json(&self, save: bool) -> J {
        let mut m = Map::new();
        m.insert("id".into(), json!(self.id));
        if save {
            m.insert("judgmentHistoryEntry".into(), json!(self.history_entry));
        }
        m.insert("service".into(), json!(self.service));
        m.insert("identifierSpace".into(), json!(self.identifier_space));
        m.insert("schemaSpace".into(), json!(self.schema_space));
        m.insert("j".into(), json!(judgment_json(self.judgment)));
        if let Some(c) = &self.matched {
            m.insert("m".into(), candidate_json(c));
        }
        if let Some(e) = &self.error {
            m.insert("e".into(), json!(e));
        }
        if save || self.judgment == Judgment::None {
            m.insert("c".into(), J::Array(self.candidates.iter().flatten().map(candidate_json).collect()));
        }
        if save {
            let f = &self.features;
            m.insert("f".into(), json!([f.type_match, f.name_match, f.name_levenshtein, f.name_word_distance]));
            m.insert("judgmentAction".into(), json!(self.judgment_action));
            m.insert("judgmentBatchSize".into(), json!(self.batch_size));
            if self.matched.is_some() {
                m.insert("matchRank".into(), json!(self.match_rank));
            }
        }
        J::Object(m)
    }

    pub fn from_json(j: &J) -> Recon {
        let s = |k: &str| j.get(k).and_then(J::as_str).map(String::from);
        let f = j.get("f").and_then(J::as_array).cloned().unwrap_or_default();
        let at = |i: usize| f.get(i).filter(|v| !v.is_null());
        Recon {
            id: j.get("id").and_then(as_long).unwrap_or_else(new_id),
            history_entry: j.get("judgmentHistoryEntry").and_then(as_long).unwrap_or(0),
            service: s("service").unwrap_or_else(|| "unknown".into()),
            identifier_space: s("identifierSpace"),
            schema_space: s("schemaSpace"),
            features: Features {
                type_match: at(0).and_then(J::as_bool),
                name_match: at(1).and_then(J::as_bool),
                name_levenshtein: at(2).and_then(J::as_i64),
                name_word_distance: at(3).and_then(J::as_f64),
            },
            candidates: j.get("c").and_then(J::as_array).filter(|c| !c.is_empty()).map(|c| c.iter().map(candidate_from_json).collect()),
            judgment: match j.get("j").and_then(J::as_str).unwrap_or("none") {
                "matched" => Judgment::Matched,
                "new" => Judgment::New,
                "error" => Judgment::Error,
                _ => Judgment::None,
            },
            judgment_action: s("judgmentAction").unwrap_or_else(|| "unknown".into()),
            batch_size: j.get("judgmentBatchSize").and_then(J::as_i64).unwrap_or(0),
            matched: j.get("m").filter(|m| m.is_object()).map(candidate_from_json),
            error: s("e"),
            match_rank: j.get("matchRank").and_then(J::as_i64).unwrap_or(-1),
        }
    }

    /// `cell.recon.<name>` in GREL.
    pub fn field(self: &Arc<Self>, name: &str) -> Value {
        let s = |v: &str| Value::Str(v.into());
        match name {
            "id" => Value::Int(self.id),
            "best" => self.best().map_or(Value::Null, |c| View::Candidate(c.clone()).into()),
            "candidates" => self.candidates.as_ref().map_or(Value::Null, |c| Value::Array(c.iter().map(|c| View::Candidate(c.clone()).into()).collect())),
            "judgment" | "judgement" => s(match self.judgment {
                Judgment::None => "none",
                Judgment::Matched => "matched",
                Judgment::New => "new",
                Judgment::Error => "errors",
            }),
            "judgmentAction" | "judgementAction" => s(&self.judgment_action),
            "judgmentHistoryEntry" | "judgementHistoryEntry" => Value::Int(self.history_entry),
            "judgmentBatchSize" | "judgementBatchSize" => Value::Int(self.batch_size),
            "matched" => Value::Bool(self.judgment == Judgment::Matched),
            "new" => Value::Bool(self.judgment == Judgment::New),
            "match" => self.matched.as_ref().map_or(Value::Null, |c| View::Candidate(c.clone()).into()),
            "error" => self.error.as_deref().map_or(Value::Null, s),
            "matchRank" => Value::Int(self.match_rank),
            "features" => View::Features(self.clone()).into(),
            "service" => s(&self.service),
            "identifierSpace" => self.identifier_space.as_deref().map_or(Value::Null, s),
            "schemaSpace" => self.schema_space.as_deref().map_or(Value::Null, s),
            _ => Value::Null,
        }
    }
}

fn as_long(v: &J) -> Option<i64> {
    v.as_i64().or_else(|| v.as_str().and_then(|s| s.parse().ok()))
}

/// Recon.stringToJudgment.
pub fn judgment_of(s: &str) -> Judgment {
    match s {
        "matched" => Judgment::Matched,
        "new" => Judgment::New,
        "error" => Judgment::Error,
        _ => Judgment::None,
    }
}

fn judgment_json(j: Judgment) -> &'static str {
    match j {
        Judgment::None => "none",
        Judgment::Matched => "matched",
        Judgment::New => "new",
        Judgment::Error => "error",
    }
}

fn candidate_json(c: &Candidate) -> J {
    json!({ "id": c.id, "name": c.name, "types": c.types, "score": c.score })
}

pub fn candidate_from_json(j: &J) -> Candidate {
    Candidate {
        id: j.get("id").and_then(J::as_str).unwrap_or_default().into(),
        name: j.get("name").and_then(J::as_str).unwrap_or_default().into(),
        types: j.get("types").and_then(J::as_array).map(|t| t.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default(),
        score: j.get("score").and_then(J::as_f64).unwrap_or(0.0),
    }
}

/// Recon objects as GREL values: what `cell.recon`, `.match`/`.best`/`.candidates[i]` and `.features` give.
#[derive(Clone, Debug, PartialEq)]
pub enum View {
    Recon(Arc<Recon>),
    Candidate(Candidate),
    Features(Arc<Recon>),
}

impl From<View> for Value {
    fn from(v: View) -> Value {
        Value::Recon(Box::new(v))
    }
}

impl View {
    /// GREL type(): OpenRefine reports the Java class.
    pub fn type_name(&self) -> &'static str {
        match self {
            View::Recon(_) => "com.google.refine.model.Recon",
            View::Candidate(_) => "com.google.refine.model.ReconCandidate",
            View::Features(_) => "com.google.refine.model.Recon$Features",
        }
    }
    pub fn field(&self, name: &str) -> Value {
        match self {
            View::Recon(r) => r.field(name),
            View::Candidate(c) => match name {
                "id" => Value::Str(c.id.clone()),
                "name" => Value::Str(c.name.clone()),
                "type" => Value::Array(c.types.iter().map(|t| Value::Str(t.clone())).collect()),
                "score" => Value::Num(c.score),
                _ => Value::Null,
            },
            View::Features(r) => {
                let f = &r.features;
                match name {
                    "typeMatch" => f.type_match.map_or(Value::Null, Value::Bool),
                    "nameMatch" => f.name_match.map_or(Value::Null, Value::Bool),
                    "nameLevenshtein" => f.name_levenshtein.map_or(Value::Null, Value::Int),
                    "nameWordDistance" => f.name_word_distance.map_or(Value::Null, Value::Num),
                    _ => Value::Null,
                }
            }
        }
    }
    pub fn to_java_string(&self) -> String {
        match self {
            View::Recon(r) => format!("Recon [id={}, judgment={}, match={}]", r.id, judgment_json(r.judgment), r.matched.as_ref().map_or("null", |c| c.id.as_str())),
            View::Candidate(c) => format!("ReconCandidate [id={}, name={}, score={}]", c.id, c.name, java_double(c.score)),
            View::Features(_) => "Features".into(),
        }
    }
}

/// A column's reconciliation layer: each row's Recon (cells with equal queries share one), and the
/// column's reconConfig and reconStats as the last step on Recons left them. OpenRefine keeps the
/// stats rather than counting: other steps (copying Recons across columns, removing rows) leave them
/// as they were.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ReconCol {
    pub cells: Vec<Option<Arc<Recon>>>,
    pub config: Option<J>,
    pub stats: Option<J>,
}

/// What a step does to a column's reconStats.
pub enum Stats {
    Keep,
    /// count them on the new cells (ReconStats.create)
    Count,
    Set(J),
}

impl ReconCol {
    pub fn new(cells: Vec<Option<Arc<Recon>>>) -> ReconCol {
        ReconCol { cells, config: None, stats: None }
    }

    /// The stats as kept, else counted.
    pub fn stats(&self, col: &Column) -> J {
        self.stats.clone().unwrap_or_else(|| self.count(col))
    }

    /// ReconStats.create: the column's cells counted by judgment.
    pub fn count(&self, col: &Column) -> J {
        let (mut non_blanks, mut new, mut matched, mut errors) = (0, 0, 0, 0);
        for (r, recon) in self.cells.iter().enumerate() {
            if !crate::grel::value::is_non_blank(col.get(r)) {
                continue;
            }
            non_blanks += 1;
            match recon.as_ref().map(|r| r.judgment) {
                Some(Judgment::New) => new += 1,
                Some(Judgment::Matched) => matched += 1,
                Some(Judgment::Error) => errors += 1,
                _ => {}
            }
        }
        json!({ "nonBlanks": non_blanks, "newTopics": new, "matchedTopics": matched, "errorTopics": errors })
    }
}

/// Row-by-row Recons as JSON: each distinct Recon once (project-file form), then per row its
/// index or -1. Cells that shared a Recon share it again after `load_cells`.
pub fn save_cells(cells: &[Option<Arc<Recon>>]) -> J {
    let mut seen: std::collections::HashMap<*const Recon, usize> = Default::default();
    let mut recons = vec![];
    let rows: Vec<i64> = cells
        .iter()
        .map(|c| match c {
            None => -1,
            Some(r) => *seen.entry(Arc::as_ptr(r)).or_insert_with(|| {
                recons.push(r.to_json(true));
                recons.len() - 1
            }) as i64,
        })
        .collect();
    json!({ "recons": recons, "rows": rows })
}

pub fn load_cells(j: &J, rows: usize) -> Option<Vec<Option<Arc<Recon>>>> {
    let recons: Vec<Arc<Recon>> = j.get("recons")?.as_array()?.iter().map(|r| Arc::new(Recon::from_json(r))).collect();
    let idx = j.get("rows")?.as_array()?;
    if idx.len() != rows {
        return None;
    }
    Some(idx.iter().map(|i| i.as_i64().and_then(|i| recons.get(i as usize).cloned())).collect())
}

// ---------------------------------------------------------------- the standard service

const DEFAULT_IDENTIFIER_SPACE: &str = "http://localhost/identifier";
const DEFAULT_SCHEMA_SPACE: &str = "http://localhost/schema";
const STOP_WORDS: [&str; 8] = ["the", "a", "and", "of", "on", "in", "at", "by"];

#[derive(Clone, Debug, PartialEq)]
pub struct ColumnDetail {
    pub column: String,
    pub property_name: String,
    pub property_id: String,
}

/// StandardReconConfig.
#[derive(Clone, Debug, PartialEq)]
pub struct StandardConfig {
    pub service: String,
    pub identifier_space: String,
    pub schema_space: String,
    pub type_id: Option<String>,
    pub type_name: Option<String>,
    pub auto_match: bool,
    pub batch_size: Option<i64>,
    pub columns: Vec<ColumnDetail>,
    pub limit: i64,
}

impl StandardConfig {
    pub fn from_json(j: &J) -> Result<StandardConfig> {
        match j.get("mode").and_then(J::as_str) {
            Some("standard-service") | None => {}
            Some(m) => return Err(format!("reconciliation mode `{m}` is not supported yet")),
        }
        let s = |k: &str| j.get(k).and_then(J::as_str).map(String::from);
        let ty = j.get("type").filter(|t| t.is_object());
        Ok(StandardConfig {
            service: s("service").ok_or("the reconciliation config has no service")?,
            identifier_space: s("identifierSpace").unwrap_or_else(|| DEFAULT_IDENTIFIER_SPACE.into()),
            schema_space: s("schemaSpace").unwrap_or_else(|| DEFAULT_SCHEMA_SPACE.into()),
            type_id: ty.and_then(|t| t.get("id")).and_then(J::as_str).map(String::from),
            type_name: ty.and_then(|t| t.get("name")).and_then(J::as_str).map(String::from),
            auto_match: j.get("autoMatch").and_then(J::as_bool).unwrap_or(true),
            batch_size: j.get("batchSize").and_then(J::as_i64),
            columns: j
                .get("columnDetails")
                .and_then(J::as_array)
                .map(|a| {
                    a.iter()
                        .map(|d| {
                            let p = |k: &str| d.get(k).and_then(J::as_str).map(String::from);
                            let prop = d.get("property");
                            ColumnDetail {
                                column: p("column").unwrap_or_default(),
                                property_name: p("propertyName").or_else(|| prop.and_then(|x| x.get("name")).and_then(J::as_str).map(String::from)).unwrap_or_default(),
                                property_id: p("propertyID").or_else(|| prop.and_then(|x| x.get("id")).and_then(J::as_str).map(String::from)).unwrap_or_default(),
                            }
                        })
                        .collect()
                })
                .unwrap_or_default(),
            limit: j.get("limit").and_then(J::as_i64).unwrap_or(0),
        })
    }

    /// The config as OpenRefine writes it (column reconConfig, operation JSON).
    pub fn to_json(&self) -> J {
        let mut m = Map::new();
        m.insert("mode".into(), json!("standard-service"));
        m.insert("service".into(), json!(self.service));
        m.insert("identifierSpace".into(), json!(self.identifier_space));
        m.insert("schemaSpace".into(), json!(self.schema_space));
        if let Some(id) = &self.type_id {
            m.insert("type".into(), json!({ "id": id, "name": self.type_name }));
        }
        m.insert("autoMatch".into(), json!(self.auto_match));
        if let Some(b) = self.batch_size {
            m.insert("batchSize".into(), json!(b));
        }
        m.insert(
            "columnDetails".into(),
            J::Array(self.columns.iter().map(|c| json!({ "column": c.column, "propertyName": c.property_name, "propertyID": c.property_id })).collect()),
        );
        m.insert("limit".into(), json!(self.limit));
        J::Object(m)
    }

    /// createNewRecon: an empty Recon for this service.
    pub fn new_recon(&self, history_entry: i64) -> Recon {
        Recon { service: self.service.clone(), ..Recon::new(history_entry, &self.identifier_space, &self.schema_space) }
    }

    /// The config OpenRefine makes for a column marked or matched without reconciling first.
    pub fn bare(service: &str, identifier_space: &str, schema_space: &str, auto_match: bool) -> StandardConfig {
        StandardConfig {
            service: service.into(),
            identifier_space: identifier_space.into(),
            schema_space: schema_space.into(),
            type_id: None,
            type_name: None,
            auto_match,
            batch_size: None,
            columns: vec![],
            limit: 0,
        }
    }

    fn batch_size(&self, rows: usize) -> usize {
        match self.batch_size {
            None => 10,
            Some(b) => ((rows / 10).max(10) as i64).min(b).max(1) as usize,
        }
    }

    /// The query for one cell, serialized exactly as Jackson writes StandardReconConfig.ReconQuery:
    /// OpenRefine groups cells by this text's hash, so equal text means one request and one Recon.
    fn query(&self, t: &Table, row: usize, text: &str) -> Result<String> {
        let mut props = vec![];
        for d in &self.columns {
            let ci = t.col(&d.column)?;
            let col = &t.columns[ci];
            // ponytail: OpenRefine also looks the cell up in the record's other rows (cellDependencies)
            let v = col.get(row);
            if !crate::grel::value::is_non_blank(v) {
                continue;
            }
            let matched = col.recon.as_ref().and_then(|rc| rc.cells[row].as_ref()).and_then(|r| r.matched.as_ref());
            let v = match matched {
                // a HashMap of id and name: Java iterates it as name, id
                Some(m) => format!("{{\"name\":{},\"id\":{}}}", json_str(&m.name), json_str(&m.id)),
                None => jackson(v),
            };
            props.push(format!("{{\"pid\":{},\"v\":{v}}}", json_str(&d.property_id)));
        }
        let mut q = format!("{{\"query\":{}", json_str(text));
        if let Some(ty) = &self.type_id {
            q += &format!(",\"type\":{}", json_str(ty));
        }
        if !props.is_empty() {
            q += &format!(",\"properties\":[{}]", props.join(","));
        }
        if self.limit != 0 {
            q += &format!(",\"limit\":{}", self.limit);
        }
        if self.type_id.is_some() {
            q += ",\"type_strict\":\"should\"";
        }
        Ok(q + "}")
    }

    /// batchRecon: one POST for the jobs, one Recon per job (in order), errors included.
    fn batch(&self, agent: &ureq::Agent, jobs: &[(&str, &str)], history_entry: i64) -> Vec<Recon> {
        let body = format!("{{{}}}", jobs.iter().enumerate().map(|(i, (_, code))| format!("\"q{i}\":{code}")).collect::<Vec<_>>().join(","));
        let mut out = vec![];
        let response = match post(agent, &self.service, &body) {
            Ok(text) => serde_json::from_str::<J>(&text).map_err(|e| e.to_string()),
            Err(e) => Err(e),
        };
        let parsed = match response {
            Ok(J::Object(m)) => Some(m),
            Ok(J::Null) => None,
            Ok(other) => {
                out.push(Recon::error(history_entry, self, format!("Cannot deserialize value of type `ObjectNode` from {}", json_kind(&other))));
                None
            }
            Err(e) => {
                out.push(Recon::error(history_entry, self, e));
                None
            }
        };
        match parsed {
            None => out.push(Recon::error(history_entry, self, "The reconciliation service returned an invalid response")),
            Some(m) => {
                for (i, (text, _)) in jobs.iter().enumerate() {
                    let key = format!("q{i}");
                    let mut recon = match m.get(&key) {
                        Some(J::Object(q)) => match q.get("result") {
                            Some(J::Array(results)) => self.results(text, results, history_entry),
                            _ => Recon::error(history_entry, self, format!("The service returned a JSON response without \"result\" field for query {key}")),
                        },
                        _ => Recon::error(history_entry, self, format!("The service returned a JSON response without \"{key}\" field")),
                    };
                    recon.service = self.service.clone();
                    out.push(recon);
                }
            }
        }
        while out.len() < jobs.len() {
            out.push(Recon::error(history_entry, self, "No. of recon objects was less than no. of jobs"));
        }
        out
    }

    /// createReconServiceResults: candidates by score (stable), the first auto-matched when the
    /// service says it is a match, then the features of the best candidate.
    fn results(&self, text: &str, results: &[J], history_entry: i64) -> Recon {
        let mut recon = Recon::new(history_entry, &self.identifier_space, &self.schema_space);
        let mut found: Vec<(Candidate, bool)> = results
            .iter()
            .map(|r| {
                let types = r.get("type").and_then(J::as_array).map(|t| t.iter().filter_map(|t| t.get("id").or(Some(t)).and_then(J::as_str).map(String::from)).collect());
                let c = Candidate {
                    id: r.get("id").and_then(J::as_str).unwrap_or_default().into(),
                    name: r.get("name").and_then(J::as_str).unwrap_or_default().into(),
                    types: types.unwrap_or_default(),
                    score: r.get("score").and_then(J::as_f64).unwrap_or(0.0),
                };
                (c, r.get("match").and_then(J::as_bool).unwrap_or(false))
            })
            .collect();
        found.sort_by(|a, b| b.0.score.total_cmp(&a.0.score));
        for (i, (c, is_match)) in found.into_iter().enumerate() {
            if self.auto_match && i == 0 && is_match {
                recon.matched = Some(c.clone());
                recon.match_rank = 0;
                recon.judgment = Judgment::Matched;
                recon.judgment_action = "auto".into();
            }
            recon.candidates.get_or_insert_with(Vec::new).push(c);
        }
        self.features(&mut recon, text);
        recon
    }

    fn features(&self, recon: &mut Recon, text: &str) {
        let Some(best) = recon.best().cloned() else {
            recon.features = Features::default();
            return;
        };
        let f = &mut recon.features;
        f.name_match = Some(java_equals_ignore_case(text, &best.name));
        f.name_levenshtein = Some(levenshtein(&text.to_lowercase(), &best.name.to_lowercase()) as i64);
        f.name_word_distance = Some(word_distance(text, &best.name));
        f.type_match = Some(self.type_id.as_ref().is_some_and(|t| best.types.contains(t)));
    }
}

fn json_str(s: &str) -> String {
    J::String(s.into()).to_string()
}

fn json_kind(v: &J) -> &'static str {
    match v {
        J::Array(_) => "Array value (token `JsonToken.START_ARRAY`)",
        J::String(_) => "String value",
        J::Number(_) => "Number value",
        J::Bool(_) => "Boolean value",
        _ => "value",
    }
}

/// A cell value as Jackson writes it inside a query.
fn jackson(v: &Value) -> String {
    match v {
        Value::Int(i) => i.to_string(),
        Value::Num(n) if n.is_finite() => java_double(*n),
        Value::Bool(b) => b.to_string(),
        other => json_str(&other.to_java_string()),
    }
}

/// HttpClient.postNameValue: a form with `queries`, the body back when the service answers 2xx.
/// Redirects as OpenRefine's HttpClient follows them: 307/308 send the form again to the new
/// address, 301/302/303 turn into a GET (ureq itself won't resend a body).
fn post(agent: &ureq::Agent, url: &str, queries: &str) -> std::result::Result<String, String> {
    let mut url = url.to_string();
    let mut res = None;
    for _ in 0..10 {
        let r = agent.post(&url).config().max_redirects(0).build().send_form([("queries", queries)]).map_err(|e| e.to_string())?;
        let next = r.headers().get("location").and_then(|l| l.to_str().ok()).and_then(|l| url::Url::parse(&url).ok()?.join(l).ok());
        match (r.status().as_u16(), next) {
            (307 | 308, Some(next)) => url = next.to_string(),
            (301..=303, Some(next)) => {
                res = Some(agent.get(next.as_str()).call().map_err(|e| e.to_string())?);
                break;
            }
            _ => {
                res = Some(r);
                break;
            }
        }
    }
    let mut res = res.ok_or("too many redirects")?;
    let status = res.status();
    let body = res.body_mut().read_to_string().map_err(|e| e.to_string())?;
    if status.is_success() {
        Ok(body)
    } else {
        let path = url::Url::parse(&url).map(|u| u.path().to_string() + &u.query().map(|q| format!("?{q}")).unwrap_or_default()).unwrap_or_else(|_| url.clone());
        Err(format!("HTTP error {} : {} for URL {path}", status.as_u16(), status.canonical_reason().unwrap_or("")))
    }
}

/// Java String.equalsIgnoreCase, unit by unit.
fn java_equals_ignore_case(a: &str, b: &str) -> bool {
    let (a, b): (Vec<u16>, Vec<u16>) = (a.encode_utf16().collect(), b.encode_utf16().collect());
    let up = |c: u16| char::from_u32(c as u32).map_or(c as u32, |c| c.to_uppercase().next().map_or(c as u32, |c| c as u32));
    let low = |c: u32| char::from_u32(c).map_or(c, |c| c.to_lowercase().next().map_or(c as u32, |c| c as u32));
    a.len() == b.len() && a.iter().zip(&b).all(|(&x, &y)| x == y || up(x) == up(y) || low(up(x)) == low(up(y)))
}

/// StringUtils.getLevenshteinDistance, over UTF-16 units.
fn levenshtein(a: &str, b: &str) -> usize {
    let (a, b): (Vec<u16>, Vec<u16>) = (a.encode_utf16().collect(), b.encode_utf16().collect());
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, &x) in a.iter().enumerate() {
        let mut cur = vec![i + 1; b.len() + 1];
        for (j, &y) in b.iter().enumerate() {
            cur[j + 1] = (prev[j] + (x != y) as usize).min(prev[j + 1] + 1).min(cur[j] + 1);
        }
        prev = cur;
    }
    prev[b.len()]
}

/// breakWords: lowercased, split on whitespace (a leading space gives an empty word, as in Java), no stop words.
fn break_words(s: &str) -> std::collections::HashSet<String> {
    let lower = s.to_lowercase();
    let mut words: Vec<&str> = regex_split_ws(&lower);
    while words.last().is_some_and(|w| w.is_empty()) {
        words.pop();
    }
    words.into_iter().filter(|w| !STOP_WORDS.contains(w)).map(String::from).collect()
}

fn regex_split_ws(s: &str) -> Vec<&str> {
    let mut out = vec![];
    let mut start = 0;
    let mut in_ws = false;
    for (i, c) in s.char_indices() {
        if c.is_whitespace() {
            if !in_ws {
                out.push(&s[start..i]);
                in_ws = true;
            }
        } else if in_ws {
            start = i;
            in_ws = false;
        }
    }
    out.push(if in_ws { "" } else { &s[start..] });
    out
}

fn word_distance(a: &str, b: &str) -> f64 {
    let (a, b) = (break_words(a), break_words(b));
    let (big, small) = if a.len() >= b.len() { (a, b) } else { (b, a) };
    if big.is_empty() {
        return 0.0;
    }
    small.iter().filter(|w| big.contains(*w)).count() as f64 / big.len() as f64
}

fn agent() -> ureq::Agent {
    ureq::Agent::new_with_config(
        ureq::config::Config::builder()
            .http_status_as_error(false)
            .timeout_global(Some(std::time::Duration::from_secs(120)))
            .user_agent(concat!("OxRefine/", env!("CARGO_PKG_VERSION")))
            .build(),
    )
}

/// A service's manifest (GET on its URL), checked to be one: it names an identifier space.
pub fn manifest(url: &str) -> Result<J> {
    let url = url.trim();
    let mut res = agent().get(url).call().map_err(|e| format!("{url}: {e}"))?;
    let status = res.status();
    let body = res.body_mut().read_to_string().map_err(|e| format!("{url}: {e}"))?;
    if !status.is_success() {
        return Err(format!("{url}: HTTP error {} : {}", status.as_u16(), status.canonical_reason().unwrap_or("")));
    }
    let j: J = serde_json::from_str(&body).map_err(|_| format!("{url} doesn't answer like a reconciliation service (no JSON manifest)"))?;
    if j.get("identifierSpace").and_then(J::as_str).is_none() {
        return Err(format!("{url} doesn't answer like a reconciliation service (its manifest has no identifierSpace)"));
    }
    Ok(j)
}

/// GuessTypesOfColumnCommand: the first 10 distinct values asked without a type (3 results each);
/// each result's types score by rank, most frequent types first, then by average score.
pub fn guess_types(t: &Table, column: &str, service: &str) -> Result<J> {
    const SAMPLES: usize = 10;
    let col = &t.columns[t.col(column)?];
    let mut samples: Vec<String> = vec![];
    for r in 0..t.rows {
        let v = col.get(r);
        if crate::grel::value::is_non_blank(v) {
            let s = v.to_java_string().trim().to_string();
            if !samples.contains(&s) {
                samples.push(s);
                if samples.len() >= SAMPLES {
                    break;
                }
            }
        }
    }
    let queries: Map<String, J> = samples.iter().enumerate().map(|(i, s)| (format!("q{i}"), json!({ "query": s, "limit": 3 }))).collect();
    let text = post(&agent(), service, &J::Object(queries).to_string())?;
    let answer: J = serde_json::from_str(&text).map_err(|_| "the reconciliation service returned an invalid response".to_string())?;
    // id -> (name, score, count), in the order first seen
    let mut groups: Vec<(String, String, f64, usize)> = vec![];
    for q in answer.as_object().into_iter().flat_map(|m| m.values()) {
        let Some(results) = q.get("result").and_then(J::as_array) else { continue };
        for (j, result) in results.iter().enumerate() {
            let score = 1.0 / (1 + j) as f64;
            let types = result.get("type").and_then(J::as_array).cloned().unwrap_or_default();
            let n = types.len();
            for (k, ty) in types.iter().enumerate() {
                let id = ty.get("id").or(Some(ty)).and_then(J::as_str).unwrap_or_default().to_string();
                let name = ty.get("name").and_then(J::as_str).unwrap_or(&id).to_string();
                let s = score * (n - k) as f64 / n as f64;
                match groups.iter_mut().find(|g| g.0 == id) {
                    Some(g) => {
                        g.2 += s;
                        g.3 += 1;
                    }
                    None => groups.push((id, name, s, 1)),
                }
            }
        }
    }
    groups.sort_by(|a, b| b.3.min(SAMPLES).cmp(&a.3.min(SAMPLES)).then((b.2 / b.3 as f64).total_cmp(&(a.2 / a.3 as f64))));
    Ok(J::Array(groups.into_iter().map(|(id, name, score, count)| json!({ "id": id, "name": name, "score": score, "count": count })).collect()))
}

/// What the grid shows of a cell's Recon: judgment, match, the best few candidates, the error.
pub fn brief(r: &Recon) -> J {
    let c = |c: &Candidate| json!({ "id": c.id, "name": c.name, "score": c.score });
    json!({
        "j": judgment_json(r.judgment),
        "m": r.matched.as_ref().map(c),
        "c": r.candidates.iter().flatten().take(3).map(c).collect::<Vec<_>>(),
        "n": r.candidates.as_ref().map_or(0, Vec::len),
        "e": r.error,
    })
}

/// Java's String.hashCode.
fn java_hash(s: &str) -> i32 {
    s.encode_utf16().fold(0i32, |h, c| h.wrapping_mul(31).wrapping_add(c as i32))
}

/// The order a java.util.HashMap iterates `keys` (distinct, in insertion order): bucket by bucket,
/// insertion order within one. OpenRefine batches reconciliation jobs in this order.
// ponytail: buckets that Java turns into trees (8+ collisions) may iterate differently
fn hashmap_order(keys: &[String]) -> Vec<usize> {
    let mut cap = 16usize;
    while keys.len() * 4 > cap * 3 {
        cap *= 2;
    }
    let mut buckets = vec![vec![]; cap];
    for (i, k) in keys.iter().enumerate() {
        let h = java_hash(k) as u32;
        buckets[((h ^ (h >> 16)) as usize) & (cap - 1)].push(i);
    }
    buckets.concat()
}

/// ReconOperation: every selected non-blank cell of `column` gets the Recon of its query; cells
/// with the same query share one. Returns, for each row, the new Recon (None = unchanged).
pub fn reconcile(t: &Table, column: &str, cfg: &StandardConfig, sel: Option<&[bool]>, history_entry: i64) -> Result<Vec<Option<Arc<Recon>>>> {
    let ci = t.col(column)?;
    let col = &t.columns[ci];
    // group the cells by OpenRefine's job key: the decimal hash of the query text
    let mut keys: Vec<String> = vec![];
    let mut groups: Vec<(String, String, Vec<usize>)> = vec![];
    let mut index: std::collections::HashMap<String, usize> = Default::default();
    for r in (0..t.rows).filter(|&r| sel.is_none_or(|s| s[r])) {
        let v = col.get(r);
        if !crate::grel::value::is_non_blank(v) {
            continue;
        }
        let text = v.to_java_string();
        let code = cfg.query(t, r, &text)?;
        let key = java_hash(&code).to_string();
        match index.get(&key) {
            Some(&g) => groups[g].2.push(r),
            None => {
                index.insert(key.clone(), groups.len());
                keys.push(key);
                groups.push((text, code, vec![r]));
            }
        }
    }
    let order = hashmap_order(&keys);
    let agent = agent();
    let mut out = vec![None; t.rows];
    crate::jobs::progress(&format!("Reconciling cells in column {column}"), order.len() as u64);
    for batch in order.chunks(cfg.batch_size(t.rows)) {
        crate::jobs::tick(0)?;
        let jobs: Vec<(&str, &str)> = batch.iter().map(|&g| (groups[g].0.as_str(), groups[g].1.as_str())).collect();
        for (&g, mut recon) in batch.iter().zip(cfg.batch(&agent, &jobs, history_entry)) {
            recon.batch_size = groups[g].2.len() as i64;
            let recon = Arc::new(recon);
            for &r in &groups[g].2 {
                out[r] = Some(recon.clone());
            }
        }
        crate::jobs::tick(batch.len() as u64)?;
    }
    Ok(out)
}

// ---------------------------------------------------------------- operations on Recons

/// A history step's id, as OpenRefine's Recons record it (judgmentHistoryEntry).
pub fn history_entry() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64)
}

/// The rows' new Recons for one column: `Some(None)` clears a cell's Recon, `None` leaves it.
pub type Edits = Vec<Option<Option<Arc<Recon>>>>;

/// The column with `edits` applied, its reconConfig replaced when given, and its stats as `stats` says.
pub fn apply_edits(col: &Column, edits: &Edits, config: Option<Option<J>>, stats: Stats) -> Column {
    let old = col.recon.as_ref();
    let cells: Vec<Option<Arc<Recon>>> = (0..col.codes.len()).map(|r| match &edits[r] {
        Some(new) => new.clone(),
        None => col.recon(r).cloned(),
    }).collect();
    let config = config.unwrap_or_else(|| old.and_then(|rc| rc.config.clone()));
    let mut rc = ReconCol { cells, config, stats: old.and_then(|rc| rc.stats.clone()) };
    let mut out = Column { dict: col.dict.clone(), codes: col.codes.clone(), recon: None };
    rc.stats = match stats {
        Stats::Keep => rc.stats,
        Stats::Count => Some(rc.count(&out)),
        Stats::Set(j) => Some(j),
    };
    out.recon = (rc.cells.iter().any(Option::is_some) || rc.config.is_some() || rc.stats.is_some()).then(|| Arc::new(rc));
    out
}

/// The stats after judging or clearing one cell, as OpenRefine's one-cell commands work them out:
/// counted before the change when the column has none, else adjusted by the change (clearing a cell
/// adds one to nonBlanks, as OpenRefine does).
pub fn one_cell_stats(col: &Column, old: Option<&Recon>, new: Option<&Recon>) -> J {
    let Some(stats) = col.recon.as_ref().and_then(|rc| rc.stats.clone()) else {
        return col.recon.as_ref().map_or_else(|| ReconCol::new(vec![None; col.codes.len()]).count(col), |rc| rc.count(col));
    };
    let n = |k: &str| stats.get(k).and_then(J::as_i64).unwrap_or(0);
    let delta = |j: Judgment| -> i64 { old.is_some_and(|o| o.judgment == j) as i64 * -1 + new.is_some_and(|x| x.judgment == j) as i64 };
    json!({
        "nonBlanks": n("nonBlanks") + new.is_none() as i64,
        "newTopics": n("newTopics") + delta(Judgment::New),
        "matchedTopics": n("matchedTopics") + delta(Judgment::Matched),
        "errorTopics": n("errorTopics") + delta(Judgment::Error),
    })
}

/// One new Recon per key (OpenRefine's `dupReconMap` and shared new items): cells with the same key
/// get the same Recon.
#[derive(Default)]
struct Shared(std::collections::HashMap<String, Arc<Recon>>);

impl Shared {
    fn get(&mut self, key: impl ToString, make: impl FnOnce() -> Recon) -> Option<Option<Arc<Recon>>> {
        Some(Some(self.0.entry(key.to_string()).or_insert_with(|| Arc::new(make())).clone()))
    }
}

/// judgmentBatchSize: how many of the edited cells got each new Recon.
fn count_batches(edits: &mut Edits) {
    let mut n: std::collections::HashMap<i64, i64> = Default::default();
    for r in edits.iter().flatten().flatten() {
        *n.entry(r.id).or_default() += 1;
    }
    let mut done: std::collections::HashMap<i64, Arc<Recon>> = Default::default();
    for e in edits.iter_mut().flatten().flatten() {
        *e = done.entry(e.id).or_insert_with(|| Arc::new(Recon { batch_size: n[&e.id], ..(**e).clone() })).clone();
    }
}

fn candidate_rank(r: &Recon, id: &str) -> i64 {
    r.candidates.as_ref().and_then(|c| c.iter().position(|c| c.id == id)).map_or(-1, |i| i as i64)
}

/// The mass operations of OpenRefine's Reconcile → Actions menu, over the selected rows.
pub enum Action<'a> {
    MatchBest,
    MarkNew { shared: bool, config: Option<&'a StandardConfig> },
    MatchSpecific { candidate: Candidate, identifier_space: Option<&'a str>, schema_space: Option<&'a str> },
    Discard { clear: bool },
    ClearSimilar(&'a str),
    JudgeSimilar { value: &'a str, judgment: Judgment, matched: Option<Candidate>, shared: bool, config: Option<StandardConfig> },
    UseValuesAsIds(&'a StandardConfig),
}

pub fn mass(col: &Column, rows: &[usize], action: Action) -> Result<Edits> {
    let h = history_entry();
    let mut edits: Edits = vec![None; col.codes.len()];
    let mut shared = Shared::default();
    for &r in rows {
        let v = col.get(r);
        let recon = col.recon(r);
        let text = v.to_java_string();
        edits[r] = match &action {
            Action::MatchBest => recon.and_then(|old| {
                let best = old.best()?.clone();
                shared.get(old.id, || Recon { matched: Some(best), match_rank: 0, judgment: Judgment::Matched, judgment_action: "mass".into(), ..old.dup(h) })
            }),
            Action::MarkNew { shared: true, config } if !v.is_null() => shared.get(text, || {
                let base = config.map_or_else(|| Recon::bare(h), |c| c.new_recon(h));
                Recon { judgment: Judgment::New, judgment_action: "mass".into(), ..base }
            }),
            Action::MarkNew { shared: false, config } if !v.is_null() => {
                let base = match recon {
                    Some(old) => old.dup(h),
                    None => config.map_or_else(|| Recon::bare(h), |c| c.new_recon(h)),
                };
                Some(Some(Arc::new(Recon { matched: None, match_rank: -1, judgment: Judgment::New, judgment_action: "mass".into(), ..base })))
            }
            Action::MarkNew { .. } => None,
            Action::MatchSpecific { candidate, identifier_space, schema_space } if !v.is_null() => {
                let key = recon.map_or(0, |r| r.id);
                let make = || {
                    let base = match recon {
                        Some(old) => old.dup(h),
                        None => match (identifier_space, schema_space) {
                            (Some(i), Some(s)) => Recon::new(h, i, s),
                            _ => Recon { identifier_space: identifier_space.map(String::from), schema_space: schema_space.map(String::from), ..Recon::bare(h) },
                        },
                    };
                    Recon { matched: Some(candidate.clone()), match_rank: -1, judgment: Judgment::Matched, judgment_action: "mass".into(), ..base }
                };
                shared.get(key, make)
            }
            Action::MatchSpecific { .. } => None,
            Action::Discard { clear: true } => recon.map(|_| None),
            Action::Discard { clear: false } => recon.and_then(|old| {
                shared.get(old.id, || Recon { matched: None, match_rank: -1, judgment: Judgment::None, judgment_action: "mass".into(), ..old.dup(h) })
            }),
            Action::ClearSimilar(value) => recon.filter(|_| &text == value).map(|_| None),
            Action::JudgeSimilar { value, judgment, matched, shared: share, config } if crate::grel::value::is_non_blank(v) && &text == value => {
                if *judgment == Judgment::Error {
                    return Err("Cannot manually set judgment to 'error'".into());
                }
                if *judgment == Judgment::New && *share {
                    shared.get("new", || {
                        let base = config.as_ref().map_or_else(|| Recon::bare(h), |c| c.new_recon(h));
                        Recon { judgment: Judgment::New, judgment_action: "similar".into(), ..base }
                    })
                } else {
                    let old = recon.ok_or_else(|| format!("the cell in row {} has no reconciliation data to judge", r + 1))?;
                    shared.get(old.id, || {
                        let mut n = Recon { match_rank: -1, judgment_action: "similar".into(), ..old.dup(h) };
                        match judgment {
                            Judgment::Matched => {
                                let m = matched.clone().unwrap_or_else(|| Candidate { id: String::new(), name: String::new(), types: vec![], score: 100.0 });
                                n.match_rank = candidate_rank(&n, &m.id);
                                n.matched = Some(m);
                                n.judgment = Judgment::Matched;
                            }
                            Judgment::New => {
                                n.judgment = Judgment::New;
                                n.matched = None;
                            }
                            _ => {
                                n.judgment = if n.error.is_none() { Judgment::None } else { Judgment::Error };
                                n.matched = None;
                            }
                        }
                        n
                    })
                }
            }
            Action::JudgeSimilar { .. } => None,
            Action::UseValuesAsIds(cfg) if crate::grel::value::is_non_blank(v) => {
                let id = text.strip_prefix(cfg.identifier_space.as_str()).unwrap_or(&text).to_string();
                let m = Candidate { id: id.clone(), name: id, types: vec![], score: 100.0 };
                Some(Some(Arc::new(Recon {
                    matched: Some(m.clone()),
                    candidates: Some(vec![m]),
                    match_rank: -1,
                    judgment: Judgment::Matched,
                    judgment_action: "mass".into(),
                    ..cfg.new_recon(h)
                })))
            }
            Action::UseValuesAsIds(_) => None,
        };
    }
    count_batches(&mut edits);
    Ok(edits)
}

/// ReconCopyAcrossColumnsOperation: the Recons of `from` cells with one of `judgments`, by cell value,
/// onto equal values in each `to` column (cells already judged only with `over_judged`).
pub fn copy_across(t: &Table, rows: &[usize], from: usize, to: &[usize], judgments: &[Judgment], over_judged: bool) -> Vec<Edits> {
    let col = &t.columns[from];
    let mut by_value: std::collections::HashMap<&Value, Arc<Recon>> = Default::default();
    for &r in rows {
        let v = col.get(r);
        if let Some(rc) = col.recon(r).filter(|rc| !v.is_null() && judgments.contains(&rc.judgment)) {
            by_value.insert(v, rc.clone());
        }
    }
    to.iter()
        .map(|&c| {
            let target = &t.columns[c];
            let mut edits: Edits = vec![None; t.rows];
            for &r in rows {
                let v = target.get(r);
                let judged = target.recon(r).is_some_and(|rc| rc.judgment != Judgment::None);
                if let Some(rc) = by_value.get(v).filter(|_| !v.is_null() && (!judged || over_judged)) {
                    edits[r] = Some(Some(rc.clone()));
                }
            }
            edits
        })
        .collect()
}

/// ReconJudgeOneCellCommand: one cell matched to a candidate, marked new, or its judgment discarded.
pub fn judge_one(col: &Column, row: usize, judgment: Judgment, matched: Option<Candidate>, spaces: (Option<&str>, Option<&str>)) -> Result<Arc<Recon>> {
    let v = col.get(row);
    if !crate::grel::value::is_non_blank(v) {
        return Err("Cell is blank or error".into());
    }
    let h = history_entry();
    let config = col.recon.as_ref().and_then(|rc| rc.config.as_ref()).and_then(|j| StandardConfig::from_json(j).ok());
    let mut n = match (col.recon(row), spaces) {
        (Some(old), _) => old.dup(h),
        (None, (Some(i), Some(s))) => Recon::new(h, i, s),
        (None, _) => config.map_or_else(|| Recon::bare(h), |c| c.new_recon(h)),
    };
    n.match_rank = -1;
    n.judgment_action = "single".into();
    n.batch_size = 1;
    match judgment {
        Judgment::None => {
            n.judgment = if n.error.is_none() { Judgment::None } else { Judgment::Error };
            n.matched = None;
        }
        Judgment::Error => return Err("Cannot manually set judgment to 'error'".into()),
        Judgment::New => {
            n.judgment = Judgment::New;
            n.matched = None;
        }
        Judgment::Matched => {
            let m = matched.ok_or("matching a cell needs the item to match")?;
            n.match_rank = candidate_rank(&n, &m.id);
            n.matched = Some(m);
            n.judgment = Judgment::Matched;
        }
    }
    Ok(Arc::new(n))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A service that moved: 307 from /old to /new, which answers with the form it got.
    #[test]
    fn post_follows_redirects() {
        use std::io::{BufRead, BufReader, Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let mut r = BufReader::new(stream.try_clone().unwrap());
                let (mut line, mut len) = (String::new(), 0);
                r.read_line(&mut line).unwrap();
                loop {
                    let mut h = String::new();
                    if r.read_line(&mut h).unwrap_or(0) == 0 || h.trim().is_empty() {
                        break;
                    }
                    if let Some(v) = h.to_ascii_lowercase().strip_prefix("content-length:") {
                        len = v.trim().parse().unwrap();
                    }
                }
                let mut body = vec![0; len];
                r.read_exact(&mut body).unwrap();
                let mut s = stream;
                let reply = if line.contains("/old") {
                    "HTTP/1.1 307 Temporary Redirect\r\nLocation: /new\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_string()
                } else {
                    let b = String::from_utf8_lossy(&body).to_string();
                    format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{b}", b.len())
                };
                s.write_all(reply.as_bytes()).unwrap();
            }
        });
        assert_eq!(post(&agent(), &format!("{base}/old"), "{}").unwrap(), "queries=%7B%7D");
    }

    #[test]
    fn features_and_order() {
        assert_eq!(levenshtein("lovelace", "ada lovelace"), 4);
        assert_eq!(word_distance("Lovelace", "Ada Lovelace"), 0.5);
        assert_eq!(word_distance("the Douglas", "Douglas"), 1.0);
        assert!(java_equals_ignore_case("douglas adams", "Douglas Adams"));
        assert_eq!(java_hash("hello"), 99162322);
        // OpenRefine's batch order for these queries (from its fixtures): Paris first, Broken last
        let names = ["Douglas Adams", "douglas adams", "Douglas Adam", "Paris", "Paris Hilton", "Ada Lovelace", "Lovelace", "Nobody Known", "Broken"];
        let keys: Vec<String> = names.iter().map(|n| java_hash(&format!("{{\"query\":\"{n}\",\"type\":\"Q5\",\"type_strict\":\"should\"}}")).to_string()).collect();
        let order: Vec<&str> = hashmap_order(&keys).into_iter().map(|i| names[i]).collect();
        assert_eq!(order[0], "Paris");
        assert_eq!(order[1], "Douglas Adams");
        assert_eq!(order[8], "Broken");
    }
}
