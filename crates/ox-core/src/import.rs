//! Importers with OpenRefine's options: delimited text, plain lines, Excel/ODS and JSON, plus the
//! clipboard and web-address sources (staged as files, then imported like any other file).
//! Behaviour is pinned to OpenRefine 3.10.1 by tests/import_conformance.rs.
use crate::{
    err,
    grel::{funcs::parse_number, value::from_json, Value},
    table::{Column, Interner, Table},
    Result,
};
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, fs::File, io::Read, path::Path, sync::Arc};

/// OpenRefine's importer options (same JSON names), with its defaults.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ImportOptions {
    /// Text encoding label ("UTF-8", "ISO-8859-1", "windows-1252"...); empty means UTF-8.
    pub encoding: String,
    /// Field separator; None means detect it from the first line.
    pub separator: Option<String>,
    pub ignore_lines: i64,
    pub header_lines: i64,
    pub skip_data_lines: i64,
    pub limit: i64,
    pub store_blank_rows: bool,
    pub store_blank_cells_as_nulls: bool,
    pub guess_cell_value_types: bool,
    pub process_quotes: bool,
    pub quote_character: String,
    pub trim_strings: bool,
    /// Excel/ODS sheets as OpenRefine lists them ("file#index"); empty means the first sheet.
    pub sheets: Vec<SheetChoice>,
    /// JSON path to the records, e.g. ["_", "_"] for a top-level array of objects.
    pub record_path: Vec<String>,
    pub store_empty_strings: bool,
    /// Line-based text: how many lines make one row.
    pub lines_per_row: i64,
}

impl Default for ImportOptions {
    fn default() -> Self {
        ImportOptions {
            encoding: String::new(),
            separator: None,
            ignore_lines: -1,
            header_lines: 1,
            skip_data_lines: 0,
            limit: -1,
            store_blank_rows: true,
            store_blank_cells_as_nulls: true,
            guess_cell_value_types: false,
            process_quotes: true,
            quote_character: "\"".into(),
            trim_strings: false,
            sheets: vec![],
            record_path: vec!["_".into(), "_".into()],
            store_empty_strings: true,
            lines_per_row: 1,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SheetChoice {
    pub name: String,
    pub file_name_and_sheet_index: String,
    pub selected: bool,
}

impl Default for SheetChoice {
    fn default() -> Self {
        SheetChoice { name: String::new(), file_name_and_sheet_index: String::new(), selected: true }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    Csv,
    /// one line per row, no separator ("text/line-based")
    Lines,
    Excel,
    Ods,
    Json,
    JsonLines,
}

/// OpenRefine format ids, or our own short names; None picks by file extension.
pub fn format_for(path: &Path, format: Option<&str>) -> Format {
    match format.unwrap_or("") {
        "text/line-based/*sv" | "csv" | "tsv" => return Format::Csv,
        "text/line-based" | "lines" => return Format::Lines,
        "binary/text/xml/xls/xlsx" | "excel" => return Format::Excel,
        "text/xml/ods" | "ods" => return Format::Ods,
        "text/json" | "json" => return Format::Json,
        "jsonl" | "jsonlines" => return Format::JsonLines,
        _ => {}
    }
    let ext = path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    match ext.as_str() {
        "xlsx" | "xlsm" | "xlsb" | "xls" => Format::Excel,
        "ods" => Format::Ods,
        "json" => Format::Json,
        "jsonl" | "ndjson" => Format::JsonLines,
        _ => Format::Csv,
    }
}

pub fn import(path: &Path, format: Option<&str>, o: &ImportOptions) -> Result<Table> {
    crate::jobs::progress(&format!("Reading {}", path.file_name().map_or(String::new(), |n| n.to_string_lossy().into_owned())), 0);
    match format_for(path, format) {
        Format::Csv => import_text(path, o),
        Format::Lines => import_lines(path, o),
        Format::Excel | Format::Ods => import_sheets(path, o),
        Format::Json | Format::JsonLines => import_json(path, format_for(path, format) == Format::JsonLines, o),
    }
}

/// What the import wizard needs before options are chosen: the guessed format and OpenRefine's
/// wizard defaults for it (`options`). Text with no separator in its first line is line-based,
/// unless the file says it is CSV/TSV.
pub fn inspect(path: &Path) -> Result<serde_json::Value> {
    use serde_json::json;
    let mut f = format_for(path, None);
    let lines = json!({ "ignoreLines": -1, "headerLines": 1, "skipDataLines": 0, "limit": -1, "storeBlankRows": true, "storeBlankCellsAsNulls": true });
    let merge = |extra: serde_json::Value| {
        let mut o = lines.clone();
        o.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        o
    };
    let mut out = json!({});
    let declared = path.extension().is_some_and(|e| ["csv", "tsv", "tab"].iter().any(|x| e.eq_ignore_ascii_case(x)));
    let sep = if f == Format::Csv { guess_separator(path)? } else { None };
    if f == Format::Csv && sep.is_none() && !declared {
        f = Format::Lines;
    }
    out["options"] = match f {
        Format::Excel | Format::Ods => {
            use calamine::Reader;
            let names = calamine::open_workbook_auto(path).map_err(err)?.sheet_names();
            let sheets: Vec<_> = names.iter().enumerate().map(|(i, n)| json!({ "name": n, "fileNameAndSheetIndex": format!("file#{i}"), "selected": true })).collect();
            out["sheets"] = json!(names);
            merge(json!({ "sheets": sheets }))
        }
        Format::Csv => {
            let sep = sep.unwrap_or(',');
            out["separator"] = json!(sep.to_string());
            // OpenRefine's wizard turns quote handling off for tab-separated text
            merge(json!({ "separator": sep.to_string(), "encoding": "UTF-8", "processQuotes": sep != '\t', "quoteCharacter": "\"", "trimStrings": true, "guessCellValueTypes": false }))
        }
        Format::Lines => merge(json!({ "headerLines": 0, "linesPerRow": 1, "encoding": "UTF-8", "guessCellValueTypes": false })),
        Format::Json | Format::JsonLines => json!({ "recordPath": ["_", "_"], "limit": -1, "trimStrings": false, "guessCellValueTypes": false, "storeEmptyStrings": true }),
    };
    out["format"] = json!(f);
    Ok(out)
}

/// Where pasted text and downloads wait for the import wizard: next to the store's `projects`
/// folder (given as `projects`), else in the temp folder.
pub fn staging_dir(projects: Option<&Path>) -> std::path::PathBuf {
    projects.map_or_else(|| std::env::temp_dir().join("oxrefine-staging"), |p| p.parent().unwrap_or(p).join("staging"))
}

/// A fresh folder under `dir`; staged files older than a day are cleared on the way.
fn stage_folder(dir: &Path) -> Result<std::path::PathBuf> {
    if let Ok(entries) = std::fs::read_dir(dir) {
        let day = std::time::Duration::from_secs(86_400);
        for e in entries.flatten() {
            if e.metadata().and_then(|m| m.modified()).is_ok_and(|t| t.elapsed().unwrap_or_default() > day) {
                let _ = std::fs::remove_dir_all(e.path());
            }
        }
    }
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos();
    let folder = dir.join(nanos.to_string());
    std::fs::create_dir_all(&folder).map_err(|e| format!("{}: {e}", folder.display()))?;
    Ok(folder)
}

/// OpenRefine's Clipboard tab: the pasted text becomes a file to import.
pub fn stage_text(dir: &Path, text: &str) -> Result<std::path::PathBuf> {
    let path = stage_folder(dir)?.join("clipboard.txt");
    std::fs::write(&path, text).map_err(err)?;
    Ok(path)
}

/// OpenRefine's Web Addresses tab: download `url` into a staged file. Returns the file and the name
/// OpenRefine gives it (the last path segment). A name without an extension gets one from the
/// Content-Type, so the format is guessed as OpenRefine does.
pub fn download(dir: &Path, url: &str) -> Result<(std::path::PathBuf, String)> {
    let parsed = url::Url::parse(url.trim()).map_err(|e| format!("{url}: {e}"))?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(format!("{url}: only http and https addresses can be imported"));
    }
    crate::jobs::progress(&format!("Connecting to {}", parsed.host_str().unwrap_or("")), 0);
    let mut res = ureq::get(parsed.as_str()).call().map_err(|e| format!("{url}: {e}"))?;
    let size = res.headers().get("content-length").and_then(|v| v.to_str().ok()?.parse().ok()).unwrap_or(0);
    let name = parsed.path_segments().and_then(|mut s| s.next_back()).filter(|s| !s.is_empty()).unwrap_or("download");
    let name: String = name.chars().map(|c| if "\\/:*?\"<>|".contains(c) { '_' } else { c }).collect();
    let mime = res.headers().get("content-type").and_then(|v| v.to_str().ok()).unwrap_or("").split(';').next().unwrap_or("").trim().to_ascii_lowercase();
    let ext = match mime.as_str() {
        _ if Path::new(&name).extension().is_some() => "",
        "text/csv" => ".csv",
        "text/tab-separated-values" => ".tsv",
        "application/json" => ".json",
        "application/x-ndjson" | "application/jsonl" => ".jsonl",
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet" => ".xlsx",
        "application/vnd.ms-excel" => ".xls",
        "application/vnd.oasis.opendocument.spreadsheet" => ".ods",
        _ => "",
    };
    let path = stage_folder(dir)?.join(format!("{name}{ext}"));
    let mut file = File::create(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    crate::jobs::progress(&format!("Downloading {name}"), size);
    if let Err(e) = std::io::copy(&mut crate::jobs::Counted(res.body_mut().as_reader()), &mut file) {
        drop(file);
        let _ = std::fs::remove_dir_all(path.parent().unwrap_or(&path));
        return Err(if crate::jobs::is_cancel(&e) { crate::jobs::CANCELLED.into() } else { format!("{url}: {e}") });
    }
    Ok((path, name))
}

// ---------------------------------------------------------------- tabular phases

/// Feeds rows through OpenRefine's phases (ignore → header → skip → data, limit) into columns.
struct Builder<'o> {
    o: &'o ImportOptions,
    line: usize,
    header: Vec<Vec<String>>,
    cols: Vec<(Interner, Vec<u32>)>,
    rows: usize,
}

enum Field<'a> {
    Text(&'a str),
    Val(Value),
}

impl<'o> Builder<'o> {
    fn new(o: &'o ImportOptions) -> Self {
        Builder { o, line: 0, header: vec![], cols: vec![], rows: 0 }
    }

    /// Returns false once the row limit is reached.
    fn push<'a>(&mut self, fields: impl Iterator<Item = Field<'a>>) -> bool {
        let mut fields: Vec<Field> = fields.collect();
        if self.line == 0 {
            if let Some(Field::Text(s)) = fields.first_mut() {
                *s = s.strip_prefix('\u{feff}').unwrap_or(s);
            }
        }
        let (ignore, header, skip) = (self.o.ignore_lines.max(0) as usize, self.o.header_lines.max(0) as usize, self.o.skip_data_lines.max(0) as usize);
        let i = self.line;
        self.line += 1;
        if i < ignore + header + skip {
            if i >= ignore && i < ignore + header {
                self.header.push(fields.into_iter().map(|f| match f {
                    Field::Text(s) => s.trim().to_string(),
                    Field::Val(v) => v.display(),
                }).collect());
            }
            return true;
        }
        if self.o.limit >= 0 && self.rows as i64 >= self.o.limit {
            return false;
        }
        if !self.o.store_blank_rows && fields.iter().all(|f| match f {
            Field::Text(s) => s.trim().is_empty(),
            Field::Val(v) => v.is_blank(),
        }) {
            return true;
        }
        let n = fields.len();
        for (c, f) in fields.into_iter().enumerate() {
            if c == self.cols.len() {
                let mut b = Interner::default();
                let null = b.intern(Value::Null);
                self.cols.push((b, vec![null; self.rows]));
            }
            let (b, codes) = &mut self.cols[c];
            codes.push(code(self.o, b, f));
        }
        self.pad(n);
        true
    }

    /// Fast path for a parsed CSV record in the data phase: no per-row allocation.
    fn push_bytes(&mut self, rec: &csv::ByteRecord) -> bool {
        let first_data = (self.o.ignore_lines.max(0) + self.o.header_lines.max(0) + self.o.skip_data_lines.max(0)) as usize;
        if self.line < first_data || self.line == 0 || !self.o.store_blank_rows {
            let fields: Vec<std::borrow::Cow<str>> = rec.iter().map(String::from_utf8_lossy).collect();
            return self.push(fields.iter().map(|s| Field::Text(s)));
        }
        if self.o.limit >= 0 && self.rows as i64 >= self.o.limit {
            return false;
        }
        self.line += 1;
        for (c, f) in rec.iter().enumerate() {
            if c == self.cols.len() {
                let mut b = Interner::default();
                let null = b.intern(Value::Null);
                self.cols.push((b, vec![null; self.rows]));
            }
            let (b, codes) = &mut self.cols[c];
            codes.push(code(self.o, b, Field::Text(&String::from_utf8_lossy(f))));
        }
        self.pad(rec.len());
        true
    }

    /// Fill the columns a short row didn't reach, then count the row.
    fn pad(&mut self, n: usize) {
        let from = n.min(self.cols.len());
        for (b, codes) in &mut self.cols[from..] {
            codes.push(b.intern(Value::Null));
        }
        self.rows += 1;
    }

    fn finish(mut self) -> Table {
        let width = self.cols.len().max(self.header.iter().map(Vec::len).max().unwrap_or(0));
        while self.cols.len() < width {
            let mut b = Interner::default();
            let null = b.intern(Value::Null);
            self.cols.push((b, vec![null; self.rows]));
        }
        let names: Vec<String> = (0..width)
            .map(|c| self.header.iter().filter_map(|h| h.get(c)).filter(|s| !s.is_empty()).cloned().collect::<Vec<_>>().join(" "))
            .collect();
        let rows = self.rows;
        Table::new(dedupe(names), self.cols.into_iter().map(|(b, codes)| Column { dict: b.finish(), codes }).collect(), rows)
    }
}

/// Dictionary code for one imported field, applying trim, blank-cell and type-guessing options.
fn code(o: &ImportOptions, b: &mut Interner, f: Field) -> u32 {
    match f {
        Field::Val(v) => b.intern(v),
        Field::Text(f) => {
            let s = if o.trim_strings { crate::grel::text::java_strip(f) } else { f };
            if s.is_empty() {
                return if o.store_blank_cells_as_nulls { b.intern(Value::Null) } else { b.intern_str("") };
            }
            if o.guess_cell_value_types {
                if let Some(n) = parse_number(s) {
                    return b.intern(n);
                }
            }
            b.intern_str(s)
        }
    }
}

/// Header names: blank ones become "Column N", repeats get " 2", " 3"...
pub fn dedupe(names: Vec<String>) -> Vec<String> {
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

// ---------------------------------------------------------------- delimited text

fn read_head(path: &Path) -> Result<Vec<u8>> {
    let mut head = vec![0u8; 1 << 16];
    let n = File::open(path).and_then(|mut f| f.read(&mut head)).map_err(|e| format!("{}: {e}", path.display()))?;
    head.truncate(n);
    Ok(head)
}

/// Most frequent of , ; tab | in the first line (ties go to ','); ',' when there is none.
pub fn sniff_separator(path: &Path) -> Result<char> {
    Ok(guess_separator(path)?.unwrap_or(','))
}

/// Like `sniff_separator`, but None when the first line has none of them.
fn guess_separator(path: &Path) -> Result<Option<char>> {
    if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("tsv") || e.eq_ignore_ascii_case("tab")) {
        return Ok(Some('\t'));
    }
    let head = read_head(path)?;
    let first = head.split(|&b| b == b'\n').next().unwrap_or_default();
    let count = |d: &u8| first.iter().filter(|b| *b == d).count();
    let best = [b'|', b';', b'\t', b','].into_iter().max_by_key(count).unwrap();
    Ok((count(&best) > 0).then_some(best as char))
}

/// OpenRefine's line-based importer: every `linesPerRow` lines make one row.
fn import_lines(path: &Path, o: &ImportOptions) -> Result<Table> {
    let text = decode(path, &o.encoding)?;
    let per = o.lines_per_row.max(1) as usize;
    let lines: Vec<&str> = text.lines().collect();
    let mut b = Builder::new(o);
    for chunk in lines.chunks(per) {
        if !b.push(chunk.iter().map(|s| Field::Text(s))) {
            break;
        }
    }
    Ok(b.finish())
}

fn is_utf8(label: &str) -> bool {
    label.is_empty() || label.eq_ignore_ascii_case("utf-8") || label.eq_ignore_ascii_case("utf8")
}

fn import_text(path: &Path, o: &ImportOptions) -> Result<Table> {
    let sep = match &o.separator {
        Some(s) if s == "\\t" => '\t',
        Some(s) if !s.is_empty() => {
            let mut c = s.chars();
            let first = c.next().unwrap();
            if c.next().is_some() {
                return Err(format!("multi-character separator {s:?} is not supported yet"));
            }
            first
        }
        _ => sniff_separator(path)?,
    };
    let quote = o.quote_character.chars().next().unwrap_or('"');
    let mut b = Builder::new(o);
    if !o.process_quotes {
        let text = decode(path, &o.encoding)?;
        loose_records(&text, sep, quote, |rec| b.push(rec.iter().map(|s| Field::Text(s))));
        return Ok(b.finish());
    }
    if !sep.is_ascii() || !quote.is_ascii() {
        return Err("separator and quote character must be ASCII".into());
    }
    let builder = || {
        let mut r = csv::ReaderBuilder::new();
        r.delimiter(sep as u8).quote(quote as u8).flexible(true).has_headers(false);
        r
    };
    let mut rec = csv::ByteRecord::new();
    let mut feed = |rdr: &mut csv::Reader<Box<dyn Read>>| -> Result<()> {
        while rdr.read_byte_record(&mut rec).map_err(err)? {
            if !b.push_bytes(&rec) {
                break;
            }
        }
        Ok(())
    };
    if is_utf8(&o.encoding) {
        let f = File::open(path).map_err(err)?;
        crate::jobs::progress(&format!("Reading {}", path.file_name().map_or(String::new(), |n| n.to_string_lossy().into_owned())), f.metadata().map_or(0, |m| m.len()));
        let file: Box<dyn Read> = Box::new(std::io::BufReader::with_capacity(1 << 20, crate::jobs::Counted(f)));
        feed(&mut builder().from_reader(file))?;
    } else {
        let text = decode(path, &o.encoding)?;
        let bytes: Box<dyn Read> = Box::new(std::io::Cursor::new(text.into_bytes()));
        feed(&mut builder().from_reader(bytes))?;
    }
    Ok(b.finish())
}

fn decode(path: &Path, label: &str) -> Result<String> {
    let mut bytes = vec![];
    let f = File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    crate::jobs::progress(&format!("Reading {}", path.file_name().map_or(String::new(), |n| n.to_string_lossy().into_owned())), f.metadata().map_or(0, |m| m.len()));
    crate::jobs::Counted(f).read_to_end(&mut bytes).map_err(|e| if crate::jobs::is_cancel(&e) { crate::jobs::CANCELLED.to_string() } else { format!("{}: {e}", path.display()) })?;
    if is_utf8(label) {
        return Ok(String::from_utf8_lossy(&bytes).into_owned());
    }
    let enc = encoding_rs::Encoding::for_label(label.as_bytes()).ok_or_else(|| format!("unknown encoding {label}"))?;
    Ok(enc.decode(&bytes).0.into_owned())
}

/// OpenRefine's "don't process quotes": quotes still group a field, but they stay in the value.
fn loose_records(text: &str, sep: char, quote: char, mut emit: impl FnMut(&[String]) -> bool) {
    let mut rec: Vec<String> = vec![];
    let mut cur = String::new();
    let (mut quoted, mut at_start) = (false, true);
    let mut it = text.chars().peekable();
    let flush = |rec: &mut Vec<String>, cur: &mut String, emit: &mut dyn FnMut(&[String]) -> bool| -> bool {
        rec.push(std::mem::take(cur));
        let blank = rec.len() == 1 && rec[0].is_empty();
        let go = blank || emit(rec);
        rec.clear();
        go
    };
    while let Some(c) = it.next() {
        if quoted {
            if c == quote {
                if it.peek() == Some(&quote) {
                    it.next();
                }
                else {
                    quoted = false;
                }
                cur.push(c);
            } else {
                cur.push(c);
            }
            continue;
        }
        match c {
            _ if c == quote && at_start => {
                quoted = true;
                at_start = false;
                cur.push(c);
            }
            _ if c == sep => {
                rec.push(std::mem::take(&mut cur));
                at_start = true;
            }
            '\r' if it.peek() == Some(&'\n') => {}
            '\n' => {
                if !flush(&mut rec, &mut cur, &mut emit) {
                    return;
                }
                at_start = true;
            }
            _ => {
                cur.push(c);
                at_start = false;
            }
        }
    }
    if !cur.is_empty() || !rec.is_empty() {
        flush(&mut rec, &mut cur, &mut emit);
    }
}

// ---------------------------------------------------------------- spreadsheets

fn sheet_value(d: &calamine::Data, ods: bool) -> Value {
    use calamine::Data as D;
    match d {
        D::Empty => Value::Null,
        D::String(s) if s.is_empty() => Value::Null,
        D::String(s) => Value::Str(s.clone()),
        D::Int(i) => Value::Int(*i),
        D::Float(f) if f.fract() == 0.0 && f.abs() < 9.007_199_254_740_992e15 => Value::Int(*f as i64),
        D::Float(f) => Value::Num(*f),
        // OpenRefine reads spreadsheet booleans as 1 / 0
        D::Bool(b) => Value::Int(*b as i64),
        D::DateTime(dt) if dt.is_duration() => Value::Num(dt.as_f64()),
        D::DateTime(dt) => match dt.as_datetime() {
            Some(t) if ods => Value::Date(t),
            // Excel dates arrive as the date text OpenRefine shows ("2024-01-15")
            Some(t) if t.time() == chrono::NaiveTime::MIN => Value::Str(t.format("%Y-%m-%d").to_string()),
            Some(t) => Value::Str(t.format("%Y-%m-%d %H:%M:%S").to_string()),
            None => Value::Num(dt.as_f64()),
        },
        D::DateTimeIso(s) => match crate::grel::dates::parse_default(s) {
            Some(t) if ods => Value::Date(t),
            _ => Value::Str(s.clone()),
        },
        D::DurationIso(s) => Value::Str(s.clone()),
        D::Error(e) => Value::Error(format!("{e:?}")),
    }
}

fn import_sheets(path: &Path, o: &ImportOptions) -> Result<Table> {
    use calamine::Reader;
    let ods = path.extension().is_some_and(|e| e.eq_ignore_ascii_case("ods"));
    let mut wb = calamine::open_workbook_auto(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let names = wb.sheet_names();
    let mut picks: Vec<usize> = o
        .sheets
        .iter()
        .filter(|s| s.selected)
        .filter_map(|s| s.file_name_and_sheet_index.rsplit('#').next()?.parse().ok().or_else(|| names.iter().position(|n| *n == s.name)))
        .collect();
    if picks.is_empty() {
        picks.push(0);
    }
    let mut tables = vec![];
    for i in picks {
        let name = names.get(i).ok_or_else(|| format!("no sheet number {i}"))?.clone();
        let range = wb.worksheet_range(&name).map_err(err)?;
        let (r0, c0) = range.start().map_or((0, 0), |(r, c)| (r as usize, c as usize));
        let mut b = Builder::new(o);
        let mut go = true;
        for _ in 0..r0 {
            go = go && b.push(std::iter::empty());
        }
        for row in range.rows() {
            if !go {
                break;
            }
            let lead = std::iter::repeat_n(Value::Null, c0);
            let mut vals: Vec<Value> = lead.chain(row.iter().map(|d| sheet_value(d, ods))).collect();
            while vals.last().is_some_and(Value::is_null) {
                vals.pop();
            }
            go = b.push(vals.into_iter().map(Field::Val));
        }
        tables.push(b.finish());
    }
    Ok(concat_by_name(tables))
}

/// Rows of several tables, one after another, with columns matched by name.
fn concat_by_name(tables: Vec<Table>) -> Table {
    if tables.len() == 1 {
        return tables.into_iter().next().unwrap();
    }
    let mut names: Vec<String> = vec![];
    for t in &tables {
        for n in &t.names {
            if !names.contains(n) {
                names.push(n.clone());
            }
        }
    }
    let rows: usize = tables.iter().map(|t| t.rows).sum();
    let cols = names
        .iter()
        .map(|n| {
            Column::from_values(tables.iter().flat_map(|t| {
                let ci = t.names.iter().position(|x| x == n);
                (0..t.rows).map(move |r| ci.map_or(Value::Null, |c| t.columns[c].get(r).clone()))
            }))
        })
        .collect();
    Table::new(names, cols, rows)
}

// ---------------------------------------------------------------- JSON

#[derive(Default)]
struct Group {
    /// (local name, full column name, values seen) in discovery order
    columns: Vec<(String, String, usize)>,
    /// (local name, group, records it appeared in)
    groups: Vec<(String, Group, usize)>,
}

impl Group {
    fn column(&mut self, local: &str, full: &str) {
        match self.columns.iter_mut().find(|c| c.0 == local) {
            Some(c) => c.2 += 1,
            None => self.columns.push((local.to_string(), full.to_string(), 1)),
        }
    }
    fn group(&mut self, local: &str) -> &mut Group {
        let i = match self.groups.iter().position(|g| g.0 == local) {
            Some(i) => i,
            None => {
                self.groups.push((local.to_string(), Group::default(), 0));
                self.groups.len() - 1
            }
        };
        self.groups[i].2 += 1;
        &mut self.groups[i].1
    }
    /// OpenRefine's column order: a group's own columns, then its subgroups, each by how often they were filled.
    fn order(&self, out: &mut Vec<String>) {
        let mut cols: Vec<_> = self.columns.iter().collect();
        cols.sort_by_key(|c| std::cmp::Reverse(c.2));
        out.extend(cols.into_iter().map(|c| c.1.clone()));
        let mut groups: Vec<_> = self.groups.iter().collect();
        groups.sort_by_key(|g| std::cmp::Reverse(g.2));
        for g in groups {
            g.1.order(out);
        }
    }
}

struct JsonRecord {
    rows: Vec<Vec<(String, Value)>>,
}

impl JsonRecord {
    /// A value goes on the first row of the record that has nothing in its column yet.
    fn place(&mut self, column: &str, v: Value) {
        match self.rows.iter_mut().find(|r| !r.iter().any(|(c, _)| c == column)) {
            Some(r) => r.push((column.to_string(), v)),
            None => self.rows.push(vec![(column.to_string(), v)]),
        }
    }
}

fn json_scalar(v: &serde_json::Value, o: &ImportOptions) -> Value {
    match v {
        serde_json::Value::String(s) => {
            let s = if o.trim_strings { crate::grel::text::java_strip(s) } else { s.as_str() };
            if s.is_empty() && !o.store_empty_strings {
                return Value::Null;
            }
            if o.guess_cell_value_types {
                if let Some(n) = parse_number(s) {
                    return n;
                }
            }
            Value::Str(s.to_string())
        }
        other => from_json(other.clone()),
    }
}

fn walk_json(node: &serde_json::Value, prefix: &str, group: &mut Group, rec: &mut JsonRecord, o: &ImportOptions) {
    use serde_json::Value as J;
    match node {
        J::Object(m) => {
            for (k, v) in m {
                let full = format!("{prefix} - {k}");
                match v {
                    J::Object(_) => walk_json(v, &full, group.group(k), rec, o),
                    J::Array(items) => {
                        let sub = group.group(k);
                        for item in items {
                            match item {
                                J::Object(_) | J::Array(_) => walk_json(item, &full, sub, rec, o),
                                scalar => {
                                    let col = format!("{full} - {k}");
                                    sub.column(k, &col);
                                    rec.place(&col, json_scalar(scalar, o));
                                }
                            }
                        }
                    }
                    scalar => {
                        group.column(k, &full);
                        rec.place(&full, json_scalar(scalar, o));
                    }
                }
            }
        }
        J::Array(items) => {
            for item in items {
                walk_json(item, prefix, group, rec, o);
            }
        }
        scalar => {
            group.column(prefix, prefix);
            rec.place(prefix, json_scalar(scalar, o));
        }
    }
}

fn import_json(path: &Path, lines: bool, o: &ImportOptions) -> Result<Table> {
    let text = decode(path, &o.encoding)?;
    let root: serde_json::Value = if lines {
        serde_json::Value::Array(text.lines().filter(|l| !l.trim().is_empty()).map(serde_json::from_str).collect::<std::result::Result<_, _>>().map_err(err)?)
    } else {
        serde_json::from_str(text.trim_start_matches('\u{feff}')).map_err(|e| format!("{}: {e}", path.display()))?
    };
    // the first path element names the root; "_" steps into array elements, anything else into a field
    let mut nodes = vec![&root];
    for seg in o.record_path.iter().skip(1) {
        nodes = nodes
            .into_iter()
            .flat_map(|n| -> Vec<&serde_json::Value> {
                match (seg.as_str(), n) {
                    ("_", serde_json::Value::Array(a)) => a.iter().collect(),
                    (k, serde_json::Value::Object(m)) => m.get(k).into_iter().collect(),
                    (k, serde_json::Value::Array(a)) => a.iter().filter_map(|x| x.get(k)).collect(),
                    _ => vec![],
                }
            })
            .collect();
    }
    let record_name = o.record_path.last().cloned().unwrap_or_else(|| "_".into());
    let mut group = Group::default();
    let mut records = vec![];
    for node in nodes.into_iter().take(if o.limit >= 0 { o.limit as usize } else { usize::MAX }) {
        let mut rec = JsonRecord { rows: vec![vec![]] };
        walk_json(node, &record_name, &mut group, &mut rec, o);
        records.push(rec);
    }
    let mut names = vec![];
    group.order(&mut names);
    let rows: Vec<&Vec<(String, Value)>> = records.iter().flat_map(|r| r.rows.iter()).collect();
    let cols = names
        .iter()
        .map(|n| Column::from_values(rows.iter().map(|r| r.iter().find(|(c, _)| c == n).map_or(Value::Null, |(_, v)| v.clone()))))
        .collect();
    Ok(Table::new(names, cols, rows.len()))
}

impl Table {
    /// A fresh table: no stars or flags, records keyed by the first column.
    pub fn new(names: Vec<String>, columns: Vec<Column>, rows: usize) -> Table {
        Table {
            key: names.first().cloned(),
            names,
            columns: columns.into_iter().map(Arc::new).collect(),
            rows,
            starred: Arc::new(vec![false; rows]),
            flagged: Arc::new(vec![false; rows]),
        }
    }
}
