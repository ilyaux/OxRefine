use crate::{
    cluster,
    engine::{self, EngineConfig},
    err, grel,
    ops::Op,
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
}

impl Project {
    pub fn open(path: &Path) -> Result<Project> {
        Ok(Project {
            name: path.file_name().map(|n| n.to_string_lossy().into()).unwrap_or_default(),
            states: vec![Table::read_csv(path)?],
            ops: vec![],
            cursor: 0,
            gen: 0,
            cache: None,
        })
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

    /// Operations up to the current step, in OpenRefine's JSON shape.
    pub fn recipe(&self) -> &[Op] {
        &self.ops[..self.cursor]
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
            "name": self.name,
            "columns": t.names,
            "rows": t.rows,
            "history": self.ops.iter().map(Op::description).collect::<Vec<_>>(),
            "cursor": self.cursor,
        })
    }
}

/// A cell for the grid: null, or the text OpenRefine shows (numbers and dates formatted).
fn cell_json(v: &grel::Value) -> Value {
    if v.is_null() { Value::Null } else { Value::String(v.display()) }
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Args {
    path: String,
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
}

/// Single JSON entry point shared by the Tauri app and `ox serve`. Returns `{data, ms}`.
pub fn dispatch(slot: &mut Option<Project>, cmd: &str, args: Value) -> Result<Value> {
    let t0 = Instant::now();
    let data = run(slot, cmd, serde_json::from_value(args).map_err(err)?)?;
    Ok(json!({ "data": data, "ms": t0.elapsed().as_secs_f64() * 1000.0 }))
}

fn run(slot: &mut Option<Project>, cmd: &str, a: Args) -> Result<Value> {
    if cmd == "open" {
        *slot = Some(Project::open(Path::new(&a.path))?);
    }
    let p = slot.as_mut().ok_or("no project open")?;
    Ok(match cmd {
        "open" | "summary" => p.summary(),
        "rows" => {
            let rows = p.rows(&a.engine)?;
            let t = p.table();
            let page: Vec<_> = rows[a.start.min(rows.len())..]
                .iter()
                .take(a.limit.min(1000))
                .map(|&r| {
                    let r = r as usize;
                    json!([r, t.columns.iter().map(|c| cell_json(c.get(r))).collect::<Vec<_>>(), t.starred[r], t.flagged[r]])
                })
                .collect();
            json!({ "total": rows.len(), "rows": page })
        }
        "facets" => json!(engine::facet_counts(p.table(), &a.engine)?),
        "apply" => {
            p.apply(a.op.ok_or("missing op")?)?;
            p.summary()
        }
        "apply_recipe" => {
            for (i, op) in a.ops.into_iter().enumerate() {
                p.apply(op).map_err(|e| format!("step {}: {e}", i + 1))?;
            }
            p.summary()
        }
        "goto" => {
            p.goto(a.index)?;
            p.summary()
        }
        "cluster" => {
            let rows = p.rows(&a.engine)?;
            let t = p.table();
            let vc = engine::value_counts(t, t.col(&a.column)?, &rows);
            let vc: Vec<(&str, u32)> = vc.iter().map(|(v, c)| (v.as_str(), *c)).collect();
            json!(cluster::cluster(&vc, &a.method, if a.n == 0 { 2 } else { a.n })?)
        }
        "preview" => {
            let e = grel::compile(&a.expression)?;
            let rows = p.rows(&a.engine)?;
            let t = p.table();
            let col = &t.columns[t.col(&a.column)?];
            // [before, after] as the expression dialog shows them: null, text, or {"error": message}
            let pairs = rows.iter().take(a.limit.clamp(1, 100)).map(|&r| {
                let v = col.get(r as usize);
                let out = match e.eval(v.clone(), &t.row(r as usize)) {
                    grel::Value::Error(m) => json!({ "error": m }),
                    out => json!(out.render()),
                };
                json!([cell_json(v), out])
            });
            json!(pairs.collect::<Vec<_>>())
        }
        "export_csv" => {
            let rows = p.rows(&a.engine)?;
            p.table().write_csv(Path::new(&a.path), &rows)?;
            json!(rows.len())
        }
        "export_recipe" => {
            let r = json!(p.recipe());
            if !a.path.is_empty() {
                std::fs::write(&a.path, serde_json::to_string_pretty(&r).map_err(err)?).map_err(err)?;
            }
            r
        }
        _ => return Err(format!("unknown command `{cmd}`")),
    })
}
