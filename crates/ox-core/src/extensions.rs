//! Extensions: folders with a `main.py` that add GREL functions, importers, exporters and operations
//! without rebuilding OxRefine (OpenRefine's equivalent is a Butterfly module in Java). They run in the
//! embedded Python (grel/python.rs) and are found in `<data folder>/extensions/<name>/` and in the
//! folders listed in `OXREFINE_EXTENSIONS`. An extension is trusted code: it runs with the same rights
//! as OxRefine. The API is documented in docs/src/extensions.md; docs/extensions/text-tools is an example.
use crate::{
    grel::{python, Value},
    table::{Column, Table},
    Result,
};
use serde::Serialize;
use serde_json::{json, Value as J};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, RwLock},
};

/// What a loaded extension provides, as the UI and the `extensions` command list it.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Extension {
    pub name: String,
    pub version: String,
    pub description: String,
    pub path: String,
    /// What main.py registered: `{kind, id, label?, ...}`.
    pub items: Vec<J>,
    /// Why it failed to load; a failed extension provides nothing.
    pub error: Option<String>,
    #[serde(skip)]
    pub source: String,
    /// Changes when the extension is reloaded, so every thread's interpreter runs the new code.
    #[serde(skip)]
    pub generation: u64,
}

static REGISTRY: RwLock<Vec<Arc<Extension>>> = RwLock::new(vec![]);
static GENERATION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// The code each thread's interpreter has to run.
pub struct Source {
    pub name: String,
    pub source: String,
    pub path: String,
    pub version: u64,
}

pub(crate) fn sources() -> Vec<Source> {
    REGISTRY.read().unwrap().iter().filter(|e| e.error.is_none()).map(|e| Source { name: e.name.clone(), source: e.source.clone(), path: e.path.clone(), version: e.generation }).collect()
}

/// Folders searched for extensions: `<root>/extensions`, then each folder in `OXREFINE_EXTENSIONS`.
pub fn folders(root: &Path) -> Vec<PathBuf> {
    let mut out = vec![root.join("extensions")];
    if let Some(extra) = std::env::var_os("OXREFINE_EXTENSIONS") {
        out.extend(std::env::split_paths(&extra));
    }
    out
}

/// (Re)load every extension in `folders`, replacing what was loaded before. A folder that is missing
/// is skipped; an extension that fails is listed with its error.
pub fn load(folders: &[PathBuf]) -> Vec<Extension> {
    let mut found = vec![];
    for folder in folders {
        let Ok(entries) = std::fs::read_dir(folder) else { continue };
        let mut dirs: Vec<PathBuf> = entries.flatten().map(|e| e.path()).filter(|p| p.join("main.py").is_file()).collect();
        dirs.sort();
        for dir in dirs {
            found.push(load_one(&dir));
        }
    }
    // later folders can't shadow an extension of the same name
    let mut seen = std::collections::HashSet::new();
    let mut unique = vec![];
    for mut e in found {
        if !seen.insert(e.name.clone()) {
            e.error = Some(format!("another extension named {} was loaded first", e.name));
            e.items.clear();
        }
        unique.push(e);
    }
    check_conflicts(&mut unique);
    *REGISTRY.write().unwrap() = unique.iter().cloned().map(Arc::new).collect();
    unique
}

fn load_one(dir: &Path) -> Extension {
    let folder_name = dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let manifest: J = std::fs::read_to_string(dir.join("extension.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(J::Null);
    let name = manifest["name"].as_str().unwrap_or(&folder_name).to_string();
    let path = dir.join("main.py");
    let mut ext = Extension {
        version: manifest["version"].as_str().unwrap_or("").into(),
        description: manifest["description"].as_str().unwrap_or("").into(),
        path: path.display().to_string(),
        items: vec![],
        error: None,
        source: String::new(),
        generation: GENERATION.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        name,
    };
    if !ext.name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') || ext.name.is_empty() {
        ext.error = Some(format!("extension name `{}` may only use letters, digits, - and _", ext.name));
        return ext;
    }
    match std::fs::read_to_string(&path) {
        Err(e) => ext.error = Some(format!("{}: {e}", path.display())),
        Ok(source) => match python::load_extension(&ext.name, &source, &ext.path, ext.generation) {
            Ok(items) => {
                ext.items = serde_json::from_str(&items).unwrap_or_default();
                ext.source = source;
            }
            Err(e) => ext.error = Some(e),
        },
    }
    ext
}

/// Two extensions can't register the same function, importer, exporter or operation id, and an
/// extension function can't hide a built-in GREL function.
fn check_conflicts(exts: &mut [Extension]) {
    let mut owners: std::collections::HashMap<(String, String), String> = Default::default();
    for e in exts.iter_mut() {
        let mut problems = vec![];
        for item in &e.items {
            let (kind, id) = (item["kind"].as_str().unwrap_or("").to_string(), item["id"].as_str().unwrap_or("").to_string());
            if kind == "function" && crate::grel::funcs::is_function(&id) {
                problems.push(format!("function {id} is built into GREL"));
            } else if let Some(other) = owners.get(&(kind.clone(), id.clone())) {
                problems.push(format!("{kind} {id} is already provided by {other}"));
            } else {
                owners.insert((kind, id), e.name.clone());
            }
        }
        if !problems.is_empty() {
            e.error = Some(problems.join("; "));
            e.items.clear();
        }
    }
}

pub fn list() -> Vec<Extension> {
    REGISTRY.read().unwrap().iter().map(|e| (**e).clone()).collect()
}

fn find(kind: &str, id: &str) -> Option<(Arc<Extension>, J)> {
    REGISTRY.read().unwrap().iter().find_map(|e| e.items.iter().find(|i| i["kind"] == kind && i["id"] == id).map(|i| (e.clone(), i.clone())))
}

/// Whether `name` is a GREL function some extension provides.
pub fn has_function(name: &str) -> bool {
    find("function", name).is_some()
}

pub fn call_function(name: &str, args: &[Value]) -> Value {
    python::ext_function(name, args)
}

/// The importer an extension registered for this file's extension, as an `ext:<id>` format.
pub fn importer_for(path: &Path) -> Option<(String, String)> {
    let ext = path.extension()?.to_string_lossy().to_ascii_lowercase();
    REGISTRY.read().unwrap().iter().find_map(|e| {
        e.items.iter().find(|i| i["kind"] == "importer" && i["extensions"].as_array().is_some_and(|x| x.iter().any(|x| x == ext.as_str()))).map(|i| {
            (format!("ext:{}", i["id"].as_str().unwrap_or("")), i["label"].as_str().unwrap_or("").to_string())
        })
    })
}

fn to_table((names, rows): python::Rows, limit: i64) -> Result<Table> {
    let names = crate::import::column_names(names, usize::MAX);
    let rows: Vec<Vec<Value>> = if limit > 0 { rows.into_iter().take(limit as usize).collect() } else { rows };
    let n = rows.len();
    let columns = (0..names.len()).map(|c| Column::from_values(rows.iter().map(|r| r.get(c).cloned().unwrap_or(Value::Null)))).collect();
    Ok(Table::new(names, columns, n))
}

fn rows_of(t: &Table, rows: &[u32]) -> python::Rows {
    let cells = rows.iter().map(|&r| t.columns.iter().map(|c| c.get(r as usize).clone()).collect()).collect();
    (t.names.clone(), cells)
}

/// Import with the importer `id` (the part after `ext:`). The importer gets the file's bytes and the
/// import options; `limit` is applied afterwards.
pub fn import(id: &str, path: &Path, o: &crate::import::ImportOptions) -> Result<Table> {
    find("importer", id).ok_or_else(|| format!("no extension provides an importer named {id}"))?;
    let data = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let options = serde_json::to_value(o).unwrap_or(J::Null);
    to_table(python::ext_import(id, &data, &options).map_err(|e| format!("importer {id}: {e}"))?, o.limit)
}

/// Export `rows` of `t` with the exporter `id`.
pub fn export(id: &str, t: &Table, rows: &[u32], params: &J) -> Result<Vec<u8>> {
    find("exporter", id).ok_or_else(|| format!("no extension provides an exporter named {id}"))?;
    let (names, cells) = rows_of(t, rows);
    python::ext_export(id, &names, &cells, params).map_err(|e| format!("exporter {id}: {e}"))
}

/// Run the operation `id` on the whole table. Rows the operation keeps are rebuilt from what it returns;
/// stars, flags and reconciliation data don't survive it.
pub fn operation(id: &str, t: &Table, params: &J) -> Result<Table> {
    find("operation", id).ok_or_else(|| format!("no extension provides an operation named {id}; install it to replay this step"))?;
    let all: Vec<u32> = (0..t.rows as u32).collect();
    let (names, cells) = rows_of(t, &all);
    let mut out = to_table(python::ext_operation(id, &names, &cells, params).map_err(|e| format!("operation {id}: {e}"))?, -1)?;
    out.key = t.key.clone().filter(|k| out.names.contains(k));
    Ok(out)
}

/// The `extensions` command: what is loaded, and where extensions are looked for.
pub fn describe(root: Option<&Path>) -> J {
    json!({
        "folders": root.map(|r| folders(r).iter().map(|f| f.display().to_string()).collect::<Vec<_>>()).unwrap_or_default(),
        "extensions": list(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn example_extension_loads_and_runs() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/extensions");
        let loaded = load(&[dir]);
        let tools = loaded.iter().find(|e| e.name == "text-tools").expect("text-tools loads");
        assert!(tools.error.is_none(), "{:?}", tools.error);
        assert!(has_function("slugify"));
        let p = crate::grel::compile("slugify(value)").unwrap();
        assert_eq!(p.eval(Value::Str("Hello, World!".into()), &crate::grel::NoRow), Value::Str("hello-world".into()));
        // a thread that never loaded it picks the extension up on first use
        let other = std::thread::spawn(|| {
            let v = crate::grel::compile("wordCount(value)").unwrap().eval(Value::Str("a b  c".into()), &crate::grel::NoRow);
            python::release_thread();
            v
        });
        assert_eq!(other.join().unwrap(), Value::Int(3));
        let t = Table::new(vec!["a".into()], vec![Column::from_values([Value::Str("x".into()), Value::Str("y".into())])], 2);
        let numbered = operation("number-rows", &t, &json!({"column": "n", "start": 10})).unwrap();
        assert_eq!(numbered.names, vec!["n", "a"]);
        assert_eq!(numbered.columns[0].get(1), &Value::Int(11));
        let md = String::from_utf8(export("markdown", &t, &[0, 1], &J::Null).unwrap()).unwrap();
        assert_eq!(md, "| a |\n| --- |\n| x |\n| y |\n");
    }
}
