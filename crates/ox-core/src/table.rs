use crate::{err, Result};
use std::{
    collections::{HashMap, HashSet},
    fs::File,
    io::Read,
    path::Path,
    sync::Arc,
};

const NONE: u32 = u32::MAX;

#[derive(Default)]
pub struct Interner {
    map: HashMap<Box<str>, u32>,
}

impl Interner {
    pub fn intern(&mut self, s: &str) -> u32 {
        if let Some(&i) = self.map.get(s) {
            return i;
        }
        let i = self.map.len() as u32;
        self.map.insert(s.into(), i);
        i
    }

    pub fn finish(self) -> Vec<Box<str>> {
        let mut dict = vec![Box::<str>::default(); self.map.len()];
        for (s, i) in self.map {
            dict[i as usize] = s;
        }
        dict
    }
}

/// Dictionary-encoded string column: each distinct value is stored once and rows
/// hold u32 codes, so transforms and facets cost O(distinct) + one pass over codes.
pub struct Column {
    pub dict: Vec<Box<str>>,
    pub codes: Vec<u32>,
}

impl Column {
    pub fn get(&self, row: usize) -> &str {
        &self.dict[self.codes[row] as usize]
    }

    /// Rows in `sel` (None = all) get `f(value)`; other rows keep their value, or turn
    /// blank when `blank_others`. `f` runs once per distinct value; the dict is compacted.
    pub fn map(&self, sel: Option<&[bool]>, blank_others: bool, mut f: impl FnMut(&str) -> String) -> Column {
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
                        mapped[c] = b.intern(&f(&self.dict[c]));
                    }
                    mapped[c]
                } else if blank_others {
                    b.intern("")
                } else {
                    if kept[c] == NONE {
                        kept[c] = b.intern(&self.dict[c]);
                    }
                    kept[c]
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
}

impl Table {
    pub fn col(&self, name: &str) -> Result<usize> {
        self.names.iter().position(|n| n == name).ok_or_else(|| format!("no column named '{name}'"))
    }

    pub fn read_csv(path: &Path) -> Result<Table> {
        let mut head = vec![0u8; 1 << 16];
        let n = File::open(path)
            .and_then(|mut f| f.read(&mut head))
            .map_err(|e| format!("{}: {e}", path.display()))?;
        let first = head[..n].split(|&b| b == b'\n').next().unwrap_or_default();
        let delim = if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("tsv")) {
            b'\t'
        } else {
            // most frequent candidate in the header line; ties go to ',' (max_by_key keeps the last)
            *[b'|', b';', b'\t', b','].iter().max_by_key(|d| first.iter().filter(|b| b == d).count()).unwrap()
        };
        let mut rdr = csv::ReaderBuilder::new()
            .delimiter(delim)
            .flexible(true)
            .has_headers(false)
            .from_path(path)
            .map_err(err)?;
        let mut rec = csv::ByteRecord::new();
        let mut names: Vec<String> = vec![];
        if rdr.read_byte_record(&mut rec).map_err(err)? {
            names = rec.iter().map(|f| String::from_utf8_lossy(f).trim_start_matches('\u{feff}').trim().to_string()).collect();
        }
        let mut cols: Vec<(Interner, Vec<u32>)> = names.iter().map(|_| Default::default()).collect();
        let mut rows = 0;
        while rdr.read_byte_record(&mut rec).map_err(err)? {
            for (i, f) in rec.iter().enumerate() {
                if i == cols.len() {
                    let mut b = Interner::default();
                    let blank = b.intern("");
                    cols.push((b, vec![blank; rows]));
                }
                let (b, codes) = &mut cols[i];
                codes.push(b.intern(&String::from_utf8_lossy(f)));
            }
            for (b, codes) in &mut cols[rec.len()..] {
                codes.push(b.intern(""));
            }
            rows += 1;
        }
        names.resize(cols.len(), String::new());
        Ok(Table {
            names: dedupe(names),
            columns: cols.into_iter().map(|(b, codes)| Arc::new(Column { dict: b.finish(), codes })).collect(),
            rows,
        })
    }

    pub fn write_csv(&self, path: &Path, rows: &[u32]) -> Result<()> {
        let mut w = csv::Writer::from_path(path).map_err(err)?;
        w.write_record(&self.names).map_err(err)?;
        for &r in rows {
            w.write_record(self.columns.iter().map(|c| c.get(r as usize))).map_err(err)?;
        }
        w.flush().map_err(err)
    }
}

fn dedupe(names: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    names
        .into_iter()
        .enumerate()
        .map(|(i, n)| {
            let base = if n.is_empty() { format!("Column {}", i + 1) } else { n };
            let (mut name, mut k) = (base.clone(), 2);
            while !seen.insert(name.clone()) {
                name = format!("{base} {k}");
                k += 1;
            }
            name
        })
        .collect()
}
