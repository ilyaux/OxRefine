//! Importers with OpenRefine's options: delimited text, plain lines, fixed-width text, Excel/ODS,
//! JSON and XML, from a file, the files inside an archive or a folder, the clipboard or a web
//! address (the last two staged as files, then imported like any other file).
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
    /// Fixed-width text: the width of each column, in characters; the rest of a line is one more column.
    pub column_widths: Vec<usize>,
    /// A "File" column with each row's file name (inside the archive, or the file itself).
    pub include_file_sources: bool,
    /// An "Archive" column with the archive's file name.
    pub include_archive_file_name: bool,
    /// Archives and folders: the files to import, by name; empty picks them as OpenRefine does.
    pub files: Vec<String>,
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
            column_widths: vec![],
            include_file_sources: false,
            include_archive_file_name: false,
            files: vec![],
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
    Xml,
    #[serde(rename = "fixed-width")]
    FixedWidth,
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
        "text/xml" | "xml" => return Format::Xml,
        "text/line-based/fixed-width" | "fixed-width" => return Format::FixedWidth,
        _ => {}
    }
    let ext = path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    match ext.as_str() {
        "xlsx" | "xlsm" | "xlsb" | "xls" => Format::Excel,
        "ods" => Format::Ods,
        "json" => Format::Json,
        "jsonl" | "ndjson" => Format::JsonLines,
        "xml" | "rss" | "atom" | "kml" | "gpx" => Format::Xml,
        _ => Format::Csv,
    }
}

/// Import a file, or the files inside an archive or a folder (one after another, columns matched by
/// name), with OpenRefine's options. `format` (an OpenRefine format id or our short name) applies to
/// every file; None picks it from the file name.
pub fn import(path: &Path, format: Option<&str>, o: &ImportOptions) -> Result<Table> {
    crate::jobs::progress(&format!("Reading {}", path.file_name().map_or(String::new(), |n| n.to_string_lossy().into_owned())), 0);
    let u = crate::unpack::unpack(path)?;
    if !u.several() && u.files[0].1 == path && !o.include_file_sources {
        return import_file(path, format_for(path, format), o);
    }
    let (picked, best) = pick(&u, o)?;
    if picked.is_empty() {
        return Err(format!("{}: no files to import", path.display()));
    }
    let f = format.map(|_| format_for(path, format));
    let tables = picked
        .into_iter()
        .map(|(name, p)| Ok(with_sources(import_file(p, f.unwrap_or(if u.several() { best } else { format_for(p, None) }), o)?, name, u.archive.as_deref(), o)))
        .collect::<Result<Vec<_>>>()?;
    Ok(concat_by_name(tables))
}

fn import_file(path: &Path, f: Format, o: &ImportOptions) -> Result<Table> {
    match f {
        Format::Csv => import_text(path, o, PARALLEL_FROM),
        Format::Lines => import_lines(path, o),
        Format::FixedWidth => import_fixed(path, o),
        Format::Excel | Format::Ods => import_sheets(path, o),
        Format::Json | Format::JsonLines => import_json(path, f == Format::JsonLines, o),
        Format::Xml => import_xml(path, o),
    }
}

/// The format the wizard would guess for one file: by extension, except that text whose first line
/// has no separator is plain lines (unless its name says CSV/TSV).
fn guess_format(path: &Path) -> Result<Format> {
    let f = format_for(path, None);
    let declared = path.extension().is_some_and(|e| ["csv", "tsv", "tab"].iter().any(|x| e.eq_ignore_ascii_case(x)));
    Ok(if f == Format::Csv && !declared && guess_separator(path)?.is_none() { Format::Lines } else { f })
}

/// The files to import from an archive or folder: those named in `files`, else, like OpenRefine,
/// every file of the most common format (files OxRefine can't read are left out). Also that format.
fn pick<'u>(u: &'u crate::unpack::Unpacked, o: &ImportOptions) -> Result<(Vec<(&'u str, &'u Path)>, Format)> {
    let guesses: Vec<Option<Format>> = u.files.iter().map(|(_, p)| guess_format(p).ok()).collect();
    let mut counts: Vec<(Format, usize)> = vec![];
    for g in guesses.iter().flatten() {
        match counts.iter_mut().find(|c| c.0 == *g) {
            Some(c) => c.1 += 1,
            None => counts.push((*g, 1)),
        }
    }
    // the most common, the first found on a tie
    let best = counts.iter().fold(None::<(Format, usize)>, |b, &c| if b.is_some_and(|b| b.1 >= c.1) { b } else { Some(c) }).map_or(Format::Csv, |b| b.0);
    let chosen = u.files.iter().zip(&guesses).filter(|((name, _), g)| if o.files.is_empty() { **g == Some(best) } else { o.files.contains(name) });
    Ok((chosen.map(|((n, p), _)| (n.as_str(), p.as_path())).collect(), best))
}

/// OpenRefine's "Archive" and "File" columns, in front.
fn with_sources(t: Table, file: &str, archive: Option<&str>, o: &ImportOptions) -> Table {
    let mut names = vec![];
    let mut cols = vec![];
    let constant = |v: &str| Column { dict: vec![Value::Str(v.into())], codes: vec![0; t.rows].into(), recon: None };
    if let (true, Some(a)) = (o.include_archive_file_name, archive) {
        names.push("Archive".to_string());
        cols.push(constant(a));
    }
    if o.include_file_sources {
        names.push("File".to_string());
        cols.push(constant(file));
    }
    if names.is_empty() {
        return t;
    }
    names.extend(t.names.iter().cloned());
    cols.extend(t.columns.iter().map(|c| Column { dict: c.dict.clone(), codes: c.codes.clone(), recon: None }));
    Table::new(names, cols, t.rows)
}

/// What the import wizard needs before options are chosen: the guessed format and OpenRefine's
/// wizard defaults for it (`options`). Text with no separator in its first line is line-based,
/// unless the file says it is CSV/TSV.
pub fn inspect(path: &Path) -> Result<serde_json::Value> {
    use serde_json::json;
    if crate::orproject::is_archive(path) {
        return Ok(json!({ "format": "openrefine-project" }));
    }
    let u = crate::unpack::unpack(path)?;
    if !u.several() {
        let mut out = inspect_file(&u.files[0].1)?;
        if let Some(o) = out["options"].as_object_mut() {
            o.insert("includeFileSources".into(), json!(false));
            o.insert("includeArchiveFileName".into(), json!(false));
        }
        return Ok(out);
    }
    // an archive or folder: the wizard's guesses for its first picked file, plus the list to pick from
    let (picked, _) = pick(&u, &ImportOptions::default())?;
    let first = picked.first().map_or(u.files[0].1.as_path(), |p| p.1);
    let mut out = inspect_file(first)?;
    if let Some(o) = out["options"].as_object_mut() {
        o.insert("includeFileSources".into(), json!(true));
        o.insert("includeArchiveFileName".into(), json!(u.archive.is_some()));
    }
    out["files"] = json!(u.files.iter().map(|(n, p)| json!({
        "name": n,
        "size": std::fs::metadata(p).map_or(0, |m| m.len()),
        "format": guess_format(p).ok(),
        "selected": picked.iter().any(|x| x.0 == n),
    })).collect::<Vec<_>>());
    out["archive"] = json!(u.archive);
    Ok(out)
}

fn inspect_file(path: &Path) -> Result<serde_json::Value> {
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
    let encoding = guess_encoding(&read_head(path)?);
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
            merge(json!({ "separator": sep.to_string(), "encoding": encoding, "processQuotes": sep != '\t', "quoteCharacter": "\"", "trimStrings": true, "guessCellValueTypes": false }))
        }
        Format::Lines => merge(json!({ "headerLines": 0, "linesPerRow": 1, "encoding": encoding, "guessCellValueTypes": false })),
        Format::Json | Format::JsonLines => json!({ "recordPath": ["_", "_"], "limit": -1, "encoding": encoding, "trimStrings": false, "guessCellValueTypes": false, "storeEmptyStrings": true }),
        // OpenRefine leaves the record element to the user; OxRefine suggests the first repeated one
        Format::Xml => {
            let paths = xml_paths(&parse_xml(&decode(path, encoding)?)?);
            let guess = paths.iter().find(|(_, n)| *n > 1).or(paths.first()).map(|(p, _)| p.clone()).unwrap_or_default();
            out["elements"] = json!(paths.iter().take(500).map(|(p, n)| json!({ "path": p, "count": n })).collect::<Vec<_>>());
            json!({ "recordPath": guess, "limit": -1, "encoding": encoding, "trimStrings": false, "guessCellValueTypes": false, "storeEmptyStrings": true })
        }
        Format::FixedWidth => merge(json!({ "headerLines": 0, "columnWidths": [], "encoding": encoding, "guessCellValueTypes": false })),
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

/// A path for a file named `name` in a fresh staging folder (an upload, or a file to download).
pub fn stage_path(dir: &Path, name: &str) -> Result<std::path::PathBuf> {
    let base = name.rsplit(['/', '\\']).next().unwrap_or("");
    let clean: String = base.chars().map(|c| if "\\/:*?\"<>|".contains(c) || c.is_control() { '_' } else { c }).collect();
    let clean = if clean.trim_matches('.').is_empty() { "file".to_string() } else { clean };
    Ok(stage_folder(dir)?.join(clean))
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
        "text/xml" | "application/xml" | "application/rss+xml" | "application/atom+xml" => ".xml",
        "application/zip" | "application/x-zip-compressed" => ".zip",
        "application/gzip" | "application/x-gzip" => ".gz",
        "application/x-bzip2" | "application/x-bzip" => ".bz2",
        "application/x-tar" => ".tar",
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
        let named = self.header.iter().map(Vec::len).max().unwrap_or(0);
        let rows = self.rows;
        Table::new(column_names(names, named), self.cols.into_iter().map(|(b, codes)| Column { dict: b.finish(), codes: codes.into(), recon: None }).collect(), rows)
    }
}

/// Dictionary code for one imported field, applying trim, blank-cell and type-guessing options.
fn code(o: &ImportOptions, b: &mut Interner, f: Field) -> u32 {
    match f {
        Field::Val(v) => b.intern(v),
        Field::Text(f) => {
            // importers strip all Unicode whitespace (non-breaking and em spaces too), unlike GREL's trim()
            let s = if o.trim_strings { f.trim() } else { f };
            if s.is_empty() {
                return if o.store_blank_cells_as_nulls { b.intern(Value::Null) } else { b.intern_str("") };
            }
            // OpenRefine's importers guess integers and finite decimals; "NaN" and "Infinity" stay text
            if o.guess_cell_value_types {
                if let Some(n) = parse_number(s).filter(|n| !matches!(n, Value::Num(x) if !x.is_finite())) {
                    return b.intern(n);
                }
            }
            b.intern_str(s)
        }
    }
}

/// OpenRefine's column names. The first `named` come from the header: trimmed, a blank one is
/// "Column", and a repeat gets its count appended ("name2", or "first name 2" when the name has a
/// space). Columns past the header are "Column N" by position.
pub fn column_names(names: Vec<String>, named: usize) -> Vec<String> {
    let mut counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let mut taken = HashSet::new();
    let mut out = Vec::with_capacity(names.len());
    for (i, n) in names.into_iter().enumerate() {
        let name = if i < named {
            let n = n.trim_matches(|c: char| c <= ' ');
            let n = n.strip_prefix('"').and_then(|s| s.strip_suffix('"')).map_or(n, str::trim);
            let n = if n.is_empty() { "Column" } else { n };
            match counts.get_mut(n) {
                Some(k) => {
                    let name = if n.contains(' ') { format!("{n} {k}") } else { format!("{n}{k}") };
                    *k += 1;
                    name
                }
                None => {
                    counts.insert(n.to_string(), 2);
                    n.to_string()
                }
            }
        } else {
            let mut k = i + 1;
            while taken.contains(&format!("Column {k}")) {
                k += 1;
            }
            format!("Column {k}")
        };
        taken.insert(name.clone());
        out.push(name);
    }
    out
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
    let text = plain_breaks(&text);
    let lines: Vec<&str> = text.lines().collect();
    let mut b = Builder::new(o);
    for chunk in lines.chunks(per) {
        if !b.push(chunk.iter().map(|s| Field::Text(s))) {
            break;
        }
    }
    Ok(b.finish())
}

/// OpenRefine's fixed-width importer: each line cut at the column widths (in characters, cells kept
/// as they are, spaces included); whatever is left past the last width is one more cell.
fn import_fixed(path: &Path, o: &ImportOptions) -> Result<Table> {
    let text = decode(path, &o.encoding)?;
    let text = plain_breaks(&text);
    let mut b = Builder::new(o);
    for line in text.lines() {
        let chars: Vec<char> = line.chars().collect();
        let mut cells: Vec<String> = vec![];
        let mut at = 0;
        for &w in &o.column_widths {
            cells.push(chars[at.min(chars.len())..(at + w).min(chars.len())].iter().collect());
            at += w;
        }
        if at < chars.len() {
            cells.push(chars[at..].iter().collect());
        }
        if !b.push(cells.iter().map(|s| Field::Text(s))) {
            break;
        }
    }
    Ok(b.finish())
}

fn is_utf8(label: &str) -> bool {
    label.is_empty() || label.eq_ignore_ascii_case("utf-8") || label.eq_ignore_ascii_case("utf8")
}

/// OpenRefine's guess: a byte order mark, else UTF-8 when the bytes are valid UTF-8, else Windows-1252.
fn guess_encoding(head: &[u8]) -> &'static str {
    match head {
        [0xEF, 0xBB, 0xBF, ..] => "UTF-8",
        [0xFF, 0xFE, ..] => "UTF-16LE",
        [0xFE, 0xFF, ..] => "UTF-16BE",
        // a multi-byte character cut off at the end of the sample still counts as valid
        _ => match std::str::from_utf8(head) {
            Ok(_) => "UTF-8",
            Err(e) if e.error_len().is_none() => "UTF-8",
            Err(_) => "windows-1252",
        },
    }
}

/// How OpenRefine splits lines: by the file's first line break. After "\n" or "\r\n", a lone "\r" is
/// data; after a lone "\r", every "\r" is a break. Line breaks inside quoted fields come out as "\n".
fn cr_breaks(head: &[u8]) -> bool {
    head.iter().position(|&b| b == b'\r' || b == b'\n').is_some_and(|i| head[i] == b'\r' && head.get(i + 1) != Some(&b'\n'))
}

/// Text with OpenRefine's line breaks made plain "\n" (see `cr_breaks`).
fn plain_breaks(text: &str) -> std::borrow::Cow<'_, str> {
    if !text.contains('\r') {
        text.into()
    } else if cr_breaks(text.as_bytes()) {
        text.replace("\r\n", "\n").replace('\r', "\n").into()
    } else {
        text.replace("\r\n", "\n").into()
    }
}

/// `plain_breaks` as a stream, for files read without decoding them first.
struct PlainBreaks<R> {
    inner: R,
    cr: bool,
    /// the last chunk ended in "\r"; the next byte decides what it was
    carry: bool,
    chunk: Vec<u8>,
    out: Vec<u8>,
    pos: usize,
}

impl<R: Read> Read for PlainBreaks<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        while self.pos == self.out.len() {
            self.out.clear();
            self.pos = 0;
            let n = self.inner.read(&mut self.chunk)?;
            if n == 0 {
                if !std::mem::take(&mut self.carry) {
                    return Ok(0);
                }
                self.out.push(if self.cr { b'\n' } else { b'\r' });
                break;
            }
            let mut rest = &self.chunk[..n];
            if std::mem::take(&mut self.carry) {
                if rest[0] == b'\n' {
                    rest = &rest[1..];
                    self.out.push(b'\n');
                } else {
                    self.out.push(if self.cr { b'\n' } else { b'\r' });
                }
            }
            // copy the runs between "\r"s whole
            while let Some(i) = rest.iter().position(|&b| b == b'\r') {
                self.out.extend_from_slice(&rest[..i]);
                match rest.get(i + 1) {
                    Some(b'\n') => {
                        self.out.push(b'\n');
                        rest = &rest[i + 2..];
                    }
                    Some(_) => {
                        self.out.push(if self.cr { b'\n' } else { b'\r' });
                        rest = &rest[i + 1..];
                    }
                    None => {
                        self.carry = true;
                        rest = &[];
                    }
                }
            }
            self.out.extend_from_slice(rest);
        }
        let k = (self.out.len() - self.pos).min(buf.len());
        buf[..k].copy_from_slice(&self.out[self.pos..self.pos + k]);
        self.pos += k;
        Ok(k)
    }
}

/// Files at least this big intern their data rows on all cores (`feed_parallel`); smaller ones
/// aren't worth the threads.
const PARALLEL_FROM: u64 = 8 << 20;
const BATCH_ROWS: usize = 16_384;

/// Data rows copied out of the parser for the column workers: field bytes back to back.
#[derive(Default)]
struct Batch {
    data: Vec<u8>,
    /// end of each field in `data`
    ends: Vec<usize>,
    /// end of each row in `ends`
    rows: Vec<usize>,
}

impl Batch {
    fn push(&mut self, rec: &csv::ByteRecord) {
        for f in rec {
            self.data.extend_from_slice(f);
            self.ends.push(self.data.len());
        }
        self.rows.push(self.ends.len());
    }
    /// Fields of row `r`, as indexes into `ends`.
    fn row(&self, r: usize) -> std::ops::Range<usize> {
        let start = if r == 0 { 0 } else { self.rows[r - 1] };
        start..self.rows[r]
    }
    fn field(&self, i: usize) -> &[u8] {
        &self.data[if i == 0 { 0 } else { self.ends[i - 1] }..self.ends[i]]
    }
}

/// The data phase of a big delimited file on all cores. This thread parses and applies the row
/// rules (limit, blank rows); worker `w` owns the columns `c` with `c % workers == w` and walks every
/// row in order, so each column's dictionary comes out exactly as one thread would build it.
/// `rec` already holds the first data record.
fn feed_parallel<R: Read>(b: &mut Builder, rdr: &mut csv::Reader<R>, rec: &mut csv::ByteRecord) -> Result<()> {
    let workers = std::thread::available_parallelism().map_or(4, |n| n.get()).clamp(1, 16);
    let mut owned: Vec<Vec<(usize, Interner, Vec<u32>)>> = (0..workers).map(|_| vec![]).collect();
    for (c, (i, codes)) in std::mem::take(&mut b.cols).into_iter().enumerate() {
        owned[c % workers].push((c, i, codes));
    }
    let (o, start) = (b.o, b.rows);
    let mut sent = 0usize;
    let result = std::thread::scope(|s| -> Result<Vec<Vec<(usize, Interner, Vec<u32>)>>> {
        let mut txs = vec![];
        let mut handles = vec![];
        for (w, mut cols) in owned.into_iter().enumerate() {
            let (tx, rx) = std::sync::mpsc::sync_channel::<Arc<Batch>>(4);
            txs.push(tx);
            handles.push(s.spawn(move || {
                let mut rows = start;
                for batch in rx {
                    for r in 0..batch.rows.len() {
                        let fields = batch.row(r);
                        let n = fields.len();
                        // columns this row reaches for the first time start with nulls for the rows before
                        let mut c = cols.last().map_or(w, |x| x.0 + workers);
                        while c < n {
                            let mut i = Interner::default();
                            let null = i.intern(Value::Null);
                            cols.push((c, i, vec![null; rows]));
                            c += workers;
                        }
                        for (c, i, codes) in cols.iter_mut() {
                            codes.push(if *c < n {
                                code(o, i, Field::Text(&String::from_utf8_lossy(batch.field(fields.start + *c))))
                            } else {
                                i.intern(Value::Null)
                            });
                        }
                        rows += 1;
                    }
                }
                cols
            }));
        }
        let mut batch = Batch::default();
        let mut parsed = Ok(());
        loop {
            if o.limit >= 0 && (start + sent) as i64 >= o.limit {
                break;
            }
            b.line += 1;
            let blank = !o.store_blank_rows && rec.iter().all(|f| String::from_utf8_lossy(f).trim().is_empty());
            if !blank {
                batch.push(rec);
                sent += 1;
                if batch.rows.len() == BATCH_ROWS {
                    let full = Arc::new(std::mem::take(&mut batch));
                    txs.iter().for_each(|tx| drop(tx.send(full.clone())));
                }
            }
            match rdr.read_byte_record(rec) {
                Ok(true) => {}
                Ok(false) => break,
                Err(e) => {
                    parsed = Err(err(e));
                    break;
                }
            }
        }
        let last = Arc::new(batch);
        txs.iter().for_each(|tx| drop(tx.send(last.clone())));
        drop(txs);
        let cols = handles.into_iter().map(|h| h.join().map_err(|_| "an import worker failed".to_string())).collect::<Result<Vec<_>>>()?;
        parsed.map(|_| cols)
    })?;
    let mut cols: Vec<(usize, Interner, Vec<u32>)> = result.into_iter().flatten().collect();
    cols.sort_by_key(|c| c.0);
    b.cols = cols.into_iter().map(|(_, i, codes)| (i, codes)).collect();
    b.rows = start + sent;
    Ok(())
}

/// Delimited text; files of `parallel_from` bytes or more use all cores for the data rows.
fn import_text(path: &Path, o: &ImportOptions, parallel_from: u64) -> Result<Table> {
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
        loose_records(&plain_breaks(&text), sep, quote, |rec| b.push(rec.iter().map(|s| Field::Text(s))));
        return Ok(b.finish());
    }
    if !sep.is_ascii() || !quote.is_ascii() {
        return Err("separator and quote character must be ASCII".into());
    }
    let builder = || {
        let mut r = csv::ReaderBuilder::new();
        // line breaks reach the parser as plain "\n" (see `plain_breaks`), so a lone "\r" stays data
        r.delimiter(sep as u8).quote(quote as u8).flexible(true).has_headers(false).terminator(csv::Terminator::Any(b'\n'));
        r
    };
    let mut rec = csv::ByteRecord::new();
    let first_data = (o.ignore_lines.max(0) + o.header_lines.max(0) + o.skip_data_lines.max(0)) as usize;
    let mut feed = |rdr: &mut csv::Reader<Box<dyn Read>>, parallel: bool| -> Result<()> {
        while rdr.read_byte_record(&mut rec).map_err(err)? {
            // header lines and the first row go through the builder; big files hand the rest to all cores
            if parallel && b.line >= first_data && b.line > 0 {
                return feed_parallel(&mut b, rdr, &mut rec);
            }
            if !b.push_bytes(&rec) {
                break;
            }
        }
        Ok(())
    };
    if is_utf8(&o.encoding) {
        let f = File::open(path).map_err(err)?;
        let parallel = f.metadata().is_ok_and(|m| m.len() >= parallel_from);
        crate::jobs::progress(&format!("Reading {}", path.file_name().map_or(String::new(), |n| n.to_string_lossy().into_owned())), f.metadata().map_or(0, |m| m.len()));
        let reader = std::io::BufReader::with_capacity(1 << 20, crate::jobs::Counted(f));
        // ponytail: line breaks are judged from the first 64 KB; a file whose first "\r" comes later keeps it as data
        let head = read_head(path)?;
        let file: Box<dyn Read> = if head.contains(&b'\r') {
            Box::new(PlainBreaks { inner: reader, cr: cr_breaks(&head), carry: false, chunk: vec![0; 1 << 16], out: vec![], pos: 0 })
        } else {
            Box::new(reader)
        };
        feed(&mut builder().from_reader(file), parallel)?;
    } else {
        let text = decode(path, &o.encoding)?;
        let parallel = text.len() as u64 >= parallel_from;
        let bytes: Box<dyn Read> = Box::new(std::io::Cursor::new(plain_breaks(&text).into_owned().into_bytes()));
        feed(&mut builder().from_reader(bytes), parallel)?;
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

/// OpenRefine's ImportColumnGroup: columns and subgroups of a JSON object or XML element, with the
/// row the next value goes on (`next`, per group and per column) while a record is read.
#[derive(Default)]
struct Group {
    /// (local name, full column name, cells filled, next row) in discovery order
    columns: Vec<(String, String, usize, usize)>,
    /// (local name, group) in discovery order
    groups: Vec<(String, Group)>,
    next: usize,
}

impl Group {
    fn column(&mut self, local: &str, full: &str) -> usize {
        match self.columns.iter().position(|c| c.0 == local) {
            Some(i) => {
                self.columns[i].2 += 1;
                i
            }
            None => {
                self.columns.push((local.to_string(), full.to_string(), 1, 0));
                self.columns.len() - 1
            }
        }
    }
    /// OpenRefine's addCell: the value goes on the later of the group's and the column's next row.
    fn add(&mut self, rec: &mut JsonRecord, local: &str, full: &str, v: Value) {
        let i = self.column(local, full);
        let row = self.next.max(self.columns[i].3);
        while rec.rows.len() <= row {
            rec.rows.push(vec![]);
        }
        rec.rows[row].push((full.to_string(), v));
        self.columns[i].3 = row + 1;
    }
    /// Start a sub-record (a nested object or element): it can't begin above where this group is.
    fn enter(&mut self, local: &str) -> &mut Group {
        let at = self.next;
        let g = self.group(local);
        g.next = g.next.max(at);
        g
    }
    /// End a sub-record: the next one starts below everything it filled.
    fn settle(&mut self) {
        self.next = self.columns.iter().map(|c| c.3).chain(self.groups.iter().map(|g| g.1.next)).fold(self.next, usize::max);
    }
    fn reset(&mut self) {
        self.next = 0;
        self.columns.iter_mut().for_each(|c| c.3 = 0);
        self.groups.iter_mut().for_each(|g| g.1.reset());
    }
    fn group(&mut self, local: &str) -> &mut Group {
        let i = match self.groups.iter().position(|g| g.0 == local) {
            Some(i) => i,
            None => {
                self.groups.push((local.to_string(), Group::default()));
                self.groups.len() - 1
            }
        };
        &mut self.groups[i].1
    }
    /// A group weighs as much as its fullest column or subgroup (OpenRefine's tabulate()).
    fn filled(&self) -> usize {
        self.columns.iter().map(|c| c.2).chain(self.groups.iter().map(|g| g.1.filled())).max().unwrap_or(0)
    }
    /// OpenRefine's column order: a group's own columns, then its subgroups, each sorted by cells
    /// filled, most first; ties keep the order they were found in.
    fn order(&self, out: &mut Vec<String>) {
        let mut cols: Vec<_> = self.columns.iter().collect();
        cols.sort_by_key(|c| std::cmp::Reverse(c.2));
        out.extend(cols.into_iter().map(|c| c.1.clone()));
        let mut groups: Vec<_> = self.groups.iter().collect();
        groups.sort_by_key(|g| std::cmp::Reverse(g.1.filled()));
        for g in groups {
            g.1.order(out);
        }
    }
}

struct JsonRecord {
    rows: Vec<Vec<(String, Value)>>,
}

/// A text value of a JSON or XML tree, with the trim, empty-string and type-guessing options.
fn tree_text(s: &str, o: &ImportOptions) -> Value {
    let s = if o.trim_strings { crate::grel::text::java_strip(s) } else { s };
    if s.is_empty() && !o.store_empty_strings {
        return Value::Null;
    }
    if o.guess_cell_value_types {
        if let Some(n) = parse_number(s).filter(|n| !matches!(n, Value::Num(x) if !x.is_finite())) {
            return n;
        }
    }
    Value::Str(s.to_string())
}

fn json_scalar(v: &serde_json::Value, o: &ImportOptions) -> Value {
    match v {
        serde_json::Value::String(s) => tree_text(s, o),
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
                    J::Object(_) => {
                        let g = group.enter(k);
                        walk_json(v, &full, g, rec, o);
                        g.settle();
                    }
                    // each item is a sub-record of the field's group
                    J::Array(items) => {
                        for item in items {
                            let g = group.enter(k);
                            match item {
                                J::Object(_) | J::Array(_) => walk_json(item, &full, g, rec, o),
                                scalar => g.add(rec, k, &format!("{full} - {k}"), json_scalar(scalar, o)),
                            }
                            g.settle();
                        }
                    }
                    scalar => group.add(rec, k, &full, json_scalar(scalar, o)),
                }
            }
        }
        J::Array(items) => {
            for item in items {
                walk_json(item, prefix, group, rec, o);
            }
        }
        scalar => group.add(rec, prefix, prefix, json_scalar(scalar, o)),
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
        group.reset();
        walk_json(node, &record_name, &mut group, &mut rec, o);
        records.push(rec);
    }
    Ok(tree_table(&group, &records))
}

/// Rows of tree records, columns in OpenRefine's order.
fn tree_table(group: &Group, records: &[JsonRecord]) -> Table {
    let mut names = vec![];
    group.order(&mut names);
    let rows: Vec<&Vec<(String, Value)>> = records.iter().flat_map(|r| r.rows.iter()).collect();
    let cols = names
        .iter()
        .map(|n| Column::from_values(rows.iter().map(|r| r.iter().find(|(c, _)| c == n).map_or(Value::Null, |(_, v)| v.clone()))))
        .collect();
    Table::new(names, cols, rows.len())
}

// ---------------------------------------------------------------- XML

enum XNode {
    El(XEl),
    Text(String),
}

struct XEl {
    name: String,
    attrs: Vec<(String, String)>,
    children: Vec<XNode>,
}

/// Adjacent text, entity references and CDATA make one text node.
fn xml_text(stack: &mut [XEl], s: &str) {
    if let Some(top) = stack.last_mut() {
        match top.children.last_mut() {
            Some(XNode::Text(t)) => t.push_str(s),
            _ => top.children.push(XNode::Text(s.to_string())),
        }
    }
}

// ponytail: the whole document becomes a tree in memory (a few times the file size); stream the records
// from the reader if XML files of hundreds of MB need importing.
fn parse_xml(text: &str) -> Result<XEl> {
    use quick_xml::events::{BytesStart, Event};
    let el = |e: &BytesStart| -> Result<XEl> {
        let attrs = e
            .attributes()
            .map(|a| {
                let a = a.map_err(|e| format!("XML: {e}"))?;
                Ok((a.key.as_ref().to_string(), a.normalized_value(quick_xml::XmlVersion::Implicit1_0).map_err(|e| format!("XML: {e}"))?.into_owned()))
            })
            .collect::<Result<_>>()?;
        Ok(XEl { name: e.name().as_ref().to_string(), attrs, children: vec![] })
    };
    let close = |stack: &mut Vec<XEl>, root: &mut Option<XEl>, x: XEl| match stack.last_mut() {
        Some(p) => p.children.push(XNode::El(x)),
        None => *root = root.take().or(Some(x)),
    };
    let mut r = quick_xml::Reader::from_str(text.trim_start_matches('\u{feff}'));
    let (mut stack, mut root) = (vec![], None);
    loop {
        match r.read_event().map_err(|e| format!("XML, at byte {}: {e}", r.buffer_position()))? {
            Event::Start(e) => stack.push(el(&e)?),
            Event::Empty(e) => {
                let x = el(&e)?;
                close(&mut stack, &mut root, x);
            }
            Event::End(_) => {
                let x = stack.pop().ok_or("XML: a closing tag with no opening one")?;
                close(&mut stack, &mut root, x);
            }
            Event::Text(t) => xml_text(&mut stack, &t.xml_content(quick_xml::XmlVersion::Implicit1_0)),
            Event::CData(c) => xml_text(&mut stack, &c.into_inner()),
            Event::GeneralRef(g) => {
                let s = match g.resolve_char_ref().map_err(|e| format!("XML: {e}"))? {
                    Some(c) => c.to_string(),
                    None => match &*g.into_inner() {
                        "amp" => "&".into(),
                        "lt" => "<".into(),
                        "gt" => ">".into(),
                        "quot" => "\"".into(),
                        "apos" => "'".into(),
                        other => format!("&{other};"),
                    },
                };
                xml_text(&mut stack, &s);
            }
            Event::Eof => break,
            _ => {}
        }
    }
    root.ok_or_else(|| "XML: no root element".into())
}

/// Every element path from the root (record-path candidates), with how many elements it reaches.
fn xml_paths(root: &XEl) -> Vec<(Vec<String>, usize)> {
    fn walk(e: &XEl, path: &mut Vec<String>, out: &mut Vec<(Vec<String>, usize)>) {
        path.push(e.name.clone());
        match out.iter_mut().find(|(p, _)| p == path) {
            Some(x) => x.1 += 1,
            None => out.push((path.clone(), 1)),
        }
        for c in &e.children {
            if let XNode::El(c) = c {
                walk(c, path, out);
            }
        }
        path.pop();
    }
    let mut out = vec![];
    walk(root, &mut vec![], &mut out);
    out
}

/// OpenRefine's XML record: attributes are columns "<path> - <name>", an element's text is the
/// column of its own path, child elements nest as groups; text that is only whitespace is skipped.
fn walk_xml(e: &XEl, prefix: &str, group: &mut Group, rec: &mut JsonRecord, o: &ImportOptions) {
    // namespace declarations are not data
    for (k, v) in e.attrs.iter().filter(|(k, _)| k != "xmlns" && !k.starts_with("xmlns:")) {
        group.add(rec, k, &format!("{prefix} - {k}"), tree_text(v, o));
    }
    for c in &e.children {
        match c {
            XNode::Text(t) if !t.trim().is_empty() => {
                // a key no attribute can have: the element's own text
                group.add(rec, "\u{0}", prefix, tree_text(t, o));
            }
            XNode::Text(_) => {}
            XNode::El(c) => {
                let g = group.enter(&c.name);
                walk_xml(c, &format!("{prefix} - {}", c.name), g, rec, o);
                g.settle();
            }
        }
    }
}

fn import_xml(path: &Path, o: &ImportOptions) -> Result<Table> {
    let root = parse_xml(&decode(path, &o.encoding)?)?;
    let want = if o.record_path.is_empty() { vec![root.name.clone()] } else { o.record_path.clone() };
    // the elements at the record path, from the root down
    let mut nodes: Vec<&XEl> = if want[0] == root.name { vec![&root] } else { vec![] };
    for seg in &want[1..] {
        nodes = nodes
            .into_iter()
            .flat_map(|n| n.children.iter().filter_map(|c| match c {
                XNode::El(x) if &x.name == seg => Some(x),
                _ => None,
            }))
            .collect();
    }
    let name = want.last().cloned().unwrap_or_default();
    let mut group = Group::default();
    let mut records = vec![];
    for node in nodes.into_iter().take(if o.limit >= 0 { o.limit as usize } else { usize::MAX }) {
        let mut rec = JsonRecord { rows: vec![vec![]] };
        group.reset();
        walk_xml(node, &name, &mut group, &mut rec, o);
        records.push(rec);
    }
    Ok(tree_table(&group, &records))
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The parallel data stage must build exactly the table the single-threaded one builds: same
    /// names, codes and dictionary order, across batch boundaries, ragged rows, late columns,
    /// blank rows, quoted newlines and every row option.
    #[test]
    fn parallel_import_matches_serial() {
        let mut text = String::from("\u{feff}title,a,b,c\nsecond,header,line,x\n");
        for r in 0..40_000u32 {
            let line = match r % 11 {
                0 => format!("{r},\"multi\nline {}\",{},  pad  ", r % 7, r % 3),
                1 => format!("{r},short"),
                2 => String::new(),
                3 => format!("{r},{},{},{},extra {},{}", r % 5, r % 2, r, r % 13, r % 4),
                4 => ",,,".to_string(),
                5 => format!("{r},{}.5,true,", r % 9),
                _ => format!("{r},v{},w{},{}", r % 101, r % 17, r % 3),
            };
            text += &line;
            text += "\n";
            if r == 30_000 {
                text += "late,1,2,3,4,5,6,7,8,9,10,11\n";
            }
        }
        let path = std::env::temp_dir().join("oxrefine-parallel-import.csv");
        std::fs::write(&path, &text).unwrap();
        let base = ImportOptions::default();
        let variants = [
            ImportOptions { ..base.clone() },
            ImportOptions { header_lines: 2, ..base.clone() },
            ImportOptions { ignore_lines: 1, skip_data_lines: 3, ..base.clone() },
            ImportOptions { store_blank_rows: false, ..base.clone() },
            ImportOptions { limit: 20_000, ..base.clone() },
            ImportOptions { trim_strings: true, guess_cell_value_types: true, store_blank_cells_as_nulls: false, ..base.clone() },
            ImportOptions { header_lines: 0, ..base.clone() },
        ];
        for (k, o) in variants.iter().enumerate() {
            let serial = import_text(&path, o, u64::MAX).unwrap();
            let parallel = import_text(&path, o, 0).unwrap();
            assert_eq!(serial.names, parallel.names, "variant {k}: names");
            assert_eq!(serial.rows, parallel.rows, "variant {k}: rows");
            for (c, (s, p)) in serial.columns.iter().zip(&parallel.columns).enumerate() {
                assert!(s.dict == p.dict && s.codes == p.codes, "variant {k}: column {c} differs");
            }
        }
    }
}
