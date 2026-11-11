//! Saved projects: one folder per project in the data directory.
//!   data.oxt     original table, written once at import (binary: JSON header + u32 codes)
//!   history.json operations and cursor, rewritten after every step (small and fast)
//!   meta.json    name, dates, size and where the data came from
//! Reopening loads the original and replays the history, so undo/redo survives restarts.
use crate::{
    err,
    grel::{value::fmt_date, Value},
    import::ImportOptions,
    ops::Op,
    table::{Column, Table},
    Result,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    io::{BufWriter, Write},
    path::{Path, PathBuf},
    sync::Arc,
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
}

#[derive(Clone)]
pub struct Store {
    root: PathBuf,
}

fn now() -> String {
    chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

/// Write via a temporary file and rename, so a crash never leaves a half-written file.
fn write_atomic(path: &Path, f: impl FnOnce(&mut dyn Write) -> std::io::Result<()>) -> Result<()> {
    let tmp = path.with_extension("tmp");
    {
        let mut w = BufWriter::new(std::fs::File::create(&tmp).map_err(err)?);
        f(&mut w).map_err(err)?;
        w.flush().map_err(err)?;
    }
    std::fs::rename(&tmp, path).map_err(err)
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
        let root = root.into().join("projects");
        std::fs::create_dir_all(&root).map_err(|e| format!("{}: {e}", root.display()))?;
        Ok(Store { root })
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

    pub fn create(&self, name: &str, table: &Table, source: Option<Source>) -> Result<Meta> {
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
        let meta = Meta { id, name: name.into(), created: now(), modified: now(), rows: table.rows, columns: table.names.len(), source };
        self.save(&meta, &History::default())?;
        Ok(meta)
    }

    pub fn save(&self, meta: &Meta, history: &History) -> Result<()> {
        let dir = self.dir(&meta.id)?;
        write_atomic(&dir.join("history.json"), |w| serde_json::to_writer(w, history).map_err(std::io::Error::other))?;
        write_atomic(&dir.join("meta.json"), |w| serde_json::to_writer_pretty(w, meta).map_err(std::io::Error::other))
    }

    pub fn load(&self, id: &str) -> Result<(Meta, Table, History)> {
        let dir = self.dir(id)?;
        let read = |f: &str| std::fs::read(dir.join(f)).map_err(|e| format!("project {id}: {f}: {e}"));
        let meta: Meta = serde_json::from_slice(&read("meta.json")?).map_err(err)?;
        let history: History = serde_json::from_slice(&read("history.json")?).map_err(err)?;
        Ok((meta, read_table(&read("data.oxt")?)?, history))
    }

    pub fn rename(&self, id: &str, name: &str) -> Result<Meta> {
        let (mut meta, _, history) = self.load(id)?;
        meta.name = name.into();
        meta.modified = now();
        self.save(&meta, &history)?;
        Ok(meta)
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

fn cell_to_json(v: &Value) -> serde_json::Value {
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
    write_atomic(path, |w| {
        w.write_all(MAGIC)?;
        w.write_all(&(header.len() as u64).to_le_bytes())?;
        w.write_all(&header)?;
        for c in &t.columns {
            for code in c.codes.iter() {
                w.write_all(&code.to_le_bytes())?;
            }
        }
        w.write_all(&t.starred.iter().map(|&b| b as u8).collect::<Vec<_>>())?;
        w.write_all(&t.flagged.iter().map(|&b| b as u8).collect::<Vec<_>>())
    })
}

pub fn read_table(bytes: &[u8]) -> Result<Table> {
    let bad = || "not an OxRefine data file (or it is damaged)".to_string();
    if bytes.get(..4) != Some(MAGIC) {
        return Err(bad());
    }
    let len = u64::from_le_bytes(bytes.get(4..12).ok_or_else(bad)?.try_into().unwrap()) as usize;
    let header: Header = serde_json::from_slice(bytes.get(12..12 + len).ok_or_else(bad)?).map_err(err)?;
    let mut pos = 12 + len;
    let n = header.rows;
    let mut columns = vec![];
    for (ci, dict) in header.dicts.into_iter().enumerate() {
        let raw = bytes.get(pos..pos + 4 * n).ok_or_else(bad)?;
        let codes: Vec<u32> = raw.chunks_exact(4).map(|c| u32::from_le_bytes(c.try_into().unwrap())).collect();
        let dict: Vec<Value> = dict.into_iter().map(cell_from_json).collect();
        if codes.iter().any(|&c| c as usize >= dict.len()) {
            return Err(bad());
        }
        let recon = header.recon.get(ci).filter(|j| !j.is_null()).and_then(|j| {
            let cells = crate::recon::load_cells(j, n)?;
            let field = |k: &str| j.get(k).filter(|c| !c.is_null()).cloned();
            Some(Arc::new(crate::recon::ReconCol { cells, config: field("config"), stats: field("stats") }))
        });
        columns.push(Arc::new(Column { dict, codes: codes.into(), recon }));
        pos += 4 * n;
    }
    let flags = |pos: usize| -> Result<Arc<Vec<bool>>> { Ok(Arc::new(bytes.get(pos..pos + n).ok_or_else(bad)?.iter().map(|&b| b != 0).collect())) };
    Ok(Table { names: header.names, columns, rows: n, starred: flags(pos)?, flagged: flags(pos + n)?, key: header.key })
}
