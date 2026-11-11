//! OpenRefine project archives (`.openrefine.tar.gz`): open them with their history, write them back.
//!
//! An archive holds the table as it is now (`data.zip` → `data.txt`) and, for every history step, a
//! change file (`history/<id>.change.zip`) recording what the step changed, old and new. OpenRefine
//! keeps no copy of the original data: earlier tables come from undoing those changes one by one.
//! `read` does exactly that, so it knows the table at every step as OpenRefine had it.
//!
//! OxRefine's own history is operations replayed from a starting table. `into_project` turns each
//! OpenRefine step into an OxRefine operation and keeps it only when replaying it gives OpenRefine's
//! table. A step it can't reproduce (an operation OxRefine lacks, a change file it can't read) is
//! merged into the starting table along with everything before it, so the data is always exactly
//! OpenRefine's; undone steps after such a point are dropped.
use crate::{
    err,
    grel::{value::fmt_date, Value},
    ops::{Op, OpKind},
    project::Project,
    recon::{Recon, ReconCol},
    table::{Column, Interner, Table},
    Result,
};
use serde::Deserialize;
use serde_json::{json, Value as J};
use std::{
    collections::{BTreeMap, HashMap},
    io::{BufRead, BufReader, Cursor, Read, Write},
    path::Path,
    sync::Arc,
};

/// What OpenRefine 3.10.1 writes at the top of its data and change files.
const VERSION: &str = "3.10-SNAPSHOT";

/// Whether a file is an OpenRefine project export: a gzipped tar with OpenRefine's `data.zip` in it
/// (a tar.gz of data files is imported as an archive instead).
pub fn is_archive(path: &Path) -> bool {
    let Ok(f) = std::fs::File::open(path) else { return false };
    let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(BufReader::new(f)));
    let Ok(entries) = tar.entries() else { return false };
    // OpenRefine writes data.zip first; a few entries are enough to tell
    entries.take(64).flatten().any(|e| e.path().is_ok_and(|p| p.to_string_lossy().trim_start_matches("./") == "data.zip"))
}

// ---------------------------------------------------------------- OpenRefine's project model

/// A column in OpenRefine's column model: rows keep cells by `cell` index, whatever the column order.
#[derive(Clone, Debug, Deserialize)]
struct Col {
    #[serde(rename = "cellIndex")]
    cell: usize,
    name: String,
    /// the last reconcile operation's config, kept by the column
    #[serde(rename = "reconConfig", default)]
    recon: Option<J>,
    #[serde(rename = "reconStats", default)]
    stats: Option<J>,
}

/// A cell: its value and Recon.
type Cell = (Value, Option<Arc<Recon>>);

/// The Recons a data or change file names, by id (its pool.txt).
type Pool = HashMap<String, Arc<Recon>>;

fn read_pool(text: &str) -> Pool {
    text.lines()
        .filter(|l| l.starts_with('{'))
        .filter_map(|l| serde_json::from_str::<J>(l).ok())
        .map(|j| {
            let r = Recon::from_json(&j);
            (r.id.to_string(), Arc::new(r))
        })
        .collect()
}

/// A row as OpenRefine saves it, cells by cell index.
#[derive(Clone)]
struct Row {
    starred: bool,
    flagged: bool,
    cells: Vec<Cell>,
}

/// The project at one step: the column model plus the cells of every cell index, including those
/// of columns no longer in the model (OpenRefine keeps them too, and an undo can bring them back).
#[derive(Clone)]
struct State {
    cols: Vec<Col>,
    data: BTreeMap<usize, Arc<Column>>,
    rows: usize,
    starred: Arc<Vec<bool>>,
    flagged: Arc<Vec<bool>>,
    key: Option<String>,
}

fn null_column(rows: usize) -> Column {
    Column { dict: vec![Value::Null], codes: vec![0; rows].into(), recon: None }
}

/// A cell of OpenRefine's JSON: null | {"v": value, "t"?: "date", "r"?: recon id} | {"e": message}.
fn cell(j: J, pool: &Pool) -> Cell {
    let J::Object(mut m) = j else { return (Value::Null, None) };
    let recon = match m.remove("r") {
        Some(J::String(id)) => pool.get(&id).cloned(),
        Some(J::Number(id)) => pool.get(&id.to_string()).cloned(),
        Some(r @ J::Object(_)) => Some(Arc::new(Recon::from_json(&r))),
        _ => None,
    };
    if let Some(J::String(e)) = m.remove("e") {
        return (Value::Error(e), recon);
    }
    let date = m.get("t").and_then(J::as_str) == Some("date");
    let v = match m.remove("v") {
        Some(J::String(s)) if date => chrono::DateTime::parse_from_rfc3339(&s).map_or(Value::Str(s), |d| Value::Date(d.naive_utc())),
        Some(J::String(s)) => Value::Str(s),
        Some(J::Number(n)) => n.as_i64().map_or_else(|| Value::Num(n.as_f64().unwrap_or(f64::NAN)), Value::Int),
        Some(J::Bool(b)) => Value::Bool(b),
        _ => Value::Null,
    };
    (v, recon)
}

/// A cell as OpenRefine writes it (non-finite numbers become text, as OpenRefine itself does).
fn cell_json(v: &Value) -> J {
    match v {
        Value::Null => J::Null,
        Value::Str(s) => json!({ "v": s }),
        Value::Int(i) => json!({ "v": i }),
        Value::Num(n) if n.is_finite() => json!({ "v": n }),
        Value::Num(n) => json!({ "v": crate::grel::value::java_double(*n) }),
        Value::Bool(b) => json!({ "v": b }),
        Value::Date(d) => json!({ "v": fmt_date(d), "t": "date" }),
        Value::Error(e) => json!({ "e": e }),
        other => json!({ "v": other.to_java_string() }),
    }
}

fn opt_cell(s: &str, pool: &Pool) -> Result<Cell> {
    if s.is_empty() {
        Ok((Value::Null, None))
    } else {
        Ok(cell(serde_json::from_str(s).map_err(err)?, pool))
    }
}

fn row(s: &str, pool: &Pool) -> Result<Row> {
    #[derive(Deserialize)]
    struct R {
        #[serde(default)]
        starred: bool,
        #[serde(default)]
        flagged: bool,
        #[serde(default)]
        cells: Vec<J>,
    }
    let r: R = serde_json::from_str(s).map_err(|e| format!("a row: {e}"))?;
    Ok(Row { starred: r.starred, flagged: r.flagged, cells: r.cells.into_iter().map(|c| cell(c, pool)).collect() })
}

/// Columns built row by row, for every cell index that appears.
#[derive(Default)]
struct Builder {
    cols: Vec<(Interner, Vec<u32>, Vec<Option<Arc<Recon>>>)>,
    rows: usize,
    starred: Vec<bool>,
    flagged: Vec<bool>,
}

impl Builder {
    fn push(&mut self, r: Row) {
        while self.cols.len() < r.cells.len() {
            let mut i = Interner::default();
            let null = i.intern(Value::Null);
            self.cols.push((i, vec![null; self.rows], vec![None; self.rows]));
        }
        let mut cells = r.cells.into_iter();
        for (i, codes, recons) in &mut self.cols {
            let (v, recon) = cells.next().unwrap_or((Value::Null, None));
            codes.push(i.intern(v));
            recons.push(recon);
        }
        self.starred.push(r.starred);
        self.flagged.push(r.flagged);
        self.rows += 1;
    }

    fn state(self, cols: Vec<Col>, key: Option<String>) -> State {
        let data = self
            .cols
            .into_iter()
            .enumerate()
            .map(|(c, (i, codes, recons))| {
                let recon = recons.iter().any(Option::is_some).then(|| Arc::new(ReconCol::new(recons)));
                (c, Arc::new(Column { dict: i.finish(), codes: codes.into(), recon }))
            })
            .collect();
        State { cols, data, rows: self.rows, starred: Arc::new(self.starred), flagged: Arc::new(self.flagged), key }
    }
}

fn from_rows(cols: Vec<Col>, rows: &[Row], key: Option<String>) -> State {
    let mut b = Builder::default();
    rows.iter().cloned().for_each(|r| b.push(r));
    b.state(cols, key)
}

impl State {
    /// The table OxRefine shows: the model's columns in order.
    fn table(&self) -> Table {
        let columns = self
            .cols
            .iter()
            .map(|c| {
                let col = self.data.get(&c.cell).cloned().unwrap_or_else(|| Arc::new(null_column(self.rows)));
                // the column model keeps the reconConfig and stats; OxRefine keeps them with the cells
                if c.recon.is_none() && c.stats.is_none() {
                    return col;
                }
                let cells = col.recon.as_ref().map_or_else(|| vec![None; self.rows], |rc| rc.cells.clone());
                let recon = Some(Arc::new(ReconCol { cells, config: c.recon.clone(), stats: c.stats.clone() }));
                Arc::new(Column { dict: col.dict.clone(), codes: col.codes.clone(), recon })
            })
            .collect();
        Table {
            names: self.cols.iter().map(|c| c.name.clone()).collect(),
            columns,
            rows: self.rows,
            starred: self.starred.clone(),
            flagged: self.flagged.clone(),
            key: self.key.clone(),
        }
    }

    /// Cells set by (row, cell index, value); only the touched cell indexes are rebuilt.
    fn set_cells(&self, edits: impl IntoIterator<Item = (usize, usize, Cell)>) -> Result<State> {
        let mut by: BTreeMap<usize, Vec<(usize, Cell)>> = BTreeMap::new();
        for (r, c, v) in edits {
            if r >= self.rows {
                return Err(format!("a change names row {r}, but there are {} rows", self.rows));
            }
            by.entry(c).or_default().push((r, v));
        }
        let mut out = self.clone();
        for (c, edits) in by {
            let old = self.data.get(&c).map_or_else(|| null_column(self.rows), |c| Column { dict: c.dict.clone(), codes: c.codes.clone(), recon: c.recon.clone() });
            out.data.insert(c, Arc::new(patch(old, edits)));
        }
        Ok(out)
    }

    /// Whole rows put in place (cells of every cell index, star and flag).
    fn put_rows(&self, rows: &[(usize, &Row)]) -> Result<State> {
        let width = rows.iter().map(|(_, r)| r.cells.len()).max().unwrap_or(0);
        let mut idx: Vec<usize> = self.data.keys().copied().collect();
        idx.extend(0..width);
        idx.sort_unstable();
        idx.dedup();
        let edits = rows.iter().flat_map(|(r, row)| idx.iter().map(move |&c| (*r, c, row.cells.get(c).cloned().unwrap_or((Value::Null, None)))));
        let mut out = self.set_cells(edits.collect::<Vec<_>>())?;
        let (mut s, mut f) = ((*out.starred).clone(), (*out.flagged).clone());
        for (r, row) in rows {
            s[*r] = row.starred;
            f[*r] = row.flagged;
        }
        out.starred = Arc::new(s);
        out.flagged = Arc::new(f);
        Ok(out)
    }

    /// Rows picked in a new order; `None` makes an empty row.
    fn pick(&self, order: &[Option<u32>]) -> State {
        let flag = |f: &Vec<bool>| Arc::new(order.iter().map(|r| r.is_some_and(|r| f[r as usize])).collect::<Vec<_>>());
        State {
            cols: self.cols.clone(),
            data: self.data.iter().map(|(&c, col)| (c, Arc::new(col.pick(order)))).collect(),
            rows: order.len(),
            starred: flag(&self.starred),
            flagged: flag(&self.flagged),
            key: self.key.clone(),
        }
    }

    fn col_at(&self, at: usize) -> Result<usize> {
        (at < self.cols.len()).then_some(at).ok_or_else(|| format!("a change names column {at}, but there are {}", self.cols.len()))
    }

    fn insert(&self, at: usize, col: &Col, cells: &[(usize, Cell)]) -> Result<State> {
        let mut out = self.set_cells(cells.iter().map(|(r, v)| (*r, col.cell, v.clone())).collect::<Vec<_>>())?;
        out.cols.insert(at.min(out.cols.len()), col.clone());
        Ok(out)
    }

    /// The column at `at` leaves the model; its cells are cleared (OpenRefine's column removal).
    fn remove(&self, at: usize) -> Result<State> {
        let mut out = self.clone();
        let c = out.cols.remove(self.col_at(at)?);
        out.data.remove(&c.cell);
        if out.key.as_ref() == Some(&c.name) {
            out.key = None;
        }
        Ok(out)
    }
}

fn patch(mut col: Column, edits: Vec<(usize, Cell)>) -> Column {
    let mut index: HashMap<Value, u32> = col.dict.iter().enumerate().map(|(i, v)| (v.clone(), i as u32)).collect();
    let rows = col.codes.len();
    let mut recons = col.recon.as_ref().map(|rc| rc.cells.clone());
    for (r, (v, recon)) in edits {
        if recon.is_some() || recons.is_some() {
            recons.get_or_insert_with(|| vec![None; rows])[r] = recon;
        }
        let code = match index.get(&v) {
            Some(&c) => c,
            None => {
                col.dict.push(v.clone());
                let c = (col.dict.len() - 1) as u32;
                index.insert(v, c);
                c
            }
        };
        Arc::make_mut(&mut col.codes)[r] = code;
    }
    col.recon = recons.filter(|c| c.iter().any(Option::is_some)).map(|cells| Arc::new(ReconCol::new(cells)));
    col
}

// ---------------------------------------------------------------- change files

/// One history step's change, as OpenRefine records it (com.google.refine.model.changes.*).
enum Change {
    /// CellChange / MassCellChange: (row, cell index, old, new)
    Cells(Vec<(usize, usize, Cell, Cell)>),
    /// ReconChange: cells of one column, and its reconConfig and reconStats before and after
    Recon { cells: Vec<(usize, usize, Cell, Cell)>, column: String, old: (Option<J>, Option<J>), new: (Option<J>, Option<J>) },
    Star(usize, bool, bool),
    Flag(usize, bool, bool),
    Many(Vec<Change>),
    Rename(String, String),
    Move(usize, usize),
    Reorder { old: Vec<Col>, new: Vec<Col>, cells: Vec<(usize, usize, Cell)> },
    RemoveColumn { at: usize, col: Col, cells: Vec<(usize, Cell)> },
    AddColumn { at: usize, col: Col, cells: Vec<(usize, Cell)> },
    Split { at: usize, col: Col, remove: bool, first: usize, names: Vec<String>, rows: Vec<usize>, new: Vec<Row>, old: Vec<Row> },
    Rows { new: Vec<Row>, old: Vec<Row> },
    RowsAndColumns { new_cols: Vec<Col>, old_cols: Vec<Col>, new: Vec<Row>, old: Vec<Row> },
    /// DataExtensionChange: whole rows before and after, the new columns' names, cell indexes and place
    Extend { at: usize, first: usize, names: Vec<String>, new: Vec<Row>, old: Vec<Row> },
    RemoveRows { at: Vec<usize>, rows: Vec<Row> },
    Order(Vec<usize>),
}

struct Lines<'a>(std::str::Lines<'a>, Pool);

impl<'a> Lines<'a> {
    fn next(&mut self) -> Result<&'a str> {
        self.0.next().ok_or_else(|| "a change file ends too early".to_string())
    }
    /// `key=value` lines up to the change's closing `/ec/`.
    fn field(&mut self) -> Result<Option<(&'a str, &'a str)>> {
        let l = self.next()?;
        if l == "/ec/" {
            return Ok(None);
        }
        Ok(Some(l.split_once('=').unwrap_or((l, ""))))
    }
    fn take(&mut self, n: &str) -> Result<Vec<&'a str>> {
        (0..num(n)?).map(|_| self.next()).collect()
    }
    fn rows(&mut self, n: &str) -> Result<Vec<Row>> {
        let lines = self.take(n)?;
        lines.into_iter().map(|l| row(l, &self.1)).collect()
    }
    fn cols(&mut self, n: &str) -> Result<Vec<Col>> {
        self.take(n)?.into_iter().map(|l| serde_json::from_str(l).map_err(err)).collect()
    }
}

fn num(s: &str) -> Result<usize> {
    s.trim().parse().map_err(|_| format!("a change file has `{s}` where a number belongs"))
}

/// `row;cell` lines: (row, cell).
fn row_cells(lines: Vec<&str>, pool: &Pool) -> Result<Vec<(usize, Cell)>> {
    lines.into_iter().map(|l| {
        let (r, c) = l.split_once(';').ok_or("a change file has a malformed cell")?;
        Ok((num(r)?, opt_cell(c, pool)?))
    }).collect()
}

/// One change: its version and class lines, then fields up to `/ec/` (nested changes included).
fn parse(p: &mut Lines) -> Result<Change> {
    p.next()?; // the version that wrote it
    let class = p.next()?;
    let kind = class.rsplit('.').next().unwrap_or(class);
    // fields come back in any order, as OpenRefine's own loaders allow
    let mut f: HashMap<&str, &str> = HashMap::new();
    let (mut cells, mut changes, mut names, mut ints) = (vec![], vec![], vec![], vec![]);
    let (mut cols_a, mut cols_b, mut rows_a, mut rows_b, mut at_cells, mut rc_cells) = (vec![], vec![], vec![], vec![], vec![], vec![]);
    while let Some((k, v)) = p.field()? {
        match (kind, k) {
            (_, "cellChangeCount") => {
                for _ in 0..num(v)? {
                    cells.push(cell_change(p)?);
                }
            }
            ("MassChange", "changeCount") => {
                for _ in 0..num(v)? {
                    changes.push(parse(p)?);
                }
            }
            (_, "columnNameCount") => names = p.take(v)?.into_iter().map(String::from).collect(),
            (_, "rowIndexCount") => ints = p.take(v)?.into_iter().map(num).collect::<Result<_>>()?,
            ("ColumnSplitChange", "tupleCount") => {
                for _ in 0..num(v)? {
                    let n = p.next()?;
                    p.take(n)?;
                }
            }
            (_, "oldColumnCount") => cols_a = p.cols(v)?,
            (_, "newColumnCount") => cols_b = p.cols(v)?,
            (_, "oldRowCount") | ("RowRemovalChange", "rowCount") => rows_a = p.rows(v)?,
            (_, "newRowCount") => rows_b = p.rows(v)?,
            (_, "newCellCount") => {
                let lines = p.take(v)?;
                at_cells = row_cells(lines, &p.1)?
            }
            ("ColumnRemovalChange", "oldCellCount") => {
                let lines = p.take(v)?;
                at_cells = row_cells(lines, &p.1)?
            }
            ("ColumnReorderChange", "oldCellCount") => {
                let lines = p.take(v)?;
                rc_cells = lines.into_iter().map(|l| {
                    let mut it = l.splitn(3, ';');
                    let (r, c, v) = (it.next().unwrap_or(""), it.next().unwrap_or(""), it.next().unwrap_or(""));
                    Ok((num(r)?, num(c)?, opt_cell(v, &p.1)?))
                }).collect::<Result<_>>()?;
            }
            // column groups and anything else counted: skip the lines they announce
            (_, k) if k.ends_with("Count") => {
                p.take(v)?;
            }
            (_, k) => {
                f.insert(k, v);
            }
        }
    }
    let get = |k: &str| f.get(k).copied().ok_or_else(|| format!("{kind}: no {k}"));
    let col = |k: &str| -> Result<Col> { serde_json::from_str(get(k)?).map_err(err) };
    let flag = |k: &str| get(k).map(|v| v == "true");
    Ok(match kind {
        "CellChange" => Change::Cells(vec![cell_body(&f, &p.1)?]),
        "MassCellChange" => Change::Cells(cells),
        "ReconChange" => {
            let config = |k: &str| f.get(k).filter(|v| !v.is_empty()).and_then(|v| serde_json::from_str::<J>(v).ok()).filter(|v| !v.is_null());
            Change::Recon {
                cells,
                column: get("commonColumnName")?.into(),
                old: (config("oldReconConfig"), config("oldReconStats")),
                new: (config("newReconConfig"), config("newReconStats")),
            }
        }
        "MassChange" => Change::Many(changes),
        "RowStarChange" => Change::Star(num(get("row")?)?, flag("oldStarred")?, flag("newStarred")?),
        "RowFlagChange" => Change::Flag(num(get("row")?)?, flag("oldFlagged")?, flag("newFlagged")?),
        "ColumnRenameChange" => Change::Rename(get("oldColumnName")?.into(), get("newColumnName")?.into()),
        "ColumnMoveChange" => Change::Move(num(get("oldColumnIndex")?)?, num(get("newColumnIndex")?)?),
        "ColumnReorderChange" => Change::Reorder { old: cols_a, new: cols_b, cells: rc_cells },
        "ColumnRemovalChange" => Change::RemoveColumn { at: num(get("oldColumnIndex")?)?, col: col("oldColumn")?, cells: at_cells },
        "ColumnAdditionChange" => Change::AddColumn {
            at: num(get("columnIndex")?)?,
            col: Col { cell: num(get("newCellIndex")?)?, name: get("columnName")?.into(), recon: None, stats: None },
            cells: at_cells,
        },
        "ColumnSplitChange" => Change::Split {
            at: num(get("columnIndex")?)?,
            col: col("column")?,
            remove: flag("removeOriginalColumn")?,
            first: num(get("firstNewCellIndex")?)?,
            names,
            rows: ints,
            new: rows_b,
            old: rows_a,
        },
        "MassRowChange" => Change::Rows { new: rows_b, old: rows_a },
        "MassRowColumnChange" => Change::RowsAndColumns { new_cols: cols_b, old_cols: cols_a, new: rows_b, old: rows_a },
        "DataExtensionChange" => Change::Extend {
            at: num(get("columnInsertIndex")?)?,
            first: num(get("firstNewCellIndex")?)?,
            names,
            new: rows_b,
            old: rows_a,
        },
        "RowRemovalChange" => Change::RemoveRows { at: ints, rows: rows_a },
        "RowReorderChange" => Change::Order(ints),
        _ => return Err(format!("OxRefine can't read OpenRefine's {kind} yet")),
    })
}

fn cell_body(f: &HashMap<&str, &str>, pool: &Pool) -> Result<(usize, usize, Cell, Cell)> {
    let get = |k: &str| f.get(k).copied().ok_or_else(|| format!("CellChange: no {k}"));
    Ok((num(get("row")?)?, num(get("cell")?)?, opt_cell(get("old")?, pool)?, opt_cell(get("new")?, pool)?))
}

/// A CellChange inside a MassCellChange: its fields, no class line.
fn cell_change(p: &mut Lines) -> Result<(usize, usize, Cell, Cell)> {
    let mut f = HashMap::new();
    while let Some((k, v)) = p.field()? {
        f.insert(k, v);
    }
    cell_body(&f, &p.1)
}

impl Change {
    /// The state after this change (`forward`) or before it (undo).
    fn run(&self, s: &State, forward: bool) -> Result<State> {
        let pick = |old: bool, new: bool| if forward { new } else { old };
        Ok(match self {
            Change::Cells(v) => s.set_cells(v.iter().map(|(r, c, o, n)| (*r, *c, if forward { n } else { o }.clone())).collect::<Vec<_>>())?,
            Change::Recon { cells, column, old, new } => {
                let mut out = s.set_cells(cells.iter().map(|(r, c, o, n)| (*r, *c, if forward { n } else { o }.clone())).collect::<Vec<_>>())?;
                let (config, mut stats) = if forward { new } else { old }.clone();
                // a ReconChange without stats counts them (ReconStats.create)
                if stats.is_none() && forward {
                    let at = out.cols.iter().find(|c| &c.name == column).map(|c| c.cell);
                    if let Some(col) = at.and_then(|i| out.data.get(&i)) {
                        stats = Some(col.recon.as_ref().map_or_else(|| ReconCol::new(vec![None; out.rows]).count(col), |rc| rc.count(col)));
                    }
                }
                if let Some(c) = out.cols.iter_mut().find(|c| &c.name == column) {
                    (c.recon, c.stats) = (config, stats);
                }
                out
            }
            Change::Star(r, o, n) | Change::Flag(r, o, n) => {
                if *r >= s.rows {
                    return Err(format!("a change names row {r}, but there are {} rows", s.rows));
                }
                let mut out = s.clone();
                let target = if matches!(self, Change::Star(..)) { &mut out.starred } else { &mut out.flagged };
                let mut v = (**target).clone();
                v[*r] = pick(*o, *n);
                *target = Arc::new(v);
                out
            }
            Change::Many(v) => {
                let mut out = s.clone();
                if forward {
                    for c in v {
                        out = c.run(&out, true)?;
                    }
                } else {
                    for c in v.iter().rev() {
                        out = c.run(&out, false)?;
                    }
                }
                out
            }
            Change::Rename(old, new) => {
                let (from, to) = if forward { (old, new) } else { (new, old) };
                let mut out = s.clone();
                let c = out.cols.iter_mut().find(|c| &c.name == from).ok_or_else(|| format!("no column {from} to rename"))?;
                c.name = to.clone();
                if out.key.as_ref() == Some(from) {
                    out.key = Some(to.clone());
                }
                out
            }
            Change::Move(old, new) => {
                let (from, to) = if forward { (*old, *new) } else { (*new, *old) };
                let mut out = s.clone();
                let c = out.cols.remove(s.col_at(from)?);
                out.cols.insert(to.min(out.cols.len()), c);
                out
            }
            Change::Reorder { old, new, cells } => {
                let mut out = s.clone();
                if forward {
                    // columns left out of the new order lose their cells
                    for c in old.iter().filter(|c| !new.iter().any(|n| n.cell == c.cell)) {
                        out.data.remove(&c.cell);
                    }
                    out.cols = new.clone();
                } else {
                    out = out.set_cells(cells.clone())?;
                    out.cols = old.clone();
                }
                out
            }
            Change::RemoveColumn { at, col, cells } => {
                if forward {
                    s.remove(*at)?
                } else {
                    s.insert(*at, col, cells)?
                }
            }
            Change::AddColumn { at, col, cells } => {
                if forward {
                    s.insert(*at, col, cells)?
                } else {
                    let i = s.cols.iter().position(|c| c.cell == col.cell).ok_or_else(|| format!("no column {} to take away", col.name))?;
                    s.remove(i)?
                }
            }
            Change::Split { at, col, remove, first, names, rows, new, old } => {
                let new_cells = *first..*first + names.len();
                if forward {
                    let mut out = s.put_rows(&rows.iter().copied().zip(new.iter()).collect::<Vec<_>>())?;
                    for (i, n) in names.iter().enumerate() {
                        out.cols.insert((at + 1 + i).min(out.cols.len()), Col { cell: first + i, name: n.clone(), recon: None, stats: None });
                    }
                    if *remove {
                        out.cols.remove(out.col_at(*at)?);
                    }
                    out
                } else {
                    let mut out = s.clone();
                    out.cols.retain(|c| !new_cells.contains(&c.cell));
                    out.data.retain(|c, _| !new_cells.contains(c));
                    out = out.put_rows(&rows.iter().copied().zip(old.iter()).collect::<Vec<_>>())?;
                    if *remove {
                        out.cols.insert((*at).min(out.cols.len()), col.clone());
                    }
                    out
                }
            }
            Change::Rows { new, old } => from_rows(s.cols.clone(), if forward { new } else { old }, s.key.clone()),
            Change::Extend { at, first, names, new, old, .. } => {
                let mut cols = s.cols.clone();
                if forward {
                    for (i, n) in names.iter().enumerate() {
                        cols.insert((at + i).min(cols.len()), Col { cell: first + i, name: n.clone(), recon: None, stats: None });
                    }
                } else {
                    cols.retain(|c| !(*first..first + names.len()).contains(&c.cell));
                }
                from_rows(cols, if forward { new } else { old }, s.key.clone())
            }
            Change::RowsAndColumns { new_cols, old_cols, new, old } => {
                let cols = if forward { new_cols } else { old_cols };
                let key = s.key.clone().filter(|k| cols.iter().any(|c| &c.name == k));
                from_rows(cols.clone(), if forward { new } else { old }, key)
            }
            Change::RemoveRows { at, rows } => {
                if forward {
                    let gone: std::collections::HashSet<usize> = at.iter().copied().collect();
                    s.pick(&(0..s.rows).filter(|r| !gone.contains(r)).map(|r| Some(r as u32)).collect::<Vec<_>>())
                } else {
                    // OpenRefine re-inserts the rows at their indexes, lowest first
                    let mut order: Vec<Option<u32>> = (0..s.rows as u32).map(Some).collect();
                    for &i in at {
                        order.insert(i.min(order.len()), None);
                    }
                    s.pick(&order).put_rows(&at.iter().copied().zip(rows.iter()).collect::<Vec<_>>())?
                }
            }
            Change::Order(ix) => {
                if forward {
                    s.pick(&ix.iter().map(|&r| Some(r as u32)).collect::<Vec<_>>())
                } else {
                    let mut back = vec![None; ix.len()];
                    for (new, &old) in ix.iter().enumerate() {
                        *back.get_mut(old).ok_or("a row order names a row that isn't there")? = Some(new as u32);
                    }
                    s.pick(&back)
                }
            }
        })
    }

    /// The single edit an entry without an operation stands for (OpenRefine's edit-one-cell and
    /// annotate-one-row), as OxRefine's own operation.
    fn single_edit(&self, s: &State) -> Option<OpKind> {
        match self {
            Change::Cells(v) if v.len() == 1 => {
                let (row, c, _, (new, _)) = &v[0];
                let column_name = s.cols.iter().find(|x| x.cell == *c)?.name.clone();
                let (value, kind) = match new {
                    Value::Str(t) => (t.clone(), "text"),
                    Value::Int(_) | Value::Num(_) => (new.to_java_string(), "number"),
                    Value::Bool(b) => (b.to_string(), "boolean"),
                    Value::Date(d) => (fmt_date(d), "date"),
                    // an empty cell: the editor stores one for a date it can't read
                    Value::Null => (String::new(), "date"),
                    _ => return None,
                };
                Some(OpKind::CellEdit { row: *row, column_name, value, kind: kind.into() })
            }
            Change::Star(row, _, n) => Some(OpKind::RowAnnotate { row: *row, starred: Some(*n), flagged: None }),
            Change::Flag(row, _, n) => Some(OpKind::RowAnnotate { row: *row, starred: None, flagged: Some(*n) }),
            Change::Many(v) if v.len() == 1 => v[0].single_edit(s),
            // recon-judge-one-cell and recon-clear-one-cell
            Change::Recon { cells, .. } if cells.len() == 1 => {
                let (row, c, _, (_, new)) = &cells[0];
                let column_name = s.cols.iter().find(|x| x.cell == *c)?.name.clone();
                Some(match new {
                    None => OpKind::ReconClearOneCell { row: *row, column_name },
                    Some(r) => OpKind::ReconJudgeOneCell {
                        row: *row,
                        column_name,
                        judgment: match r.judgment {
                            crate::recon::Judgment::Matched => "matched",
                            crate::recon::Judgment::New => "new",
                            _ => "none",
                        }
                        .into(),
                        r#match: r.matched.as_ref().map(|m| json!({ "id": m.id, "name": m.name, "types": m.types, "score": m.score })),
                        identifier_space: r.identifier_space.clone(),
                        schema_space: r.schema_space.clone(),
                    },
                })
            }
            _ => None,
        }
    }
}

// ---------------------------------------------------------------- reading

#[derive(Deserialize)]
struct Entry {
    id: i64,
    #[serde(default)]
    description: String,
    #[serde(default)]
    operation: Option<J>,
}

/// An OpenRefine project as read from its archive.
pub struct Archive {
    pub name: String,
    /// history descriptions, past then future
    pub history: Vec<String>,
    /// how many history steps are done
    pub cursor: usize,
    /// the first step whose table is known: undo stopped there (a change OxRefine can't read)
    pub first: usize,
    /// why undo or redo stopped early
    pub stopped: Option<String>,
    /// whether cells carried reconciliation data
    pub recon: bool,
    entries: Vec<Entry>,
    changes: Vec<Option<Change>>,
    states: Vec<State>,
}

/// What happened to OpenRefine's history on the way in.
#[derive(Default, serde::Serialize)]
pub struct Report {
    /// steps merged into the starting table (they can't be undone in OxRefine)
    pub merged: usize,
    /// undone steps that were dropped
    pub dropped: usize,
    pub reason: Option<String>,
}

fn files(path: &Path) -> Result<HashMap<String, Vec<u8>>> {
    let f = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(BufReader::new(f)));
    let mut out = HashMap::new();
    for e in tar.entries().map_err(|e| format!("not an OpenRefine project archive: {e}"))? {
        let mut e = e.map_err(|e| format!("damaged project archive: {e}"))?;
        if !e.header().entry_type().is_file() {
            continue;
        }
        let name = e.path().map_err(err)?.to_string_lossy().replace('\\', "/");
        let mut bytes = vec![];
        e.read_to_end(&mut bytes).map_err(|e| format!("damaged project archive: {e}"))?;
        out.insert(name.trim_start_matches("./").to_string(), bytes);
    }
    Ok(out)
}

fn unzip(bytes: &[u8], name: &str) -> Result<String> {
    let mut z = zip::ZipArchive::new(Cursor::new(bytes)).map_err(err)?;
    let mut s = String::new();
    z.by_name(name).map_err(err)?.read_to_string(&mut s).map_err(err)?;
    Ok(s)
}

/// Read an archive and work out the table at every history step it can.
pub fn read(path: &Path) -> Result<Archive> {
    crate::jobs::progress("Reading the OpenRefine project", 0);
    let mut files = files(path)?;
    let meta: J = files.get("metadata.json").map(|b| serde_json::from_slice(b).unwrap_or(J::Null)).unwrap_or(J::Null);
    let data = files.remove("data.zip").ok_or("not an OpenRefine project archive (no data.zip)")?;
    let mut z = zip::ZipArchive::new(Cursor::new(&data)).map_err(err)?;
    let pool = read_pool(&unzip(&data, "pool.txt").unwrap_or_default());
    let recon = !pool.is_empty();

    // data.txt: version, column model, history, overlay models, then the rows
    let mut lines = BufReader::new(z.by_name("data.txt").map_err(err)?).lines();
    let mut next = || -> Result<String> { lines.next().ok_or("data.txt ends too early")?.map_err(err) };
    let (mut cols, mut key_at, mut entries, mut past, mut b) = (vec![], 0, vec![], 0, Builder::default());
    next()?;
    loop {
        let l = next()?;
        let (k, v) = l.split_once('=').unwrap_or((&l, ""));
        match k {
            "keyColumnIndex" => key_at = num(v)?,
            "columnCount" => {
                for _ in 0..num(v)? {
                    cols.push(serde_json::from_str::<Col>(&next()?).map_err(err)?);
                }
            }
            "columnGroupCount" => {
                for _ in 0..num(v)? {
                    next()?;
                }
            }
            "pastEntryCount" | "futureEntryCount" => {
                if k == "pastEntryCount" {
                    past = num(v)?;
                }
                for _ in 0..num(v)? {
                    entries.push(serde_json::from_str::<Entry>(&next()?).map_err(err)?);
                }
            }
            "rowCount" => {
                let n = num(v)?;
                crate::jobs::progress("Reading the OpenRefine project", n as u64);
                for _ in 0..n {
                    b.push(row(&next()?, &pool)?);
                    crate::jobs::tick(1)?;
                }
                break;
            }
            _ => {}
        }
    }
    let key = cols.get(key_at).map(|c| c.name.clone());
    let now = b.state(cols, key);

    let changes: Vec<Option<Change>> = entries
        .iter()
        .map(|e| {
            let f = files.get(&format!("history/{}.change.zip", e.id))?;
            let text = unzip(f, "change.txt").ok()?;
            Some(parse(&mut Lines(text.lines(), read_pool(&unzip(f, "pool.txt").unwrap_or_default()))))
        })
        .map(|c| c.and_then(|c| c.ok()))
        .collect();
    let why = |i: usize, e: &Entry| -> String {
        let reason = match files.get(&format!("history/{}.change.zip", e.id)) {
            None => "its change file is missing".to_string(),
            Some(f) => match unzip(f, "change.txt").map(|t| parse(&mut Lines(t.lines(), read_pool(&unzip(f, "pool.txt").unwrap_or_default())))) {
                Ok(Err(m)) | Err(m) => m,
                Ok(Ok(_)) => "it doesn't fit the table".into(),
            },
        };
        format!("step {} ({}): {reason}", i + 1, e.description)
    };

    // undo back to the start, then redo the undone steps
    let mut stopped = None;
    let mut back = vec![now];
    for i in (0..past).rev() {
        match changes[i].as_ref().map(|c| c.run(back.last().unwrap(), false)) {
            Some(Ok(s)) => back.push(s),
            _ => {
                stopped = Some(why(i, &entries[i]));
                break;
            }
        }
    }
    let first = past + 1 - back.len();
    back.reverse();
    let mut states = back;
    for i in past..entries.len() {
        match changes[i].as_ref().map(|c| c.run(states.last().unwrap(), true)) {
            Some(Ok(s)) => states.push(s),
            _ => {
                stopped = stopped.or_else(|| Some(why(i, &entries[i])));
                break;
            }
        }
    }
    Ok(Archive {
        name: meta["name"].as_str().filter(|n| !n.is_empty()).map_or_else(|| file_stem(path), String::from),
        history: entries.iter().map(|e| e.description.clone()).collect(),
        cursor: past,
        first,
        stopped,
        recon,
        entries,
        changes,
        states,
    })
}

fn file_stem(path: &Path) -> String {
    let n = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    n.trim_end_matches(".tar.gz").trim_end_matches(".tgz").trim_end_matches(".openrefine").to_string()
}

/// Two tables alike for the purpose of trusting a replayed step: same shape, flags and cells, with
/// any two errors alike (messages are worded differently) and numbers compared by value.
fn same(a: &Table, b: &Table) -> bool {
    let cell = |x: &Value, y: &Value| match (x, y) {
        (Value::Error(_), Value::Error(_)) => true,
        (Value::Int(i), Value::Num(n)) | (Value::Num(n), Value::Int(i)) => *i as f64 == *n,
        _ => x == y,
    };
    // Recons alike: same judgment and match (ids are random)
    let recon = |x: Option<&Arc<Recon>>, y: Option<&Arc<Recon>>| match (x, y) {
        (None, None) => true,
        (Some(x), Some(y)) => x.judgment == y.judgment && x.matched.as_ref().map(|m| &m.id) == y.matched.as_ref().map(|m| &m.id),
        _ => false,
    };
    a.names == b.names
        && a.rows == b.rows
        && a.starred == b.starred
        && a.flagged == b.flagged
        && a.columns.iter().zip(&b.columns).all(|(x, y)| Arc::ptr_eq(x, y) || (0..a.rows).all(|r| cell(x.get(r), y.get(r)) && recon(x.recon(r), y.recon(r))))
}

impl Archive {
    /// The table after `step` history steps, as OpenRefine had it (None before `first`).
    pub fn table(&self, step: usize) -> Option<Table> {
        self.states.get(step.checked_sub(self.first)?).map(State::table)
    }

    /// The last step whose table is known.
    pub fn last(&self) -> usize {
        self.first + self.states.len() - 1
    }

    /// An OxRefine project: OpenRefine's steps as operations, from the first one OxRefine can replay
    /// to the same result; anything before a step it can't is merged into the starting table.
    pub fn into_project(self) -> (Project, Report) {
        let mut report = Report { merged: self.first, reason: self.stopped.clone(), ..Report::default() };
        let mut tables = vec![self.states[0].table()];
        let mut ops: Vec<Op> = vec![];
        for (i, state) in self.states.iter().enumerate().skip(1) {
            let k = self.first + i - 1; // the step from states[i-1] to states[i]
            let e = &self.entries[k];
            let kind = match &e.operation {
                Some(o) if !o.is_null() => serde_json::from_value::<Op>(o.clone()).map(|o| o.kind).map_err(|e| match o["op"].as_str() {
                    Some(name) if e.to_string().starts_with("unknown variant") => format!("OxRefine doesn't have {name} yet"),
                    _ => format!("OxRefine can't read this operation ({e})"),
                }),
                _ => self.changes[k].as_ref().and_then(|c| c.single_edit(&self.states[i - 1])).ok_or_else(|| "OxRefine can't redo this kind of edit".to_string()),
            };
            let target = state.table();
            // OpenRefine keeps what it fetched and the Recons the service gave; the step replays
            // from those instead of asking again
            let kind = kind.map(|mut k| {
                if let OpKind::ColumnAdditionByFetchingUrls { new_column_name, fetched, .. } = &mut k {
                    if let Ok(c) = target.col(new_column_name) {
                        *fetched = Some((0..target.rows).map(|r| fetched_cell(target.columns[c].get(r))).collect());
                    }
                }
                if let OpKind::Recon { column_name, results, .. } = &mut k {
                    if let Ok(c) = target.col(column_name) {
                        let cells: Vec<_> = (0..target.rows).map(|r| target.columns[c].recon(r).cloned()).collect();
                        *results = Some(crate::recon::save_cells(&cells));
                    }
                }
                if let OpKind::ExtendReconciledData { base_column_name, extension, column_insert_index, results, .. } = &mut k {
                    // the rows with a matched item, and the columns the step added
                    let prev = self.states[i - 1].table();
                    let n = extension.get("properties").and_then(J::as_array).map_or(0, Vec::len);
                    let at = *column_insert_index;
                    let names: Vec<String> = target.names.iter().skip(at).take(n).cloned().collect();
                    let rows: Vec<usize> = prev.col(base_column_name).map_or_else(
                        |_| vec![],
                        |c| (0..prev.rows).filter(|&r| prev.columns[c].recon(r).is_some_and(|rc| rc.matched.is_some())).collect(),
                    );
                    *results = Some(extension_from(&prev, &target, base_column_name, at, &names, &rows, extension));
                }
                k
            });
            let replayed = kind.and_then(|kind| {
                let mut op = Op { kind, description: e.description.clone() };
                let t = op.apply(tables.last().unwrap())?;
                if same(&t, &target) { Ok((op, t)) } else { Err("OxRefine's result differs from OpenRefine's".into()) }
            });
            match replayed {
                Ok((op, t)) => {
                    ops.push(op);
                    tables.push(t);
                }
                Err(m) if k < self.cursor => {
                    report.merged = k + 1;
                    report.reason = Some(format!("step {} ({}): {m}", k + 1, e.description));
                    ops.clear();
                    tables = vec![target];
                }
                Err(m) => {
                    report.reason = report.reason.or(Some(format!("step {} ({}): {m}", k + 1, e.description)));
                    break;
                }
            }
        }
        report.dropped = self.entries.len() - report.merged - ops.len();
        let cursor = self.cursor - report.merged;
        let n = tables.len();
        let mut p = Project::from_steps(self.name, tables, ops);
        // the history ends early when a redo couldn't be read; the cursor stays inside it
        p.goto(cursor.min(n - 1)).expect("cursor within the steps");
        (p, report)
    }
}

/// What the service answered for a data extension, read back from the table it made, so the step
/// replays without asking: the extended rows' data rows are the target's rows from that row on, as
/// long as they hold any of the new columns' values.
fn extension_from(prev: &Table, target: &Table, base: &str, at: usize, names: &[String], rows: &[usize], extension: &J) -> J {
    let props = extension.get("properties").and_then(J::as_array).cloned().unwrap_or_default();
    let columns: Vec<J> = names
        .iter()
        .enumerate()
        .map(|(i, n)| json!({ "id": props.get(i).and_then(|p| p.get("id")).cloned().unwrap_or(json!(n)), "name": n }))
        .collect();
    let (Ok(pb), Ok(tb)) = (prev.col(base), target.col(base)) else { return json!({ "columns": columns, "rows": [] }) };
    let (pk, tk) = (prev.key_index(), target.key_index());
    let new_cols: Vec<usize> = (0..names.len()).map(|i| at + i).filter(|&c| c < target.names.len()).collect();
    let value = |c: usize, r: usize| -> J {
        let col = &target.columns[c];
        if let Some(m) = col.recon(r).and_then(|rc| rc.matched.as_ref()) {
            return json!({ "id": m.id, "name": m.name, "type": m.types });
        }
        match col.get(r) {
            Value::Null => J::Null,
            Value::Str(s) => json!({ "str": s }),
            Value::Int(i) => json!({ "int": i }),
            Value::Num(n) => json!({ "float": n }),
            Value::Bool(b) => json!({ "bool": b }),
            Value::Date(d) => json!({ "date": format!("{}+00:00", d.format("%Y-%m-%dT%H:%M:%S")) }),
            other => json!({ "str": other.to_java_string() }),
        }
    };
    let filled = |r: usize| new_cols.iter().any(|&c| !target.columns[c].get(r).is_null());
    let row = |r: usize| new_cols.iter().map(|&c| value(c, r)).collect::<Vec<_>>();
    let blank = |t: &Table, c: usize, r: usize| t.columns[c].get(r).is_blank();
    let mut out = vec![];
    let (mut p, mut q) = (0usize, 0usize);
    while p < prev.rows && q < target.rows {
        if rows.contains(&p) && filled(q) {
            // the extended row, then the rows its other values went to (blank base and key cells)
            let mut data = vec![row(q)];
            let mut k = q + 1;
            while k < target.rows && filled(k) && blank(target, tb, k) && blank(target, tk, k) {
                data.push(row(k));
                k += 1;
            }
            // the old rows those values took, by OpenRefine's rule; the rest were new rows
            let mut next = p + 1;
            for _ in 1..data.len() {
                if next < prev.rows && blank(prev, pb, next) && blank(prev, pk, next) {
                    next += 1;
                }
            }
            out.push(json!([p, data]));
            p = next;
            q = k;
        } else {
            p += 1;
            q += 1;
        }
    }
    json!({ "columns": columns, "rows": out })
}

/// A fetched cell as an operation keeps it: null | text | {"e": message}.
fn fetched_cell(v: &Value) -> J {
    match v {
        Value::Null => J::Null,
        Value::Error(e) => json!({ "e": e }),
        other => J::String(other.to_java_string()),
    }
}

// ---------------------------------------------------------------- writing

fn col_json(i: usize, name: &str, col: &Column) -> String {
    let mut j = json!({ "cellIndex": i, "originalName": name, "name": name });
    if let Some(rc) = &col.recon {
        if let Some(c) = &rc.config {
            j["reconConfig"] = c.clone();
        }
        if let Some(s) = &rc.stats {
            j["reconStats"] = s.clone();
        }
    }
    j.to_string()
}

/// The Recons a data or change file names, written once each to its pool.txt.
#[derive(Default)]
struct PoolOut {
    seen: std::collections::HashSet<i64>,
    lines: String,
}

impl PoolOut {
    fn cell(&mut self, v: &Value, recon: Option<&Arc<Recon>>) -> J {
        let mut j = cell_json(v);
        if let Some(r) = recon {
            if j.is_null() {
                j = json!({});
            }
            j["r"] = J::String(r.id.to_string());
            if self.seen.insert(r.id) {
                self.lines += &(r.to_json(true).to_string() + "\n");
            }
        }
        j
    }
    fn opt(&mut self, v: &Value, recon: Option<&Arc<Recon>>) -> String {
        match self.cell(v, recon) {
            J::Null => String::new(),
            j => j.to_string(),
        }
    }
    fn text(&self) -> String {
        format!("{VERSION}\nreconCount={}\n{}", self.seen.len(), self.lines)
    }
}

fn row_json(t: &Table, r: usize, pool: &mut PoolOut) -> String {
    let cells: Vec<J> = t.columns.iter().map(|c| pool.cell(c.get(r), c.recon(r))).collect();
    json!({ "starred": t.starred[r], "flagged": t.flagged[r], "cells": cells }).to_string()
}

fn same_recon(x: Option<&Arc<Recon>>, y: Option<&Arc<Recon>>) -> bool {
    match (x, y) {
        (None, None) => true,
        (Some(x), Some(y)) => Arc::ptr_eq(x, y),
        _ => false,
    }
}

fn zip_files(files: &[(&str, &[u8])]) -> Result<Vec<u8>> {
    let mut z = zip::ZipWriter::new(Cursor::new(vec![]));
    for (name, bytes) in files {
        z.start_file(*name, zip::write::SimpleFileOptions::default()).map_err(err)?;
        z.write_all(bytes).map_err(err)?;
    }
    Ok(z.finish().map_err(err)?.into_inner())
}

/// The change file from table `a` to `b`, made by `op`. Single edits are written the way OpenRefine
/// writes them (even when nothing changed); other steps of the same shape as their changed cells and
/// flags; anything else as both whole tables (OpenRefine's MassRowColumnChange, which undoes anything).
// ponytail: structural steps store both tables in full, like OpenRefine's own MassRowChange; store
// only the moved or removed rows if exported archives of big projects get too large.
/// The change file and its pool.txt.
fn change_file(a: &Table, b: &Table, op: &Op) -> (String, String) {
    let mut pool = PoolOut::default();
    let w = change_text(a, b, op, &mut pool);
    (w, pool.text())
}

fn change_text(a: &Table, b: &Table, op: &Op, pool: &mut PoolOut) -> String {
    let mut w = format!("{VERSION}\n");
    let diffs = |pool: &mut PoolOut, i: usize, x: &Column, y: &Column| -> Vec<String> {
        (0..a.rows)
            .filter(|&r| x.get(r) != y.get(r) || !same_recon(x.recon(r), y.recon(r)))
            .map(|r| format!("row={r}\ncell={i}\nold={}\nnew={}\n/ec/\n", pool.opt(x.get(r), x.recon(r)), pool.opt(y.get(r), y.recon(r))))
            .collect()
    };
    match &op.kind {
        OpKind::CellEdit { row, column_name, .. } if a.names == b.names => {
            let i = a.names.iter().position(|n| n == column_name).unwrap_or(0);
            let (x, y) = (&a.columns[i], &b.columns[i]);
            let (old, new) = (pool.opt(x.get(*row), x.recon(*row)), pool.opt(y.get(*row), y.recon(*row)));
            return w + &format!("com.google.refine.model.changes.CellChange\nrow={row}\ncell={i}\nold={old}\nnew={new}\n/ec/\n");
        }
        OpKind::RowAnnotate { row: r, starred: Some(_), .. } => {
            return w + &format!("com.google.refine.model.changes.RowStarChange\nrow={r}\nnewStarred={}\noldStarred={}\n/ec/\n", b.starred[*r], a.starred[*r]);
        }
        OpKind::RowAnnotate { row: r, .. } => {
            return w + &format!("com.google.refine.model.changes.RowFlagChange\nrow={r}\nnewFlagged={}\noldFlagged={}\n/ec/\n", b.flagged[*r], a.flagged[*r]);
        }
        _ if op.recon_column().is_some() && a.names == b.names && a.rows == b.rows => {
            let column_name = op.recon_column().unwrap_or_default();
            let i = a.names.iter().position(|n| n == column_name).unwrap_or(0);
            let (x, y) = (&a.columns[i], &b.columns[i]);
            let field = |j: &Option<J>| j.as_ref().map(J::to_string).unwrap_or_default();
            let config = |c: &Column| c.recon.as_ref().map(|rc| (field(&rc.config), field(&rc.stats))).unwrap_or_default();
            let ((old_config, old_stats), (new_config, new_stats)) = (config(x), config(y));
            let d = diffs(pool, i, x, y);
            return w + &format!(
                "com.google.refine.model.changes.ReconChange\nnewReconConfig={new_config}\nnewReconStats={new_stats}\noldReconConfig={old_config}\noldReconStats={old_stats}\ncommonColumnName={column_name}\nupdateRowContextDependencies=false\ncellChangeCount={}\n{}/ec/\n",
                d.len(),
                d.concat()
            );
        }
        _ => {}
    }
    if a.names == b.names && a.rows == b.rows {
        let mut parts = vec![];
        for (i, (x, y)) in a.columns.iter().zip(&b.columns).enumerate() {
            if Arc::ptr_eq(x, y) {
                continue;
            }
            let diffs = diffs(pool, i, x, y);
            if !diffs.is_empty() {
                parts.push(format!(
                    "{VERSION}\ncom.google.refine.model.changes.MassCellChange\ncommonColumnName={}\nupdateRowContextDependencies=true\ncellChangeCount={}\n{}/ec/\n",
                    a.names[i],
                    diffs.len(),
                    diffs.concat()
                ));
            }
        }
        for r in 0..a.rows {
            if a.starred[r] != b.starred[r] {
                parts.push(format!("{VERSION}\ncom.google.refine.model.changes.RowStarChange\nrow={r}\nnewStarred={}\noldStarred={}\n/ec/\n", b.starred[r], a.starred[r]));
            }
            if a.flagged[r] != b.flagged[r] {
                parts.push(format!("{VERSION}\ncom.google.refine.model.changes.RowFlagChange\nrow={r}\nnewFlagged={}\noldFlagged={}\n/ec/\n", b.flagged[r], a.flagged[r]));
            }
        }
        w += &format!("com.google.refine.model.changes.MassChange\nupdateRowContextDependencies=true\nchangeCount={}\n{}/ec/\n", parts.len(), parts.concat());
    } else {
        w += "com.google.refine.model.changes.MassRowColumnChange\n";
        for (label, t) in [("new", b), ("old", a)] {
            w += &format!("{label}ColumnCount={}\n", t.names.len());
            t.names.iter().enumerate().for_each(|(i, n)| w += &(col_json(i, n, &t.columns[i]) + "\n"));
        }
        for (label, t) in [("new", b), ("old", a)] {
            w += &format!("{label}RowCount={}\n", t.rows);
            (0..t.rows).for_each(|r| w += &(row_json(t, r, pool) + "\n"));
        }
        w += "oldColumnGroupCount=0\n/ec/\n";
    }
    w
}

/// Write `p` as an archive OpenRefine 3.10 opens, history and undone steps included.
pub fn write(p: &Project, path: &Path) -> Result<()> {
    let (tables, ops, cursor) = p.history_parts();
    crate::jobs::progress("Writing the OpenRefine project", ops.len() as u64 + 1);
    let now = chrono::Utc::now();
    let stamp = now.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string();
    let id0 = now.timestamp_millis();
    let entry = |k: usize, op: &Op| {
        let operation = if op.in_recipe() { op.recipe_json() } else { J::Null };
        json!({ "id": id0 + k as i64, "description": op.description(), "operation": operation, "time": stamp }).to_string()
    };

    let t = &tables[cursor];
    let mut d = format!("{VERSION}\ncolumnModel=\nmaxCellIndex={}\nkeyColumnIndex={}\ncolumnCount={}\n", t.names.len() as i64 - 1, t.key_index().min(t.names.len().saturating_sub(1)), t.names.len());
    t.names.iter().enumerate().for_each(|(i, n)| d += &(col_json(i, n, &t.columns[i]) + "\n"));
    d += &format!("columnGroupCount=0\n/e/\nhistory=\npastEntryCount={cursor}\n");
    ops[..cursor].iter().enumerate().for_each(|(k, o)| d += &(entry(k, o) + "\n"));
    d += &format!("futureEntryCount={}\n", ops.len() - cursor);
    ops[cursor..].iter().enumerate().for_each(|(k, o)| d += &(entry(cursor + k, o) + "\n"));
    d += &format!("/e/\nrowCount={}\n", t.rows);
    let mut data_pool = PoolOut::default();
    for r in 0..t.rows {
        d += &row_json(t, r, &mut data_pool);
        d.push('\n');
    }
    let created = stamp.clone();
    let meta = json!({
        "created": created, "modified": stamp, "name": p.name, "password": "", "encoding": "UTF-8", "encodingConfidence": 0,
        "tags": [], "creator": "", "contributors": "", "subject": "", "description": "", "rowCount": t.rows, "title": "",
        "version": "", "license": "", "homepage": "", "image": "", "importOptionMetadata": [], "customMetadata": {},
    });

    let file = std::fs::File::create(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut tar = tar::Builder::new(flate2::write::GzEncoder::new(std::io::BufWriter::new(file), flate2::Compression::default()));
    let mut add = |name: &str, bytes: &[u8]| -> Result<()> {
        let mut h = tar::Header::new_gnu();
        h.set_size(bytes.len() as u64);
        h.set_mode(0o644);
        h.set_mtime(now.timestamp().max(0) as u64);
        h.set_cksum();
        tar.append_data(&mut h, name, bytes).map_err(err)
    };
    add("metadata.json", meta.to_string().as_bytes())?;
    add("data.zip", &zip_files(&[("data.txt", d.as_bytes()), ("pool.txt", data_pool.text().as_bytes())])?)?;
    crate::jobs::tick(1)?;
    for k in 0..ops.len() {
        let (c, pool) = change_file(&tables[k], &tables[k + 1], &ops[k]);
        add(&format!("history/{}.change.zip", id0 + k as i64), &zip_files(&[("change.txt", c.as_bytes()), ("pool.txt", pool.as_bytes())])?)?;
        crate::jobs::tick(1)?;
    }
    tar.into_inner().map_err(err)?.finish().map_err(err)?.flush().map_err(err)
}
