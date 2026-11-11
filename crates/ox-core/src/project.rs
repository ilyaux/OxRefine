use crate::{
    cluster,
    engine::{self, EngineConfig},
    err, grel,
    import::{self, ImportOptions},
    ops::Op,
    store::{History, Meta, Source, Store},
    table::Table,
    Result,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{path::Path, sync::Arc, time::Instant};

/// A loaded table plus its full history. `states[i]` is the table after `ops[..i]`.
// ponytail: one snapshot per step; each step only re-allocates the columns it touched,
// so memory grows ~4 bytes/row per step. Store code diffs instead if long histories on huge files matter.
pub struct Project {
    pub name: String,
    states: Vec<Table>,
    ops: Vec<Op>,
    cursor: usize,
    gen: u64,
    cache: Option<(u64, String, Arc<Vec<u32>>)>,
    /// Set when the project lives in a store; every step is saved.
    pub meta: Option<Meta>,
}

impl Project {
    pub fn new(name: impl Into<String>, table: Table) -> Project {
        Project { name: name.into(), states: vec![table], ops: vec![], cursor: 0, gen: 0, cache: None, meta: None }
    }

    /// Import a file with OpenRefine's default options (CSV/TSV, Excel, ODS or JSON by extension).
    pub fn open(path: &Path) -> Result<Project> {
        Ok(Project::new(file_name(path), import::import(path, None, &ImportOptions::default())?))
    }

    /// Rebuild a saved project: the original table, then its operations replayed.
    pub fn restore(meta: Meta, original: Table, history: History) -> Result<Project> {
        let mut p = Project::new(meta.name.clone(), original);
        for (i, op) in history.ops.into_iter().enumerate() {
            p.apply(op).map_err(|e| format!("replaying step {}: {e}", i + 1))?;
        }
        p.goto(history.cursor.min(p.ops.len()))?;
        p.meta = Some(meta);
        Ok(p)
    }

    pub fn table(&self) -> &Table {
        &self.states[self.cursor]
    }

    pub fn apply(&mut self, mut op: Op) -> Result<()> {
        let t = op.apply(self.table())?;
        self.states.truncate(self.cursor + 1);
        self.ops.truncate(self.cursor);
        self.states.push(t);
        self.ops.push(op);
        self.cursor += 1;
        self.gen += 1;
        Ok(())
    }

    pub fn goto(&mut self, i: usize) -> Result<()> {
        if i >= self.states.len() {
            return Err(format!("no history step {i}"));
        }
        self.cursor = i;
        self.gen += 1;
        Ok(())
    }

    /// Every recorded step, including undone ones.
    pub fn steps(&self) -> &[Op] {
        &self.ops
    }

    /// Operations up to the current step, in OpenRefine's JSON shape (without single-cell edits).
    pub fn recipe(&self) -> Vec<&Op> {
        self.ops[..self.cursor].iter().filter(|o| o.in_recipe()).collect()
    }

    /// Filtered row indices, cached per (table version, facets) so scrolling is free.
    pub fn rows(&mut self, cfg: &EngineConfig) -> Result<Arc<Vec<u32>>> {
        let key = serde_json::to_string(cfg).map_err(err)?;
        if let Some((g, k, r)) = &self.cache {
            if *g == self.gen && *k == key {
                return Ok(r.clone());
            }
        }
        let r = Arc::new(engine::filter(self.table(), cfg, None)?);
        self.cache = Some((self.gen, key, r.clone()));
        Ok(r)
    }

    fn summary(&self) -> Value {
        let t = self.table();
        json!({
            "id": self.meta.as_ref().map(|m| m.id.clone()),
            "name": self.name,
            "columns": t.names,
            "rows": t.rows,
            "history": self.ops.iter().map(Op::description).collect::<Vec<_>>(),
            // per step: whether it can go into an extracted recipe
            "recipe": self.ops.iter().map(Op::in_recipe).collect::<Vec<_>>(),
            "cursor": self.cursor,
        })
    }

    /// Persist history after a change (no-op for unsaved projects).
    fn save(&mut self, store: Option<&Store>) -> Result<()> {
        let (Some(store), Some(meta)) = (store, self.meta.as_mut()) else { return Ok(()) };
        let history = History { ops: self.ops.clone(), cursor: self.cursor };
        let t = &self.states[self.cursor];
        store.touch(meta, t, &history)
    }
}

fn file_name(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().into()).unwrap_or_default()
}

/// The open project and, when projects are saved, the store they live in.
#[derive(Default)]
pub struct Session {
    pub project: Option<Project>,
    pub store: Option<Store>,
}

impl Session {
    pub fn with_store(store: Store) -> Session {
        Session { project: None, store: Some(store) }
    }
}

/// A cell for the grid: null, or the text OpenRefine shows (numbers and dates formatted).
fn cell_json(v: &grel::Value) -> Value {
    if v.is_null() { Value::Null } else { Value::String(v.display()) }
}

/// One letter per cell so the grid can style it: s text/blank, n number, b boolean, d date, e error, o other.
fn cell_type(v: &grel::Value) -> char {
    use grel::Value::*;
    match v {
        Null | Str(_) => 's',
        Int(_) | Num(_) => 'n',
        Bool(_) => 'b',
        Date(_) => 'd',
        Error(_) => 'e',
        _ => 'o',
    }
}

/// Grid rows: `[row, cells, starred, flagged, types]`, plus the record number when the row starts a
/// record (`starts` is given in records mode).
fn table_page(t: &Table, rows: impl Iterator<Item = usize>, starts: Option<&[u32]>) -> Vec<Value> {
    rows.map(|r| {
        let types: String = t.columns.iter().map(|c| cell_type(c.get(r))).collect();
        let mut row = json!([r, t.columns.iter().map(|c| cell_json(c.get(r))).collect::<Vec<_>>(), t.starred[r], t.flagged[r], types]);
        if let Some(s) = starts {
            // s[r] is the record number of row r; only its first row shows it
            row.as_array_mut().unwrap().push(if r == 0 || s[r] != s[r - 1] { json!(s[r]) } else { Value::Null });
        }
        row
    })
    .collect()
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Args {
    id: String,
    name: String,
    path: String,
    format: Option<String>,
    options: ImportOptions,
    engine: EngineConfig,
    start: usize,
    limit: usize,
    column: String,
    expression: String,
    method: String,
    n: usize,
    index: usize,
    op: Option<Op>,
    ops: Vec<Op>,
    params: Value,
    steps: Vec<usize>,
    /// nearest-neighbour clustering: distance radius and blocking n-gram size
    radius: Option<f64>,
    block: usize,
    /// clipboard text to stage (`paste`), or where a staged file came from (`open`)
    text: String,
    url: String,
    origin: String,
}

/// Single JSON entry point shared by the Tauri app and `ox serve`. Returns `{data, ms}`.
/// Each command runs as a job, so `processes` can report its progress and `cancel_process` stop it.
pub fn dispatch(s: &mut Session, cmd: &str, args: Value) -> Result<Value> {
    if let Some(r) = dispatch_free(cmd, &args) {
        return r;
    }
    let t0 = Instant::now();
    let a = serde_json::from_value(args).map_err(err)?;
    let data = crate::jobs::run("Working", || run(s, cmd, a))?;
    Ok(json!({ "data": data, "ms": t0.elapsed().as_secs_f64() * 1000.0 }))
}

/// Commands that need no session, so hosts answer them while another command holds it:
/// `processes` (running jobs with progress) and `cancel_process` (`id`, or every job without one).
pub fn dispatch_free(cmd: &str, args: &Value) -> Option<Result<Value>> {
    let data = match cmd {
        "processes" => crate::jobs::list(),
        "cancel_process" => {
            crate::jobs::cancel(args.get("id").and_then(Value::as_u64).unwrap_or(0));
            Value::Null
        }
        _ => return None,
    };
    Some(Ok(json!({ "data": data, "ms": 0.0 })))
}

fn run(s: &mut Session, cmd: &str, a: Args) -> Result<Value> {
    let store = s.store.as_ref();
    // ---------------------------------------------------------- no project needed
    match cmd {
        "inspect" => return import::inspect(Path::new(&a.path)),
        // Clipboard and Web Addresses: stage the data as a file, then the usual import wizard runs on it
        "paste" => {
            let path = import::stage_text(&import::staging_dir(store.map(|s| s.root())), &a.text)?;
            return Ok(json!({ "path": path, "name": "Clipboard", "origin": "(clipboard)" }));
        }
        "fetch_url" => {
            let (path, name) = import::download(&import::staging_dir(store.map(|s| s.root())), &a.url)?;
            return Ok(json!({ "path": path, "name": name, "origin": a.url.trim() }));
        }
        "preview_import" => {
            let mut o = a.options.clone();
            o.limit = if a.limit == 0 { 100 } else { a.limit as i64 };
            let t = import::import(Path::new(&a.path), a.format.as_deref(), &o)?;
            return Ok(json!({ "columns": t.names, "rows": table_page(&t, 0..t.rows, None) }));
        }
        "projects" => return Ok(json!(store.map(Store::list).unwrap_or_default())),
        "open" => {
            let path = Path::new(&a.path);
            let table = import::import(path, a.format.as_deref(), &a.options)?;
            crate::jobs::progress("Saving the project", 0);
            let name = if a.name.trim().is_empty() { file_name(path) } else { a.name.trim().to_string() };
            let mut p = Project::new(name.clone(), table);
            if let Some(st) = store {
                let origin = if a.origin.is_empty() { a.path.clone() } else { a.origin.clone() };
                let source = Source { path: origin, format: a.format.clone(), options: a.options.clone() };
                p.meta = Some(st.create(&name, p.table(), Some(source))?);
            }
            // a staged paste or download is in the project now
            if path.starts_with(import::staging_dir(store.map(|s| s.root()))) {
                let _ = std::fs::remove_dir_all(path.parent().unwrap_or(path));
            }
            s.project = Some(p);
        }
        "open_project" => {
            let st = store.ok_or("projects are not saved in this session")?;
            let (meta, original, history) = st.load(&a.id)?;
            s.project = Some(Project::restore(meta, original, history)?);
        }
        "rename_project" => {
            let st = store.ok_or("projects are not saved in this session")?;
            let meta = st.rename(&a.id, a.name.trim())?;
            if let Some(p) = s.project.as_mut().filter(|p| p.meta.as_ref().is_some_and(|m| m.id == a.id)) {
                p.name = meta.name.clone();
                p.meta = Some(meta.clone());
            }
            return Ok(json!(meta));
        }
        "delete_project" => {
            store.ok_or("projects are not saved in this session")?.delete(&a.id)?;
            if s.project.as_ref().is_some_and(|p| p.meta.as_ref().is_some_and(|m| m.id == a.id)) {
                s.project = None;
            }
            return Ok(Value::Null);
        }
        "close" => {
            s.project = None;
            return Ok(Value::Null);
        }
        _ => {}
    }

    // ---------------------------------------------------------- the open project
    let p = s.project.as_mut().ok_or("no project open")?;
    Ok(match cmd {
        "open" | "open_project" | "summary" => p.summary(),
        "rows" => {
            let rows = p.rows(&a.engine)?;
            let t = p.table();
            // ponytail: record numbers are recounted per page (O(rows)); cache them per table version if paging gets slow
            let starts: Option<Vec<u32>> = (a.engine.mode == "record-based").then(|| {
                let mut n = 0u32;
                t.record_starts(t.key_index()).into_iter().enumerate().map(|(r, s)| { if s && r > 0 { n += 1 } n }).collect()
            });
            let page = table_page(t, rows[a.start.min(rows.len())..].iter().take(a.limit.min(1000)).map(|&r| r as usize), starts.as_deref());
            json!({ "total": rows.len(), "rows": page })
        }
        "facets" => json!(engine::facet_counts(p.table(), &a.engine)?),
        "apply" => {
            p.apply(a.op.ok_or("missing op")?)?;
            p.save(store)?;
            p.summary()
        }
        "apply_recipe" => {
            let n = a.ops.len();
            crate::jobs::progress(&format!("Applying {n} steps"), n as u64);
            let result = a.ops.into_iter().enumerate().try_for_each(|(i, op)| {
                crate::jobs::tick(0)?;
                p.apply(op).map_err(|e| format!("step {}: {e}", i + 1))?;
                crate::jobs::tick(1)
            });
            // keep the steps that did apply, even when a later one failed
            p.save(store)?;
            result?;
            p.summary()
        }
        "goto" => {
            p.goto(a.index)?;
            p.save(store)?;
            p.summary()
        }
        "cluster" => {
            let rows = p.rows(&a.engine)?;
            let t = p.table();
            let vc = engine::value_counts(t, t.col(&a.column)?, &rows);
            let vc: Vec<(&str, u32)> = vc.iter().map(|(v, c)| (v.as_str(), *c)).collect();
            // OpenRefine's defaults: n-gram size 2, radius 1, blocks of 6-character n-grams
            let (n, block) = (if a.n == 0 { 2 } else { a.n }, if a.block == 0 { 6 } else { a.block });
            json!(cluster::cluster(&vc, &a.method, n, a.radius.unwrap_or(1.0), block)?)
        }
        "preview" => {
            let e = grel::compile(&a.expression)?;
            let rows = p.rows(&a.engine)?;
            let t = p.table();
            let col = &t.columns[t.col(&a.column)?];
            let ctx = engine::Ctx::default();
            // [before, after] as the expression dialog shows them: null, text, or {"error": message}
            let pairs = rows.iter().take(a.limit.clamp(1, 100)).map(|&r| {
                let v = col.get(r as usize);
                let out = match e.eval(v.clone(), &t.row_ctx(r as usize, &ctx)) {
                    grel::Value::Error(m) => json!({ "error": m }),
                    out => json!(out.render()),
                };
                json!([cell_json(v), out])
            });
            json!(pairs.collect::<Vec<_>>())
        }
        // writes the file at `path`; without a path returns the text of the first `limit` rows (previews)
        "export" => {
            let rows = p.rows(&a.engine)?;
            let format = a.format.as_deref().unwrap_or_else(|| crate::export::format_for(&a.path));
            if a.path.is_empty() {
                let rows = &rows[..rows.len().min(if a.limit == 0 { usize::MAX } else { a.limit })];
                let bytes = crate::export::export(p.table(), rows, format, &a.params, &p.name)?;
                json!(String::from_utf8_lossy(&bytes))
            } else {
                let bytes = crate::export::export(p.table(), &rows, format, &a.params, &p.name)?;
                std::fs::write(&a.path, bytes).map_err(|e| format!("{}: {e}", a.path))?;
                json!(rows.len())
            }
        }
        // `steps` picks history steps by index (the History panel's Extract); default: up to the cursor
        "export_recipe" => {
            let r = if a.steps.is_empty() {
                json!(p.recipe())
            } else {
                json!(a.steps.iter().filter_map(|&i| p.steps().get(i)).filter(|o| o.in_recipe()).collect::<Vec<_>>())
            };
            if !a.path.is_empty() {
                std::fs::write(&a.path, serde_json::to_string_pretty(&r).map_err(err)?).map_err(err)?;
            }
            r
        }
        _ => return Err(format!("unknown command `{cmd}`")),
    })
}
