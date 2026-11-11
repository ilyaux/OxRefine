//! OpenRefine's transpose operations, which change the shape of the table: columns into rows, rows
//! into columns, and key/value pairs into columns. They work row by row on plain cell vectors.
// ponytail: the table is copied into row vectors and back (a few times its size in memory);
// work on the dictionary columns directly if these become slow on very large projects.
use crate::{grel::Value, table::{Column, Table}, Result};
use std::collections::HashMap;

fn rows(t: &Table) -> Vec<Vec<Value>> {
    (0..t.rows).map(|r| t.columns.iter().map(|c| c.get(r).clone()).collect()).collect()
}

/// A table from rows (short ones padded with blanks); new rows are neither starred nor flagged,
/// as in OpenRefine. The records key stays on its column when that column survives.
fn table(old: &Table, names: Vec<String>, rows: Vec<Vec<Value>>) -> Table {
    let n = rows.len();
    let columns = (0..names.len()).map(|c| Column::from_values(rows.iter().map(|r| r.get(c).cloned().unwrap_or(Value::Null)))).collect();
    let mut t = Table::new(names, columns, n);
    if let Some(k) = old.key.as_ref().filter(|k| t.names.contains(k)) {
        t.key = Some(k.clone());
    }
    t
}

fn set(row: &mut Vec<Value>, i: usize, v: Value) {
    if row.len() <= i {
        row.resize(i + 1, Value::Null);
    }
    row[i] = v;
}

/// OpenRefine's TransposeColumnsIntoRowsOperation: `count` columns from `start` (all the rest when
/// not positive) become rows, in one combined column or a key and a value column.
#[allow(clippy::too_many_arguments)]
pub fn columns_into_rows(
    t: &Table,
    start: &str,
    count: i64,
    combined: Option<&str>,
    key_value: Option<(&str, &str)>,
    prepend: bool,
    separator: &str,
    ignore_blank: bool,
    fill_down: bool,
) -> Result<Table> {
    let s = t.col(start)?;
    let n = t.names.len();
    let count = if count > 0 { (count as usize).min(n - s) } else { n - s };
    let added: Vec<String> = match (combined, key_value) {
        (Some(c), _) => vec![c.to_string()],
        (None, Some((k, v))) => vec![k.to_string(), v.to_string()],
        (None, None) => return Err("name the new column, or the key and value columns".into()),
    };
    let width = added.len();
    let mut names: Vec<String> = t.names[..s].to_vec();
    names.extend(added);
    names.extend(t.names[s + count..].iter().cloned());

    let mut out: Vec<Vec<Value>> = vec![];
    for row in rows(t) {
        let first = out.len();
        out.push(vec![Value::Null; names.len()]);
        let mut transposed = 0;
        for (c, cell) in row.into_iter().enumerate() {
            if c < s {
                out[first][c] = cell;
            } else if c < s + count {
                let column = &t.names[c];
                let target = |out: &mut Vec<Vec<Value>>, transposed: usize| {
                    if transposed == 0 {
                        first
                    } else {
                        out.push(vec![Value::Null; names.len()]);
                        out.len() - 1
                    }
                };
                if width == 1 {
                    let v = match (&cell, prepend) {
                        (Value::Null, true) if !ignore_blank => Value::Str(format!("{column}{separator}")),
                        (Value::Null, _) => continue,
                        (v, true) => Value::Str(format!("{column}{separator}{}", v.to_java_string())),
                        (v, false) => v.clone(),
                    };
                    let r = target(&mut out, transposed);
                    out[r][s] = v;
                } else {
                    if ignore_blank && cell.is_null() {
                        continue;
                    }
                    let r = target(&mut out, transposed);
                    out[r][s] = Value::Str(column.clone());
                    out[r][s + 1] = cell;
                }
                transposed += 1;
            } else {
                out[first][c - count + width] = cell;
            }
        }
        if fill_down {
            for r in first + 1..out.len() {
                for c in (0..names.len()).filter(|&c| c < s || c >= s + width) {
                    if out[r][c].is_null() && !out[first][c].is_null() {
                        out[r][c] = out[first][c].clone();
                    }
                }
            }
        }
    }
    Ok(table(t, names, out))
}

/// OpenRefine's TransposeRowsIntoColumnsOperation: every `k` cells down `column` go side by side
/// in columns "<column> 1" … "<column> k"; other cells of those rows keep rows of their own.
pub fn rows_into_columns(t: &Table, column: &str, k: usize) -> Result<Table> {
    let ci = t.col(column)?;
    let k = k.max(1);
    let mut names: Vec<String> = t.names[..ci].to_vec();
    names.extend((1..=k).map(|i| format!("{column} {i}")));
    names.extend(t.names[ci + 1..].iter().cloned());
    let old = rows(t);
    let mut out: Vec<Vec<Value>> = vec![];
    for group in old.chunks(k) {
        let first = out.len();
        out.push(vec![Value::Null; names.len()]);
        for (r2, row) in group.iter().enumerate() {
            let mut new_row = vec![Value::Null; names.len()];
            let mut has_data = false;
            for (c, cell) in row.iter().enumerate().filter(|(_, v)| !v.is_null()) {
                if c == ci {
                    out[first][ci + r2] = cell.clone();
                } else if r2 == 0 {
                    out[first][if c < ci { c } else { c + k - 1 }] = cell.clone();
                } else {
                    new_row[if c < ci { c } else { c + k - 1 }] = cell.clone();
                    has_data = true;
                }
            }
            if r2 > 0 && has_data {
                out.push(new_row);
            }
        }
    }
    Ok(table(t, names, out))
}

/// A column name not taken yet: the name itself, else with 2, 3… appended.
fn unduplicated(names: &[String], base: &str) -> String {
    if !names.iter().any(|n| n == base) {
        return base.to_string();
    }
    (2..).map(|i| format!("{base}{i}")).find(|n| !names.contains(n)).unwrap()
}

/// OpenRefine's KeyValueColumnizeOperation: each distinct key becomes a column holding its values.
/// Rows that agree on every other column are merged into one; with no other columns, a new row
/// starts at each repeat of the first key. Notes go into "<note column> : <key>" columns, joined by ";".
pub fn key_value_columnize(t: &Table, key: &str, value: &str, note: Option<&str>) -> Result<Table> {
    let (kc, vc) = (t.col(key)?, t.col(value)?);
    let nc = note.map(|n| t.col(n)).transpose()?;
    let unchanged: Vec<usize> = (0..t.names.len()).filter(|&c| c != kc && c != vc && Some(c) != nc).collect();
    let mut names: Vec<String> = unchanged.iter().map(|&c| t.names[c].clone()).collect();
    let mut value_cols: HashMap<String, usize> = HashMap::new();
    let mut note_keys: Vec<String> = vec![];
    let mut out: Vec<Vec<Value>> = vec![];
    let mut groups: HashMap<String, usize> = HashMap::new();
    let mut current: Vec<usize> = vec![];
    let mut record_key: Option<String> = None;
    let degenerate = unchanged.is_empty();
    // value columns get their place now; note columns go after all of them, so they are numbered later
    let mut notes: Vec<(usize, String, Value)> = vec![];
    for row in rows(t) {
        if row[kc].is_blank() {
            if degenerate {
                current.clear();
            } else {
                out.push(unchanged.iter().map(|&c| row[c].clone()).collect());
            }
            continue;
        }
        let k = row[kc].to_java_string();
        if degenerate && record_key.as_ref().is_none_or(|r| *r == k) {
            out.push(vec![]);
            current = vec![out.len() - 1];
        }
        let col = match value_cols.get(&k) {
            Some(&c) => c,
            None => {
                let name = unduplicated(&names, &k);
                names.push(name);
                value_cols.insert(k.clone(), names.len() - 1);
                record_key.get_or_insert_with(|| k.clone());
                names.len() - 1
            }
        };
        let target = if degenerate {
            match current.iter().copied().find(|&r| out[r].get(col).is_none_or(Value::is_null)) {
                Some(r) => r,
                None => {
                    out.push(vec![]);
                    current.push(out.len() - 1);
                    out.len() - 1
                }
            }
        } else {
            let sig = unchanged.iter().map(|&c| if row[c].is_null() { String::new() } else { row[c].to_java_string() }).collect::<Vec<_>>().join("\u{0}");
            *groups.entry(sig).or_insert_with(|| {
                out.push(unchanged.iter().map(|&c| row[c].clone()).collect());
                out.len() - 1
            })
        };
        set(&mut out[target], col, row[vc].clone());
        if let Some(n) = nc.filter(|&n| !row[n].is_blank()) {
            if !note_keys.contains(&k) {
                note_keys.push(k.clone());
            }
            notes.push((target, k, row[n].clone()));
        }
    }
    let first_note = names.len();
    if let Some(n) = note {
        for k in &note_keys {
            let name = unduplicated(&names, &format!("{n} : {k}"));
            names.push(name);
        }
    }
    for (r, k, v) in notes {
        let c = first_note + note_keys.iter().position(|x| *x == k).unwrap();
        let joined = match out[r].get(c) {
            Some(old) if !old.is_null() => Value::Str(format!("{};{}", old.to_java_string(), v.to_java_string())),
            _ => v,
        };
        set(&mut out[r], c, joined);
    }
    if degenerate {
        out.retain(|r| r.iter().any(|v| !v.is_null()));
    }
    Ok(table(t, names, out))
}
