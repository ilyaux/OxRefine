use crate::{grel, grel::Value, Result};
use rustc_hash::FxHashMap as HashMap;
use std::{path::Path, sync::Arc};

const NONE: u32 = u32::MAX;

/// Assigns dictionary codes. Strings have their own map so CSV import can look up a `&str`
/// without allocating; every other value type goes through the general map.
#[derive(Default)]
pub struct Interner {
    strs: HashMap<Box<str>, u32>,
    other: HashMap<Value, u32>,
}

impl Interner {
    fn next(&self) -> u32 {
        (self.strs.len() + self.other.len()) as u32
    }
    pub fn intern_str(&mut self, s: &str) -> u32 {
        if let Some(&i) = self.strs.get(s) {
            return i;
        }
        let i = self.next();
        self.strs.insert(s.into(), i);
        i
    }
    pub fn intern(&mut self, v: Value) -> u32 {
        if let Value::Str(s) = v {
            if let Some(&i) = self.strs.get(s.as_str()) {
                return i;
            }
            let i = self.next();
            self.strs.insert(s.into_boxed_str(), i);
            return i;
        }
        if let Some(&i) = self.other.get(&v) {
            return i;
        }
        let i = self.next();
        self.other.insert(v, i);
        i
    }
    pub fn finish(self) -> Vec<Value> {
        let mut dict = vec![Value::Null; self.strs.len() + self.other.len()];
        for (s, i) in self.strs {
            dict[i as usize] = Value::Str(s.into());
        }
        for (v, i) in self.other {
            dict[i as usize] = v;
        }
        dict
    }
}

/// Dictionary-encoded column of OpenRefine cell values: each distinct value is stored once and
/// rows hold u32 codes, so transforms and facets cost O(distinct) + one pass over codes.
pub struct Column {
    pub dict: Vec<Value>,
    pub codes: Vec<u32>,
}

impl Column {
    pub fn get(&self, row: usize) -> &Value {
        &self.dict[self.codes[row] as usize]
    }

    /// Rows in `sel` (None = all) get `f(value)`; other rows keep their value, or turn
    /// null when `blank_others`. `f` runs once per distinct value; the dict is compacted.
    pub fn map(&self, sel: Option<&[bool]>, blank_others: bool, mut f: impl FnMut(&Value) -> Value) -> Column {
        let mut b = Interner::default();
        let n = self.dict.len();
        let (mut mapped, mut kept) = (vec![NONE; n], vec![NONE; n]);
        let codes = self
            .codes
            .iter()
            .enumerate()
            .map(|(r, &c)| {
                let c = c as usize;
                if sel.map_or(true, |s| s[r]) {
                    if mapped[c] == NONE {
                        mapped[c] = b.intern(f(&self.dict[c]));
                    }
                    mapped[c]
                } else if blank_others {
                    b.intern(Value::Null)
                } else {
                    if kept[c] == NONE {
                        kept[c] = b.intern(self.dict[c].clone());
                    }
                    kept[c]
                }
            })
            .collect();
        Column { dict: b.finish(), codes }
    }

    /// Rows in a new order; `None` makes an empty (null) row. The dictionary is shared, not rebuilt.
    pub fn pick(&self, rows: &[Option<u32>]) -> Column {
        let mut dict = self.dict.clone();
        let null = match dict.iter().position(Value::is_null) {
            Some(i) => i as u32,
            None => {
                dict.push(Value::Null);
                (dict.len() - 1) as u32
            }
        };
        Column { dict, codes: rows.iter().map(|r| r.map_or(null, |r| self.codes[r as usize])).collect() }
    }

    pub fn from_values(values: impl IntoIterator<Item = Value>) -> Column {
        let mut b = Interner::default();
        let codes = values.into_iter().map(|v| b.intern(v)).collect();
        Column { dict: b.finish(), codes }
    }

    /// Like `map`, but `f(row, value)` runs for every selected row (expressions reading other cells).
    pub fn map_rows(&self, sel: Option<&[bool]>, blank_others: bool, mut f: impl FnMut(usize, &Value) -> Value) -> Column {
        let mut b = Interner::default();
        let codes = (0..self.codes.len())
            .map(|r| {
                let v = self.get(r);
                if sel.map_or(true, |s| s[r]) {
                    b.intern(f(r, v))
                } else if blank_others {
                    b.intern(Value::Null)
                } else {
                    b.intern(v.clone())
                }
            })
            .collect();
        Column { dict: b.finish(), codes }
    }
}

/// Immutable snapshot; cloning is cheap (columns are shared), so history keeps one per step.
#[derive(Clone, Default)]
pub struct Table {
    pub names: Vec<String>,
    pub columns: Vec<Arc<Column>>,
    pub rows: usize,
    pub starred: Arc<Vec<bool>>,
    pub flagged: Arc<Vec<bool>>,
    /// Records key column (OpenRefine's keyCellIndex): the first column at import, followed through renames.
    pub key: Option<String>,
}

/// One row as GREL sees it (`cells`, `row`, `rowIndex`).
pub struct RowRef<'a> {
    pub table: &'a Table,
    pub row: usize,
    pub ctx: Option<&'a crate::engine::Ctx>,
}

impl grel::Row for RowRef<'_> {
    fn index(&self) -> usize {
        self.row
    }
    fn columns(&self) -> Vec<String> {
        self.table.names.clone()
    }
    fn cell(&self, column: &str) -> Option<Value> {
        let ci = self.table.names.iter().position(|n| n == column)?;
        Some(self.table.columns[ci].get(self.row).clone())
    }
    fn starred(&self) -> bool {
        self.table.starred[self.row]
    }
    fn flagged(&self) -> bool {
        self.table.flagged[self.row]
    }
    fn facet_count(&self, choice: &Value, expression: &str, column: &str) -> Value {
        match self.ctx {
            Some(ctx) => ctx.facet_count(self.table, choice, expression, column),
            None => Value::err("facetCount needs a project"),
        }
    }
}

impl Table {
    pub fn col(&self, name: &str) -> Result<usize> {
        self.names.iter().position(|n| n == name).ok_or_else(|| format!("no column named '{name}'"))
    }

    pub fn row(&self, row: usize) -> RowRef<'_> {
        RowRef { table: self, row, ctx: None }
    }

    /// A row that can also answer facetCount(), with results shared through `ctx`.
    pub fn row_ctx<'a>(&'a self, row: usize, ctx: &'a crate::engine::Ctx) -> RowRef<'a> {
        RowRef { table: self, row, ctx: Some(ctx) }
    }

    /// Column that delimits records: a non-blank key cell starts a new record.
    pub fn key_index(&self) -> usize {
        self.key.as_ref().and_then(|k| self.names.iter().position(|n| n == k)).unwrap_or(0)
    }

    /// For each row, whether it starts a record (row 0 always does) by the given key column.
    pub fn record_starts(&self, key: usize) -> Vec<bool> {
        let Some(col) = self.columns.get(key) else { return vec![true; self.rows] };
        (0..self.rows).map(|r| r == 0 || !col.get(r).is_blank()).collect()
    }

    /// The table with rows picked/reordered; `None` inserts an empty, unstarred row.
    pub fn pick_rows(&self, rows: &[Option<u32>]) -> Table {
        let flag = |f: &Vec<bool>| Arc::new(rows.iter().map(|r| r.is_some_and(|r| f[r as usize])).collect::<Vec<_>>());
        Table {
            names: self.names.clone(),
            columns: self.columns.iter().map(|c| Arc::new(c.pick(rows))).collect(),
            rows: rows.len(),
            starred: flag(&self.starred),
            flagged: flag(&self.flagged),
            key: self.key.clone(),
        }
    }

    /// CSV/TSV import with OpenRefine's default options: every cell a string, empty cells null.
    pub fn read_csv(path: &Path) -> Result<Table> {
        crate::import::import(path, None, &crate::import::ImportOptions::default())
    }

}
