//! Saved projects: one folder per project in the data directory.
//!   data.oxt     original table, written once at import (binary: JSON header + u32 codes)
//!   history.json operations and cursor, rewritten after every step (small and fast)
//!   meta.json    name, dates, size, where the data came from, and OpenRefine's metadata (tags...)
//! Reopening loads the original and replays the history, so undo/redo survives restarts.
use crate::{
    err,
    grel::{value::fmt_date, Value},
    import::ImportOptions,
    ops::Op,
    spill::{CodeBudget, CodeBuilder},
    table::{Column, Table},
    Result,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    collections::{HashMap, HashSet},
    io::{BufReader, BufWriter, Cursor, Read, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Meta {
    pub id: String,
    pub name: String,
    pub created: String,
    pub modified: String,
    pub rows: usize,
    pub columns: usize,
    #[serde(default)]
    pub source: Option<Source>,
    #[serde(default)]
    pub info: Info,
}

/// OpenRefine's project metadata, with its field names: tags, a description and the rest of what
/// its "About" dialog shows, and `customMetadata` from archives. Kept as typed, unchanged.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Info {
    pub tags: Vec<String>,
    pub title: String,
    pub description: String,
    pub creator: String,
    pub contributors: String,
    pub subject: String,
    pub version: String,
    pub license: String,
    pub homepage: String,
    pub image: String,
    #[serde(rename = "customMetadata")]
    pub custom: serde_json::Map<String, serde_json::Value>,
}

impl Info {
    /// Tags as OpenRefine's set-project-tags makes them: split at commas and spaces, blanks dropped
    /// (repeats stay, as there).
    pub fn tidy(mut self) -> Info {
        self.tags = self.tags.iter().flat_map(|t| t.split(|c: char| c == ',' || c.is_whitespace())).filter(|t| !t.is_empty()).map(String::from).collect();
        self
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Source {
    pub path: String,
    pub format: Option<String>,
    pub options: ImportOptions,
}

#[derive(Default, Serialize, Deserialize)]
pub struct History {
    pub ops: Vec<Op>,
    pub cursor: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub original: Option<crate::orproject::OriginalArchive>,
}

#[derive(Clone)]
pub struct Store {
    root: PathBuf,
    code_budget: CodeBudget,
    history_counts: Arc<Mutex<HashMap<String, CachedHistory>>>,
}

#[derive(Serialize)]
pub struct ListedProject {
    #[serde(flatten)]
    pub meta: Meta,
    pub stats: ProjectStats,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectStats {
    pub history_steps: Option<usize>,
    pub size_bytes: Option<u64>,
}

#[derive(Clone)]
struct CachedHistory {
    stamp: (u64, SystemTime),
    steps: Option<usize>,
}

// Older libraries are counted once per file version, without loading operations or their payloads.
const HISTORY_SCAN_LIMIT: u64 = 64 * 1024 * 1024;
const SIZE_ENTRY_LIMIT: usize = 10_000;

fn linked(metadata: &std::fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 { return true; }
    }
    metadata.file_type().is_symlink()
}

fn regular_file(path: &Path) -> Option<std::fs::Metadata> {
    let metadata = std::fs::symlink_metadata(path).ok()?;
    (metadata.is_file() && !linked(&metadata)).then_some(metadata)
}

fn history_stamp(path: &Path) -> Option<(u64, SystemTime)> {
    let metadata = regular_file(path)?;
    Some((metadata.len(), metadata.modified().ok()?))
}

fn count_operations<'de, D: serde::Deserializer<'de>>(deserializer: D) -> std::result::Result<usize, D::Error> {
    struct Counter;
    impl<'de> serde::de::Visitor<'de> for Counter {
        type Value = usize;
        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result { f.write_str("an operation array") }
        fn visit_seq<A: serde::de::SeqAccess<'de>>(self, mut seq: A) -> std::result::Result<usize, A::Error> {
            #[derive(Deserialize)]
            struct Name { op: String }
            let mut count = 0;
            while let Some(operation) = seq.next_element::<Name>()? {
                if operation.op.is_empty() { return Err(serde::de::Error::custom("missing operation name")); }
                count += 1;
            }
            Ok(count)
        }
    }
    deserializer.deserialize_seq(Counter)
}

fn history_steps(path: &Path, len: u64) -> Option<usize> {
    if len > HISTORY_SCAN_LIMIT { return None; }
    #[derive(Deserialize)]
    struct Count {
        #[serde(deserialize_with = "count_operations")]
        ops: usize,
        cursor: usize,
    }
    let reader = BufReader::new(std::fs::File::open(path).ok()?).take(HISTORY_SCAN_LIMIT + 1);
    let count: Count = serde_json::from_reader(reader).ok()?;
    (count.cursor <= count.ops).then_some(count.ops)
}

/// Logical bytes in regular files; links, unreadable entries and oversized trees are unknown.
fn project_bytes(path: &Path) -> Option<u64> {
    let mut directories = vec![path.to_owned()];
    let (mut bytes, mut entries) = (0u64, 0);
    while let Some(dir) = directories.pop() {
        let metadata = std::fs::symlink_metadata(&dir).ok()?;
        if !metadata.is_dir() || linked(&metadata) { return None; }
        for entry in std::fs::read_dir(dir).ok()? {
            entries += 1;
            if entries > SIZE_ENTRY_LIMIT { return None; }
            let path = entry.ok()?.path();
            let metadata = std::fs::symlink_metadata(&path).ok()?;
            if linked(&metadata) { return None; }
            if metadata.is_dir() { directories.push(path); }
            else if metadata.is_file() { bytes = bytes.checked_add(metadata.len())?; }
            else { return None; }
        }
    }
    Some(bytes)
}

fn now() -> String {
    chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

/// Write via a temporary file and rename, so a crash never leaves a half-written file.
pub(crate) fn write_atomic(path: &Path, f: impl FnOnce(&mut dyn Write) -> std::io::Result<()>) -> Result<()> {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let tmp = path.with_extension(format!("{}-{}.tmp", std::process::id(), NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
    let file = std::fs::OpenOptions::new().write(true).create_new(true).open(&tmp).map_err(err)?;
    let result = (|| {
        let mut w = BufWriter::new(file);
        f(&mut w).map_err(err)?;
        w.flush().map_err(err)?;
        drop(w);
        std::fs::rename(&tmp, path).map_err(err)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

impl Store {
    /// OXREFINE_HOME, else the platform's per-user data folder.
    pub fn default_root() -> PathBuf {
        if let Some(h) = std::env::var_os("OXREFINE_HOME") {
            return PathBuf::from(h);
        }
        let base = if cfg!(windows) {
            std::env::var_os("APPDATA").map(PathBuf::from)
        } else if cfg!(target_os = "macos") {
            std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Application Support"))
        } else {
            std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        };
        base.unwrap_or_else(std::env::temp_dir).join(if cfg!(any(windows, target_os = "macos")) { "OxRefine" } else { "oxrefine" })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn open(root: impl Into<PathBuf>) -> Result<Store> {
        let root = root.into();
        let budget = crate::storage::from_env(&root.join(".spill"))?;
        Self::open_with_budget(root, budget)
    }

    /// Explicit policy for tests, benchmarks and hosts that already own a shared code budget.
    pub fn open_with_budget(root: impl Into<PathBuf>, code_budget: CodeBudget) -> Result<Store> {
        let root = root.into().join("projects");
        std::fs::create_dir_all(&root).map_err(|e| format!("{}: {e}", root.display()))?;
        Ok(Store { root, code_budget, history_counts: Arc::new(Mutex::new(HashMap::new())) })
    }

    pub fn code_budget(&self) -> CodeBudget {
        self.code_budget.clone()
    }

    /// Project folder; ids are digits and dashes only, so a request can't reach outside the store.
    fn dir(&self, id: &str) -> Result<PathBuf> {
        if id.is_empty() || !id.chars().all(|c| c.is_ascii_digit() || c == '-') {
            return Err(format!("invalid project id {id:?}"));
        }
        Ok(self.root.join(id))
    }

    pub fn list(&self) -> Vec<Meta> {
        let mut out: Vec<Meta> = std::fs::read_dir(&self.root)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| serde_json::from_slice(&std::fs::read(e.path().join("meta.json")).ok()?).ok())
            .collect();
        out.sort_by(|a, b| b.modified.cmp(&a.modified));
        out
    }

    /// Home's derived statistics; ordinary metadata/cross() listing does not scan project files.
    pub fn list_with_stats(&self) -> Vec<ListedProject> {
        let mut out = vec![];
        for entry in std::fs::read_dir(&self.root).into_iter().flatten().flatten() {
            let path = entry.path();
            let Ok(metadata) = std::fs::symlink_metadata(&path) else { continue };
            if !metadata.is_dir() || linked(&metadata) { continue; }
            let Some(id) = path.file_name().and_then(|n| n.to_str()) else { continue };
            if self.dir(id).is_err() || regular_file(&path.join("meta.json")).is_none() { continue; }
            let Some(meta) = std::fs::read(path.join("meta.json")).ok().and_then(|b| serde_json::from_slice::<Meta>(&b).ok()) else { continue };
            if meta.id != id { continue; }
            let history = path.join("history.json");
            let steps = history_stamp(&history).and_then(|stamp| {
                let cached = self.history_counts.lock().unwrap_or_else(|e| e.into_inner()).get(id).cloned();
                if let Some(cached) = cached.filter(|cached| cached.stamp == stamp) { return cached.steps; }
                let steps = history_steps(&history, stamp.0);
                if history_stamp(&history) != Some(stamp) { return None; }
                self.history_counts.lock().unwrap_or_else(|e| e.into_inner()).insert(id.into(), CachedHistory { stamp, steps });
                steps
            });
            out.push(ListedProject { meta, stats: ProjectStats { history_steps: steps, size_bytes: project_bytes(&path) } });
        }
        out.sort_by(|a, b| b.meta.modified.cmp(&a.meta.modified));
        let ids: HashSet<_> = out.iter().map(|p| p.meta.id.as_str()).collect();
        self.history_counts.lock().unwrap_or_else(|e| e.into_inner()).retain(|id, _| ids.contains(id.as_str()));
        out
    }

    pub fn create(&self, name: &str, table: &Table, source: Option<Source>) -> Result<Meta> {
        crate::jobs::progress("Saving the project", 0);
        let ms = SystemTime::now().duration_since(UNIX_EPOCH).map_err(err)?.as_millis();
        let mut id = ms.to_string();
        let mut k = 1;
        while self.root.join(&id).exists() {
            id = format!("{ms}-{k}");
            k += 1;
        }
        let dir = self.dir(&id)?;
        std::fs::create_dir_all(&dir).map_err(err)?;
        write_table(&dir.join("data.oxt"), table)?;
        let meta = Meta { id, name: name.into(), created: now(), modified: now(), rows: table.rows, columns: table.names.len(), source, info: Info::default() };
        self.save(&meta, &History::default())?;
        Ok(meta)
    }

    pub fn save(&self, meta: &Meta, history: &History) -> Result<()> {
        crate::jobs::progress("Saving the project", 0);
        let dir = self.dir(&meta.id)?;
        if let Some(original) = &history.original {
            let path = dir.join("original.openrefine.tar.gz");
            if !path.exists() {
                original.validate()?;
                write_atomic(&path, |w| w.write_all(&original.bytes))?;
            }
        }
        write_atomic(&dir.join("history.json"), |w| serde_json::to_writer(w, history).map_err(std::io::Error::other))?;
        let result = write_atomic(&dir.join("meta.json"), |w| serde_json::to_writer_pretty(w, meta).map_err(std::io::Error::other));
        if result.is_ok() {
            if let Some(stamp) = history_stamp(&dir.join("history.json")) {
                self.history_counts.lock().unwrap_or_else(|e| e.into_inner()).insert(meta.id.clone(), CachedHistory { stamp, steps: Some(history.ops.len()) });
            }
        }
        crate::jobs::finish_stage(&result);
        result
    }

    pub fn load(&self, id: &str) -> Result<(Meta, Table, History)> {
        let dir = self.dir(id)?;
        let read = |f: &str| std::fs::read(dir.join(f)).map_err(|e| format!("project {id}: {f}: {e}"));
        let meta: Meta = serde_json::from_slice(&read("meta.json")?).map_err(err)?;
        let mut history: History = serde_json::from_slice(&read("history.json")?).map_err(err)?;
        if let Some(original) = &mut history.original {
            original.bytes = read("original.openrefine.tar.gz")?.into();
            original.validate()?;
        } else if dir.join("original.openrefine.tar.gz").exists() {
            return Err("a preserved original archive has no history manifest; the project cannot be opened safely".into());
        }
        let path = dir.join("data.oxt");
        let file = std::fs::File::open(&path).map_err(|e| format!("project {id}: data.oxt: {e}"))?;
        crate::jobs::progress_units("Reading the saved project", file.metadata().map_or(0, |m| m.len()), "bytes");
        let table = crate::storage::with_budget(self.code_budget(), || read_table_reader(file))?;
        Ok((meta, table, history))
    }

    pub fn rename(&self, id: &str, name: &str) -> Result<Meta> {
        let (mut meta, _, history) = self.load(id)?;
        meta.name = name.into();
        meta.modified = now();
        self.save(&meta, &history)?;
        Ok(meta)
    }

    /// Change a saved project's metadata without loading its data.
    pub fn set_info(&self, id: &str, info: Info) -> Result<Meta> {
        let path = self.dir(id)?.join("meta.json");
        let mut meta: Meta = serde_json::from_slice(&std::fs::read(&path).map_err(|e| format!("project {id}: {e}"))?).map_err(err)?;
        meta.info = info;
        meta.modified = now();
        write_atomic(&path, |w| serde_json::to_writer_pretty(w, &meta).map_err(std::io::Error::other))?;
        Ok(meta)
    }

    /// Preferences, shared by every project like OpenRefine's workspace preferences: names as in
    /// OpenRefine (`ui.browsing.listFacet.limit`...), any JSON value. In preferences.json next to the projects.
    pub fn preferences(&self) -> serde_json::Map<String, serde_json::Value> {
        let path = self.root.parent().unwrap_or(&self.root).join("preferences.json");
        std::fs::read(path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }

    /// Set one preference; null removes it.
    pub fn set_preference(&self, name: &str, value: serde_json::Value) -> Result<serde_json::Map<String, serde_json::Value>> {
        let mut prefs = self.preferences();
        if value.is_null() {
            prefs.remove(name);
        } else {
            prefs.insert(name.to_string(), value);
        }
        let path = self.root.parent().unwrap_or(&self.root).join("preferences.json");
        write_atomic(&path, |w| serde_json::to_writer_pretty(w, &prefs).map_err(std::io::Error::other))?;
        Ok(prefs)
    }

    pub fn delete(&self, id: &str) -> Result<()> {
        let dir = self.dir(id)?;
        if !dir.join("meta.json").exists() {
            return Err(format!("no project {id}"));
        }
        std::fs::remove_dir_all(dir).map_err(err)
    }

    pub fn touch(&self, meta: &mut Meta, t: &Table, history: &History) -> Result<()> {
        meta.modified = now();
        meta.rows = t.rows;
        meta.columns = t.names.len();
        self.save(meta, history)
    }
}

// ---------------------------------------------------------------- binary table format

const MAGIC: &[u8; 4] = b"OXT1";
const TABLE_CHUNK: usize = 64 * 1024;

fn damaged_table() -> String {
    "not an OxRefine data file (or it is damaged)".into()
}

fn read_table_exact(reader: &mut impl Read, bytes: &mut [u8]) -> Result<()> {
    reader.read_exact(bytes).map_err(|e| if crate::jobs::is_cancel(&e) {
        crate::jobs::CANCELLED.into()
    } else { format!("{}: {e}", damaged_table()) })?;
    crate::jobs::tick(0)
}

pub(crate) fn cell_to_json(v: &Value) -> serde_json::Value {
    match v {
        Value::Null => serde_json::Value::Null,
        Value::Str(s) => json!(s),
        Value::Int(i) => json!(i),
        Value::Num(n) => json!({ "n": n }),
        Value::Bool(b) => json!(b),
        Value::Date(d) => json!({ "d": fmt_date(d) }),
        Value::Error(e) => json!({ "e": e }),
        other => json!(other.to_java_string()),
    }
}

fn cell_from_json(j: serde_json::Value) -> Value {
    use serde_json::Value as J;
    match j {
        J::Null => Value::Null,
        J::String(s) => Value::Str(s),
        J::Bool(b) => Value::Bool(b),
        J::Number(n) => n.as_i64().map_or_else(|| Value::Num(n.as_f64().unwrap_or(f64::NAN)), Value::Int),
        J::Object(mut m) => {
            if let Some(n) = m.remove("n") {
                Value::Num(n.as_f64().unwrap_or(f64::NAN))
            } else if let Some(J::String(d)) = m.remove("d") {
                crate::grel::dates::parse_default(&d).map_or(Value::Str(d), Value::Date)
            } else if let Some(J::String(e)) = m.remove("e") {
                Value::Error(e)
            } else {
                Value::Null
            }
        }
        J::Array(_) => Value::Null,
    }
}

#[derive(Serialize, Deserialize)]
struct Header {
    names: Vec<String>,
    rows: usize,
    key: Option<String>,
    dicts: Vec<Vec<serde_json::Value>>,
    /// per column: null, or {"config", "recons", "rows"} (see recon::save_cells)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    recon: Vec<serde_json::Value>,
}

/// The dictionaries as the header's `dicts`, serialized value by value (no JSON tree of the whole
/// table in memory; a 1M-row column of unique values would cost more than the column itself).
struct Dicts<'a>(&'a [Arc<Column>]);
struct Dict<'a>(&'a [Value]);

impl Serialize for Dicts<'_> {
    fn serialize<S: serde::Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        s.collect_seq(self.0.iter().map(|c| Dict(&c.dict)))
    }
}

impl Serialize for Dict<'_> {
    fn serialize<S: serde::Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        s.collect_seq(self.0.iter().map(cell_to_json))
    }
}

#[derive(Serialize)]
struct HeaderOut<'a> {
    names: &'a [String],
    rows: usize,
    key: &'a Option<String>,
    dicts: Dicts<'a>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    recon: Vec<serde_json::Value>,
}

pub fn write_table(path: &Path, t: &Table) -> Result<()> {
    if t.names.len() != t.columns.len() || t.starred.len() != t.rows || t.flagged.len() != t.rows
        || t.columns.iter().any(|c| c.codes.len() != t.rows) {
        return Err("cannot save a table with inconsistent row or column lengths".into());
    }
    crate::jobs::tick(0)?;
    let header = HeaderOut {
        names: &t.names,
        rows: t.rows,
        key: &t.key,
        dicts: Dicts(&t.columns),
        recon: if t.columns.iter().any(|c| c.recon.is_some()) {
            t.columns
                .iter()
                .map(|c| match &c.recon {
                    None => serde_json::Value::Null,
                    Some(rc) => {
                        let mut j = crate::recon::save_cells(&rc.cells);
                        j["config"] = rc.config.clone().unwrap_or(serde_json::Value::Null);
                        j["stats"] = rc.stats.clone().unwrap_or(serde_json::Value::Null);
                        j
                    }
                })
                .collect()
        } else {
            vec![]
        },
    };
    let header = serde_json::to_vec(&header).map_err(err)?;
    crate::jobs::tick(0)?;
    write_atomic(path, |w| {
        w.write_all(MAGIC)?;
        w.write_all(&(header.len() as u64).to_le_bytes())?;
        w.write_all(&header)?;
        let mut bytes = [0u8; TABLE_CHUNK];
        for c in &t.columns {
            for codes in c.codes.chunks(TABLE_CHUNK / 4) {
                crate::jobs::tick(0).map_err(std::io::Error::other)?;
                for (out, code) in bytes.chunks_exact_mut(4).zip(codes) {
                    out.copy_from_slice(&code.to_le_bytes());
                }
                w.write_all(&bytes[..codes.len() * 4])?;
            }
        }
        for flags in [&t.starred, &t.flagged] {
            for flags in flags.chunks(TABLE_CHUNK) {
                crate::jobs::tick(0).map_err(std::io::Error::other)?;
                for (out, &flag) in bytes.iter_mut().zip(flags) { *out = u8::from(flag); }
                w.write_all(&bytes[..flags.len()])?;
            }
        }
        crate::jobs::tick(0).map_err(std::io::Error::other)
    })
}

pub fn read_table(bytes: &[u8]) -> Result<Table> {
    read_table_reader(Cursor::new(bytes))
}

/// OXT row codes are streamed into private owned storage under the active code budget.
/// Dictionaries, reconciliation metadata and flags remain resident; no whole data-file buffer is made.
pub fn read_table_reader(reader: impl Read) -> Result<Table> {
    let mut reader = BufReader::with_capacity(TABLE_CHUNK, crate::jobs::Counted(reader));
    crate::jobs::tick(0)?;
    let mut prefix = [0u8; 12];
    read_table_exact(&mut reader, &mut prefix)?;
    if &prefix[..4] != MAGIC { return Err(damaged_table()); }
    let header_len = usize::try_from(u64::from_le_bytes(prefix[4..].try_into().unwrap()))
        .ok().filter(|&n| n <= isize::MAX as usize).ok_or_else(damaged_table)?;
    let mut header_reader = (&mut reader).take(header_len as u64);
    let header = serde_json::from_reader::<_, Header>(&mut header_reader);
    // A counted reader can interrupt JSON parsing; preserve the job's cancellation outcome.
    crate::jobs::tick(0)?;
    let header = header.map_err(err)?;
    if header_reader.limit() != 0 || header.names.len() != header.dicts.len() { return Err(damaged_table()); }
    let n = header.rows;
    let column_bytes = n.checked_mul(4).filter(|&n| n <= isize::MAX as usize).ok_or_else(damaged_table)?;
    column_bytes.checked_mul(header.dicts.len()).and_then(|bytes| n.checked_mul(2).and_then(|flags| bytes.checked_add(flags)))
        .and_then(|payload| payload.checked_add(header_len)).and_then(|len| len.checked_add(12)).ok_or_else(damaged_table)?;
    let budget = crate::storage::current_budget();
    let mut columns = vec![];
    let mut bytes = [0u8; TABLE_CHUNK];
    let mut block = [0u32; TABLE_CHUNK / 4];
    for (ci, dict) in header.dicts.into_iter().enumerate() {
        let dict: Vec<Value> = dict.into_iter().map(cell_from_json).collect();
        let mut codes = CodeBuilder::new(budget.clone());
        let mut remaining = n;
        while remaining > 0 {
            let count = remaining.min(block.len());
            read_table_exact(&mut reader, &mut bytes[..count * 4])?;
            for (code, raw) in block[..count].iter_mut().zip(bytes[..count * 4].chunks_exact(4)) {
                *code = u32::from_le_bytes(raw.try_into().unwrap());
                if *code as usize >= dict.len() { return Err(damaged_table()); }
            }
            codes.extend_slice(&block[..count])?;
            remaining -= count;
        }
        let recon = header.recon.get(ci).filter(|j| !j.is_null()).and_then(|j| {
            let cells = crate::recon::load_cells(j, n)?;
            let field = |k: &str| j.get(k).filter(|c| !c.is_null()).cloned();
            Some(Arc::new(crate::recon::ReconCol { cells, config: field("config"), stats: field("stats") }))
        });
        columns.push(Arc::new(Column { dict, codes: codes.finish()?, recon }));
    }
    let mut flags = || -> Result<Arc<Vec<bool>>> {
        let mut out = Vec::new();
        while out.len() < n {
            let count = (n - out.len()).min(bytes.len());
            read_table_exact(&mut reader, &mut bytes[..count])?;
            if bytes[..count].iter().any(|&b| b > 1) { return Err(damaged_table()); }
            out.try_reserve(count).map_err(|e| format!("allocating row flags: {e}"))?;
            out.extend(bytes[..count].iter().map(|&b| b != 0));
        }
        Ok(Arc::new(out))
    };
    let starred = flags()?;
    let flagged = flags()?;
    let mut extra = [0u8; 1];
    if reader.read(&mut extra).map_err(|e| if crate::jobs::is_cancel(&e) { crate::jobs::CANCELLED.into() } else { err(e) })? != 0 {
        return Err(damaged_table());
    }
    crate::jobs::tick(0)?;
    Ok(Table { names: header.names, columns, rows: n, starred, flagged, key: header.key })
}
