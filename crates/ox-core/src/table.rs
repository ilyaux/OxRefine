use crate::{grel, grel::Value, spill::{CodeBudget, CodeBuilder, Codes}, Result};
use std::{path::Path, sync::Arc};

const NONE: u32 = u32::MAX;

/// Assigns dictionary codes. The values live once, in the dictionary itself; the table only holds
/// their codes (4 bytes a slot), so a column of a million distinct values costs the values plus a few
/// MB, also while the table grows. Strings can be looked up as `&str`, without allocating.
#[derive(Default)]
pub struct Interner {
    dict: Vec<Value>,
    index: hashbrown::HashTable<u32>,
}

/// A string hashes the same as a `&str` and as `Value::Str`, so CSV import can look it up borrowed.
fn hash_str(s: &str) -> u64 {
    std::hash::BuildHasher::hash_one(&rustc_hash::FxBuildHasher, s)
}

fn hash_value(v: &Value) -> u64 {
    match v {
        Value::Str(s) => hash_str(s),
        v => std::hash::BuildHasher::hash_one(&rustc_hash::FxBuildHasher, v),
    }
}

impl Interner {
    fn insert(&mut self, hash: u64, v: Value) -> u32 {
        let i = self.dict.len() as u32;
        self.dict.push(v);
        let dict = &self.dict;
        self.index.insert_unique(hash, i, |&j| hash_value(&dict[j as usize]));
        i
    }
    pub fn intern_str(&mut self, s: &str) -> u32 {
        let h = hash_str(s);
        let dict = &self.dict;
        if let Some(&i) = self.index.find(h, |&j| matches!(&dict[j as usize], Value::Str(t) if t == s)) {
            return i;
        }
        self.insert(h, Value::Str(s.into()))
    }
    pub fn intern(&mut self, v: Value) -> u32 {
        let h = hash_value(&v);
        let dict = &self.dict;
        if let Some(&i) = self.index.find(h, |&j| dict[j as usize] == v) {
            return i;
        }
        self.insert(h, v)
    }
    pub fn finish(self) -> Vec<Value> {
        self.dict
    }
}

/// Dictionary-encoded column of OpenRefine cell values: each distinct value is stored once and
/// rows hold u32 codes, so transforms and facets cost O(distinct) + one pass over codes.
#[derive(Default)]
pub struct Column {
    pub dict: Vec<Value>,
    /// Immutable shared row codes, held in memory or an owned spill mapping. Edits create a new
    /// code sequence so older history snapshots stay unchanged.
    pub codes: Codes,
    /// Reconciliation data, row by row, for columns that were reconciled.
    pub recon: Option<Arc<crate::recon::ReconCol>>,
}

impl Column {
    pub fn get(&self, row: usize) -> &Value {
        &self.dict[self.codes[row] as usize]
    }

    /// The Recon of a row's cell, if it has one.
    pub fn recon(&self, row: usize) -> Option<&Arc<crate::recon::Recon>> {
        self.recon.as_ref().and_then(|rc| rc.cells.get(row)).and_then(Option::as_ref)
    }

    /// This column with another column's reconciliation data (same rows): what operations that
    /// change cell values in place (transforms, mass edits) do in OpenRefine.
    pub fn keep_recon(mut self, from: &Column) -> Column {
        self.recon = from.recon.clone();
        self
    }

    /// Rows in `sel` (None = all) get `f(value)`; other rows keep their value, or turn
    /// null when `blank_others`. `f` runs once per distinct value; the dict is compacted.
    pub fn try_map(&self, sel: Option<&[bool]>, blank_others: bool, budget: &CodeBudget, mut f: impl FnMut(&Value) -> Value) -> Result<Column> {
        let mut b = Interner::default();
        let n = self.dict.len();
        let (mut mapped, mut kept) = (vec![NONE; n], vec![NONE; n]);
        let mut codes = CodeBuilder::new(budget.clone());
        let mut block = [0; 16 * 1024];
        for (page, rows) in self.codes.chunks(block.len()).enumerate() {
            for (i, &c) in rows.iter().enumerate() {
                let r = page * block.len() + i;
                let c = c as usize;
                block[i] = if sel.map_or(true, |s| s[r]) {
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
                };
            }
            codes.extend_slice(&block[..rows.len()])?;
        }
        Ok(Column { dict: b.finish(), codes: codes.finish()?, recon: None })
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
        let recon = self.recon.as_ref().map(|rc| {
            let cells = rows.iter().map(|r| r.and_then(|r| rc.cells[r as usize].clone())).collect();
            Arc::new(crate::recon::ReconCol { cells, config: rc.config.clone(), stats: rc.stats.clone() })
        });
        Column { dict, codes: rows.iter().map(|r| r.map_or(null, |r| self.codes[r as usize])).collect::<Vec<u32>>().into(), recon }
    }

    /// Fallible row selection under the shared runtime code budget.
    pub fn try_pick(&self, rows: &[Option<u32>], budget: &CodeBudget) -> Result<Column> {
        let mut dict = self.dict.clone();
        let null = match dict.iter().position(Value::is_null) {
            Some(i) => i as u32,
            None => {
                dict.push(Value::Null);
                (dict.len() - 1) as u32
            }
        };
        let mut codes = CodeBuilder::new(budget.clone());
        for r in rows {
            codes.push(r.map_or(null, |r| self.codes[r as usize]))?;
        }
        let recon = self.recon.as_ref().map(|rc| {
            let cells = rows.iter().map(|r| r.and_then(|r| rc.cells[r as usize].clone())).collect();
            Arc::new(crate::recon::ReconCol { cells, config: rc.config.clone(), stats: rc.stats.clone() })
        });
        Ok(Column { dict, codes: codes.finish()?, recon })
    }

    pub fn from_values(values: impl IntoIterator<Item = Value>) -> Column {
        let mut b = Interner::default();
        let codes: Vec<u32> = values.into_iter().map(|v| b.intern(v)).collect();
        Column { dict: b.finish(), codes: codes.into(), recon: None }
    }

    /// Build row codes incrementally, spilling before the resident code budget is exceeded.
    pub fn try_from_values(values: impl IntoIterator<Item = Value>, budget: &CodeBudget) -> Result<Column> {
        let mut b = Interner::default();
        let mut codes = CodeBuilder::new(budget.clone());
        for value in values {
            codes.push(b.intern(value))?;
        }
        Ok(Column { dict: b.finish(), codes: codes.finish()?, recon: None })
    }

    /// Like `try_map`, but `f(row, value)` runs for every selected row (expressions reading other cells).
    pub fn try_map_rows(&self, sel: Option<&[bool]>, blank_others: bool, budget: &CodeBudget, mut f: impl FnMut(usize, &Value) -> Value) -> Result<Column> {
        let mut b = Interner::default();
        let mut codes = CodeBuilder::new(budget.clone());
        for r in 0..self.codes.len() {
            let v = self.get(r);
            let code = if sel.map_or(true, |s| s[r]) {
                b.intern(f(r, v))
            } else if blank_others {
                b.intern(Value::Null)
            } else {
                b.intern(v.clone())
            };
            codes.push(code)?;
        }
        Ok(Column { dict: b.finish(), codes: codes.finish()?, recon: None })
    }
}

#[cfg(test)]
mod spill_tests {
    use super::*;

    fn values(col: &Column) -> Vec<Value> { (0..col.codes.len()).map(|r| col.get(r).clone()).collect() }

    #[test]
    fn chunked_mapping_keeps_lazy_calls_merges_and_masks_across_the_boundary() {
        const EDGE: usize = 16_384;
        let dir = tempfile::tempdir().unwrap();
        for limit in [usize::MAX, 0] {
            let budget = CodeBudget::new(limit, dir.path().into());
            let mut source = vec![3; EDGE + 5];
            for (r, code) in [(1, 2), (EDGE - 1, 4), (EDGE, 5), (EDGE + 1, 2), (EDGE + 3, 4), (EDGE + 4, 5)] { source[r] = code; }
            let mut codes = CodeBuilder::new(budget.clone());
            codes.extend_slice(&source).unwrap();
            let col = Column { dict: vec![Value::Null, Value::Str("unused".into()), Value::Str("a".into()), Value::Str("b".into()), Value::Error("bad".into()), Value::Str(String::new())], codes: codes.finish().unwrap(), recon: None };
            let original = col.codes.clone();
            let mut selected = vec![false; source.len()];
            for r in [0, EDGE - 1, EDGE, EDGE + 1, EDGE + 3] { selected[r] = true; }
            for blank in [false, true] {
                let mut calls = vec![];
                let out = col.try_map(Some(&selected), blank, &budget, |v| {
                    calls.push(v.clone());
                    match v {
                        Value::Str(s) if s == "a" || s == "b" => Value::Str("merged".into()),
                        Value::Error(_) => Value::Null,
                        Value::Str(s) if s.is_empty() => Value::Error("empty".into()),
                        _ => panic!("unselected or unused dictionary entries must not be evaluated"),
                    }
                }).unwrap();
                assert_eq!(calls, vec![Value::Str("b".into()), Value::Error("bad".into()), Value::Str(String::new()), Value::Str("a".into())]);
                let mut expected = vec![if blank { Value::Null } else { Value::Str("b".into()) }; source.len()];
                expected[0] = Value::Str("merged".into());
                expected[1] = if blank { Value::Null } else { Value::Str("a".into()) };
                expected[EDGE - 1] = Value::Null;
                expected[EDGE] = Value::Error("empty".into());
                expected[EDGE + 1] = Value::Str("merged".into());
                expected[EDGE + 3] = Value::Null;
                expected[EDGE + 4] = if blank { Value::Null } else { Value::Str(String::new()) };
                assert_eq!(values(&out), expected);
                assert_eq!(out.codes.is_mapped(), limit == 0);
                assert!(Codes::ptr_eq(&original, &col.codes));
                assert_eq!(&*col.codes, source.as_slice());
            }
            assert!(budget.stats().heap_peak_bytes <= limit);
        }
    }

    #[test]
    fn chunked_unfiltered_mapping_handles_empty_full_and_partial_blocks() {
        let dir = tempfile::tempdir().unwrap();
        let budget = CodeBudget::new(usize::MAX, dir.path().into());
        for len in [0, 1, 16_383, 16_384, 16_385, 32_769] {
            let col = Column::from_values((0..len).map(|r| Value::Int((r % 2 + 1) as i64)));
            let mut calls = vec![];
            let out = col.try_map(None, false, &budget, |v| { calls.push(v.clone()); Value::Int(7) }).unwrap();
            assert_eq!(values(&out), vec![Value::Int(7); len]);
            let expected = match len { 0 => vec![], 1 => vec![Value::Int(1)], _ => vec![Value::Int(1), Value::Int(2)] };
            assert_eq!(calls, expected);
        }
    }

    #[test]
    fn mapped_columns_transform_by_distinct_value_or_row_without_changing_input() {
        let dir = tempfile::tempdir().unwrap();
        let budget = CodeBudget::new(0, dir.path().into());
        let original = vec![Value::Str("a".into()), Value::Str("b".into()), Value::Str("a".into()), Value::Null];
        let col = Column::try_from_values(original.clone(), &budget).unwrap();
        assert!(col.codes.is_mapped());
        let mut calls = 0;
        let mapped = col.try_map(None, false, &budget, |v| { calls += 1; if v.is_null() { Value::Null } else { Value::Str("merged".into()) } }).unwrap();
        assert_eq!(calls, 3, "expression still runs once per distinct value");
        assert!(mapped.codes.is_mapped());
        assert_eq!(values(&mapped), vec![Value::Str("merged".into()), Value::Str("merged".into()), Value::Str("merged".into()), Value::Null]);
        let selected = [true, false, true, false];
        let by_row = col.try_map_rows(Some(&selected), true, &budget, |r, _| Value::Int(r as i64)).unwrap();
        assert!(by_row.codes.is_mapped());
        assert_eq!(values(&by_row), vec![Value::Int(0), Value::Null, Value::Int(2), Value::Null]);
        assert_eq!(values(&col), original, "history source stays immutable");
        assert_eq!(budget.stats().heap_peak_bytes, 0);
        assert!(budget.stats().mapped_bytes >= 3 * 4 * 4);
        drop((col, mapped, by_row));
        assert_eq!(budget.stats().mapped_bytes, 0);
        assert_eq!(budget.stats().spill_files, 0);
    }

    #[test]
    fn mapped_row_selection_preserves_values_order_flags_and_null_rows() {
        let dir = tempfile::tempdir().unwrap();
        let budget = CodeBudget::new(0, dir.path().into());
        let col = Column::try_from_values([Value::Int(10), Value::Int(20)], &budget).unwrap();
        let mut t = Table::new(vec!["key".into()], vec![col], 2);
        t.starred = Arc::new(vec![true, false]);
        t.flagged = Arc::new(vec![false, true]);
        let selected = t.try_pick_rows(&[Some(1), None, Some(0), Some(1)], &budget).unwrap();
        assert!(selected.columns[0].codes.is_mapped());
        assert_eq!(values(&selected.columns[0]), vec![Value::Int(20), Value::Null, Value::Int(10), Value::Int(20)]);
        assert_eq!(&*selected.starred, &[false, false, true, false]);
        assert_eq!(&*selected.flagged, &[true, false, false, true]);
        assert_eq!(values(&t.columns[0]), vec![Value::Int(10), Value::Int(20)]);
    }

    #[test]
    fn failed_spill_does_not_replace_source_codes_or_values() {
        let dir = tempfile::tempdir().unwrap();
        let not_a_directory = dir.path().join("file");
        std::fs::write(&not_a_directory, b"unchanged").unwrap();
        let budget = CodeBudget::new(0, not_a_directory.clone());
        let col = Column::from_values([Value::Int(10), Value::Int(20)]);
        let before = col.codes.clone();
        assert!(col.try_map(None, false, &budget, |_| Value::Int(30)).is_err());
        assert!(col.try_map_rows(None, false, &budget, |r, _| Value::Int(r as i64)).is_err());
        assert!(col.try_pick(&[Some(1), Some(0)], &budget).is_err());
        assert!(Codes::ptr_eq(&before, &col.codes));
        assert_eq!(values(&col), vec![Value::Int(10), Value::Int(20)]);
        assert_eq!(std::fs::read(not_a_directory).unwrap(), b"unchanged");
        assert_eq!(budget.stats().mapped_bytes, 0);
        assert_eq!(budget.stats().spill_files, 0);
    }

    #[test]
    fn operations_share_budget_and_keep_undo_history_when_code_io_fails() {
        use serde_json::json;
        let dir = tempfile::tempdir().unwrap();
        let budget = CodeBudget::new(0, dir.path().into());
        let col = Column::try_from_values(["a", "b", "a"].map(|s| Value::Str(s.into())), &budget).unwrap();
        let mut p = crate::Project::new("spill history", Table::new(vec!["name".into()], vec![col], 3));
        crate::storage::with_budget(budget.clone(), || {
            for op in [
                json!({ "op": "core/text-transform", "columnName": "name", "expression": "value.toUppercase()" }),
                json!({ "op": "core/mass-edit", "columnName": "name", "expression": "value", "edits": [{ "from": ["A"], "to": "B" }] }),
                json!({ "op": "ox/cell-edit", "columnName": "name", "row": 1, "value": "C", "type": "text" }),
                json!({ "op": "core/column-addition", "baseColumnName": "name", "newColumnName": "index", "columnInsertIndex": 1, "expression": "rowIndex" }),
            ] {
                p.apply(serde_json::from_value(op).unwrap()).unwrap();
                assert!(p.table().columns.iter().all(|c| c.codes.is_mapped()));
            }
        });
        assert_eq!(values(&p.table().columns[0]), ["B", "C", "B"].map(|s| Value::Str(s.into())));
        assert_eq!(values(&p.table().columns[1]), vec![Value::Int(0), Value::Int(1), Value::Int(2)]);
        p.goto(1).unwrap();
        assert_eq!(values(&p.table().columns[0]), ["A", "B", "A"].map(|s| Value::Str(s.into())));
        p.goto(4).unwrap();
        let before = p.table().columns[0].codes.clone();
        let bad_path = dir.path().join("not-a-directory");
        std::fs::write(&bad_path, b"keep").unwrap();
        let broken = CodeBudget::new(0, bad_path);
        let result = crate::storage::with_budget(broken, || p.apply(serde_json::from_value(json!({
            "op": "ox/cell-edit", "columnName": "name", "row": 0, "value": "failed", "type": "text",
        })).unwrap()));
        assert!(result.is_err());
        assert_eq!(p.steps().len(), 4);
        assert!(Codes::ptr_eq(&before, &p.table().columns[0].codes));
        p.goto(0).unwrap();
        assert_eq!(values(&p.table().columns[0]), ["a", "b", "a"].map(|s| Value::Str(s.into())));
        p.goto(4).unwrap();
        assert_eq!(values(&p.table().columns[0]), ["B", "C", "B"].map(|s| Value::Str(s.into())));
        assert_eq!(budget.stats().heap_peak_bytes, 0);
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
    /// the column of the cell the expression runs on (for `cell.recon`)
    pub col: Option<usize>,
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
    fn recon(&self, column: &str) -> Option<Arc<crate::recon::Recon>> {
        let ci = self.table.names.iter().position(|n| n == column)?;
        self.table.columns[ci].recon(self.row).cloned()
    }
    fn current_recon(&self) -> Option<Arc<crate::recon::Recon>> {
        self.table.columns.get(self.col?)?.recon(self.row).cloned()
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
    fn record(&self) -> Value {
        let t = self.table;
        let owned;
        let starts = match self.ctx {
            Some(ctx) => ctx.record_starts(t),
            None => {
                owned = crate::engine::record_starts(t);
                &owned
            }
        };
        let index = starts.partition_point(|&s| s <= self.row).saturating_sub(1);
        let (from, to) = (starts.get(index).copied().unwrap_or(0), starts.get(index + 1).copied().unwrap_or(t.rows));
        grel::eval::record_value(index, from, to, &t.names, |r, c| (t.columns[c].get(r).clone(), t.columns[c].recon(r).cloned()))
    }
}

impl Table {
    pub fn col(&self, name: &str) -> Result<usize> {
        self.names.iter().position(|n| n == name).ok_or_else(|| format!("no column named '{name}'"))
    }

    pub fn row(&self, row: usize) -> RowRef<'_> {
        RowRef { table: self, row, ctx: None, col: None }
    }

    /// A row that can also answer facetCount(), with results shared through `ctx`.
    pub fn row_ctx<'a>(&'a self, row: usize, ctx: &'a crate::engine::Ctx) -> RowRef<'a> {
        RowRef { table: self, row, ctx: Some(ctx), col: None }
    }

    /// A row whose expressions run on the cell in column `col`.
    pub fn row_at<'a>(&'a self, row: usize, ctx: &'a crate::engine::Ctx, col: usize) -> RowRef<'a> {
        RowRef { table: self, row, ctx: Some(ctx), col: Some(col) }
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

    pub fn try_pick_rows(&self, rows: &[Option<u32>], budget: &CodeBudget) -> Result<Table> {
        let columns = self.columns.iter().map(|c| c.try_pick(rows, budget).map(Arc::new)).collect::<Result<Vec<_>>>()?;
        let flag = |f: &Vec<bool>| Arc::new(rows.iter().map(|r| r.is_some_and(|r| f[r as usize])).collect::<Vec<_>>());
        Ok(Table { names: self.names.clone(), columns, rows: rows.len(), starred: flag(&self.starred), flagged: flag(&self.flagged), key: self.key.clone() })
    }

    /// CSV/TSV import with OpenRefine's default options: every cell a string, empty cells null.
    pub fn read_csv(path: &Path) -> Result<Table> {
        crate::import::import(path, None, &crate::import::ImportOptions::default())
    }

}
