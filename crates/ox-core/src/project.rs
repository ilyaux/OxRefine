use crate::{
    cluster,
    engine::{self, EngineConfig},
    err, grel,
    import::{self, ImportOptions},
    ops::Op,
    store::{History, Info, Meta, Source, Store},
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
    /// OpenRefine's metadata: tags, description... (also in `meta` for the project list)
    pub info: Info,
}

impl Project {
    pub fn new(name: impl Into<String>, table: Table) -> Project {
        Project { name: name.into(), states: vec![table], ops: vec![], cursor: 0, gen: 0, cache: None, meta: None, info: Info::default() }
    }

    /// Import a file with OpenRefine's default options (CSV/TSV, Excel, ODS or JSON by extension),
    /// or open an OpenRefine project archive with its history.
    pub fn open(path: &Path) -> Result<Project> {
        if crate::orproject::is_archive(path) {
            return Ok(crate::orproject::read(path)?.into_project().0);
        }
        Ok(Project::new(file_name(path), import::import(path, None, &ImportOptions::default())?))
    }

    /// Rebuild a saved project: the original table, then its operations replayed.
    pub fn restore(meta: Meta, original: Table, history: History) -> Result<Project> {
        let mut p = Project::new(meta.name.clone(), original);
        for (i, op) in history.ops.into_iter().enumerate() {
            p.apply(op).map_err(|e| format!("replaying step {}: {e}", i + 1))?;
        }
        p.goto(history.cursor.min(p.ops.len()))?;
        p.info = meta.info.clone();
        p.meta = Some(meta);
        Ok(p)
    }

    /// A project from tables already worked out: `tables[i]` is the table after `ops[..i]`.
    pub(crate) fn from_steps(name: String, tables: Vec<Table>, ops: Vec<Op>) -> Project {
        let cursor = ops.len();
        Project { name, states: tables, ops, cursor, gen: 0, cache: None, meta: None, info: Info::default() }
    }

    /// Every table, every step and the cursor (for writing the whole history out).
    pub(crate) fn history_parts(&self) -> (&[Table], &[Op], usize) {
        (&self.states, &self.ops, self.cursor)
    }

    pub fn table(&self) -> &Table {
        &self.states[self.cursor]
    }

    pub fn apply(&mut self, mut op: Op) -> Result<()> {
        let t = share(self.table(), op.apply(self.table())?);
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
            "info": self.info,
            "columns": t.names,
            "rows": t.rows,
            "history": self.ops.iter().map(Op::description).collect::<Vec<_>>(),
            // per step: whether it can go into an extracted recipe
            "recipe": self.ops.iter().map(Op::in_recipe).collect::<Vec<_>>(),
            "cursor": self.cursor,
            // reconciled columns: the service and how far along judging is
            "recon": t.names.iter().zip(&t.columns).filter_map(|(n, c)| {
                let rc = c.recon.as_ref()?;
                let service = rc.config.as_ref().and_then(|j| j.get("service")).cloned()
                    .or_else(|| rc.cells.iter().flatten().next().map(|r| json!(r.service)))?;
                Some((n.clone(), json!({ "service": service, "stats": rc.stats(c) })))
            }).collect::<serde_json::Map<_, _>>(),
        })
    }

    /// Persist history after a change (no-op for unsaved projects).
    fn save(&mut self, store: Option<&Store>) -> Result<()> {
        let (Some(store), Some(meta)) = (store, self.meta.as_mut()) else { return Ok(()) };
        meta.info = self.info.clone();
        let history = History { ops: self.ops.clone(), cursor: self.cursor };
        let t = &self.states[self.cursor];
        store.touch(meta, t, &history)
    }
}

/// What a step left as it was shares the previous step's memory: a column that is equal, or the row
/// codes of one whose rows were only renamed value for value (each old code became one new code and
/// no two became the same; most transforms), with its dictionary reordered to the old codes.
// ponytail: a step that changes the codes of some rows (a mass edit merging values) still stores the
// column's codes in full (4 bytes a row); store those as a patch over the previous codes if long
// histories of such steps on very large projects need it.
fn share(prev: &Table, mut t: Table) -> Table {
    if prev.rows != t.rows {
        return t;
    }
    for i in 0..t.columns.len() {
        let Some(p) = prev.names.iter().position(|n| *n == t.names[i]).map(|j| &prev.columns[j]) else { continue };
        let c = &t.columns[i];
        if Arc::ptr_eq(p, c) || Arc::ptr_eq(&p.codes, &c.codes) {
            continue;
        }
        let same_recon = match (&p.recon, &c.recon) {
            (None, None) => true,
            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
            _ => false,
        };
        if same_recon && p.codes == c.codes && p.dict == c.dict {
            t.columns[i] = p.clone();
            continue;
        }
        // old code -> new code, the same for every row, and never two old codes to one new one
        const NONE: u32 = u32::MAX;
        let (mut to, mut from) = (vec![NONE; p.dict.len()], vec![NONE; c.dict.len()]);
        let renamed = p.codes.iter().zip(c.codes.iter()).all(|(&o, &n)| match (to[o as usize], from[n as usize]) {
            (NONE, NONE) => {
                to[o as usize] = n;
                from[n as usize] = o;
                true
            }
            (x, y) => x == n && y == o,
        });
        if renamed {
            let dict = to.iter().map(|&n| if n == NONE { crate::grel::Value::Null } else { c.dict[n as usize].clone() }).collect();
            t.columns[i] = Arc::new(crate::table::Column { dict, codes: p.codes.clone(), recon: c.recon.clone() });
        }
    }
    t
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
        let a = row.as_array_mut().unwrap();
        // [5]: s[r] is the record number of row r; only its first row shows it
        if let Some(s) = starts {
            a.push(if r == 0 || s[r] != s[r - 1] { json!(s[r]) } else { Value::Null });
        }
        // [6]: Recons by column index, when the row has any
        let recons: serde_json::Map<String, Value> =
            t.columns.iter().enumerate().filter_map(|(i, c)| c.recon(r).map(|rc| (i.to_string(), crate::recon::brief(rc)))).collect();
        if !recons.is_empty() {
            if starts.is_none() {
                a.push(Value::Null);
            }
            a.push(Value::Object(recons));
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
    info: Info,
    /// a preference's value (`set_preference`)
    value: Value,
}

/// Single JSON entry point shared by the Tauri app and `ox serve`. Returns `{data, ms}`.
/// Each command runs as a job, so `processes` can report its progress and `cancel_process` stop it.
pub fn dispatch(s: &mut Session, cmd: &str, args: Value) -> Result<Value> {
    if let Some(r) = dispatch_free(cmd, &args) {
        return r;
    }
    let t0 = Instant::now();
    let a = serde_json::from_value(args).map_err(err)?;
    let find = projects_by_name(s);
    let data = crate::jobs::run("Working", || crate::cross::scope(find, || run(s, cmd, a)))?;
    Ok(json!({ "data": data, "ms": t0.elapsed().as_secs_f64() * 1000.0 }))
}

/// What cross() can look into: the open project and every saved one, by name (a saved project is
/// opened, its history replayed, the first time a command asks for it).
fn projects_by_name(s: &Session) -> impl Fn(&str) -> Vec<Table> + 'static {
    let store = s.store.clone();
    let open = s.project.as_ref().map(|p| (p.meta.as_ref().map(|m| m.id.clone()), p.name.clone(), p.table().clone()));
    move |name| {
        let mut out = vec![];
        let open_id = open.as_ref().and_then(|o| o.0.clone());
        if let Some((_, n, t)) = &open {
            if n == name {
                out.push(t.clone());
            }
        }
        for m in store.iter().flat_map(Store::list).filter(|m| m.name == name && Some(&m.id) != open_id.as_ref()) {
            let st = store.as_ref().unwrap();
            if let Ok(p) = st.load(&m.id).and_then(|(meta, original, history)| Project::restore(meta, original, history)) {
                out.push(p.table().clone());
            }
        }
        out
    }
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
        "recon_service" => return crate::recon::manifest(&a.url),
        // `method`: entity, type or property; `text`: what was typed
        "recon_suggest" => return crate::recon::suggest(&a.url, &a.method, &a.text),
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
        // an OpenRefine project archive, history included; `report` says what history couldn't come along
        "import_project" => {
            let (mut p, report) = crate::orproject::read(Path::new(&a.path))?.into_project();
            if !a.name.trim().is_empty() {
                p.name = a.name.trim().to_string();
            }
            if let Some(st) = store {
                crate::jobs::progress("Saving the project", 0);
                let source = Source { path: a.path.clone(), format: Some("openrefine-project".into()), options: ImportOptions::default() };
                p.meta = Some(st.create(&p.name, &p.history_parts().0[0], Some(source))?);
                p.save(store)?;
            }
            let mut out = p.summary();
            out["report"] = json!(report);
            s.project = Some(p);
            return Ok(out);
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
        // OpenRefine's metadata of a saved project (the open one or any other); tags are tidied as OpenRefine does
        "set_project_info" => {
            let st = store.ok_or("projects are not saved in this session")?;
            let info = a.info.tidy();
            if let Some(p) = s.project.as_mut().filter(|p| p.meta.as_ref().is_some_and(|m| m.id == a.id)) {
                p.info = info;
                p.save(Some(st))?;
                return Ok(json!(p.meta));
            }
            return Ok(json!(st.set_info(&a.id, info)?));
        }
        "preferences" => return Ok(json!(store.map(Store::preferences).unwrap_or_default())),
        "set_preference" => {
            let st = store.ok_or("preferences are not saved in this session")?;
            return Ok(json!(st.set_preference(&a.name, a.value)?));
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
        // what each column holds, for the grid's headers (over every row; facets don't change it)
        "profile" => json!(engine::profile(p.table())),
        "facets" => {
            let prefs = store.map(Store::preferences).unwrap_or_default();
            let limit = prefs.get("ui.browsing.listFacet.limit").and_then(Value::as_u64).map_or(engine::FACET_LIMIT, |n| n as usize);
            json!(engine::facet_counts(p.table(), &a.engine, limit)?)
        }
        "recon_guess_types" => crate::recon::guess_types(p.table(), &a.column, &a.url)?,
        // what the column's service can add: its proposed properties for the column's type
        "recon_propose" => {
            let t = p.table();
            let rc = t.columns[t.col(&a.column)?].recon.as_ref().and_then(|rc| rc.config.clone()).unwrap_or_default();
            let service = rc.get("service").and_then(Value::as_str).ok_or("this column hasn't been reconciled")?;
            let ty = rc.get("type").and_then(|t| t.get("id")).and_then(Value::as_str);
            let mut out = crate::recon::propose_properties(service, ty)?;
            out["service"] = json!(service);
            out
        }
        // one cell's Recon in full (`index` is the row), for judging it
        "recon_cell" => {
            let t = p.table();
            let c = &t.columns[t.col(&a.column)?];
            json!(c.recon(a.index).map(|r| r.to_json(true)))
        }
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
            let ci = t.col(&a.column)?;
            let col = &t.columns[ci];
            let ctx = engine::Ctx::default();
            // [before, after] as the expression dialog shows them: null, text, or {"error": message}
            let pairs = rows.iter().take(a.limit.clamp(1, 100)).map(|&r| {
                let v = col.get(r as usize);
                let out = match e.eval(v.clone(), &t.row_at(r as usize, &ctx, ci)) {
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
                crate::export::export_file(p.table(), &rows, format, &a.params, &p.name, Path::new(&a.path))?;
                json!(rows.len())
            }
        }
        "export_project" => {
            crate::orproject::write(p, Path::new(&a.path))?;
            json!(a.path)
        }
        // `steps` picks history steps by index (the History panel's Extract); default: up to the cursor
        "export_recipe" => {
            let r = if a.steps.is_empty() {
                json!(p.recipe().into_iter().map(Op::recipe_json).collect::<Vec<_>>())
            } else {
                json!(a.steps.iter().filter_map(|&i| p.steps().get(i)).filter(|o| o.in_recipe()).map(Op::recipe_json).collect::<Vec<_>>())
            };
            if !a.path.is_empty() {
                std::fs::write(&a.path, serde_json::to_string_pretty(&r).map_err(err)?).map_err(err)?;
            }
            r
        }
        _ => return Err(format!("unknown command `{cmd}`")),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// History keeps one copy of a column's row codes for as long as steps leave them as they are.
    #[test]
    fn steps_share_unchanged_codes() {
        let path = std::env::temp_dir().join(format!("oxrefine-share-{}.csv", std::process::id()));
        let mut csv = String::from("name,city\n");
        for i in 0..1000 {
            csv += &format!(" person {} ,City{}\n", i % 37, i % 11);
        }
        std::fs::write(&path, csv).unwrap();
        let mut p = Project::open(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        let op = |col: &str, e: &str| -> Op {
            serde_json::from_value(json!({ "op": "core/text-transform", "columnName": col, "expression": format!("grel:{e}") })).unwrap()
        };
        for e in ["value.trim()", "value.toUppercase()", "value.toLowercase()", "value + ''"] {
            p.apply(op("name", e)).unwrap();
        }
        // a step that merges values changes codes, and gets codes of its own
        p.apply(op("city", "value.substring(0, 5)")).unwrap();
        let codes: Vec<&Arc<Vec<u32>>> = p.states.iter().flat_map(|t| t.columns.iter().map(|c| &c.codes)).collect();
        let mut unique: Vec<&Arc<Vec<u32>>> = vec![];
        for c in codes {
            if !unique.iter().any(|u| Arc::ptr_eq(u, c)) {
                unique.push(c);
            }
        }
        // name's codes once (its four steps only change the distinct values), city's twice
        let per_state: Vec<Vec<usize>> = p.states.iter().map(|t| t.columns.iter().map(|c| unique.iter().position(|u| Arc::ptr_eq(u, &c.codes)).unwrap()).collect()).collect();
        assert_eq!(unique.len(), 3, "6 states x 2 columns should keep 3 code vectors: {per_state:?}");
        assert_eq!(p.table().columns[0].get(0).as_str(), Some("person 0"));
    }
}
