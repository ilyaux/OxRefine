//! Exporters with OpenRefine's formats and options: CSV/TSV and custom tabular ("*sv"), HTML table,
//! templating, SQL and Excel. Behaviour is pinned to OpenRefine 3.10.1 by tests/export_conformance.rs.
use crate::{
    engine::Ctx,
    err,
    grel::{self, text::java_strip, Value},
    table::Table,
    Result,
};
use serde::Deserialize;
use serde_json::Value as J;

/// OpenRefine's "*sv" exporter options (the Custom tabular exporter dialog).
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TabularOptions {
    pub separator: String,
    pub line_separator: String,
    pub quote_all: bool,
    pub output_column_headers: bool,
    pub output_blank_rows: bool,
    pub columns: Vec<ColumnChoice>,
    /// OpenRefine's "tsv" format writes fields as they are; csv and "*sv" quote when needed.
    #[serde(skip)]
    pub raw: bool,
}

impl Default for TabularOptions {
    fn default() -> Self {
        TabularOptions { separator: ",".into(), line_separator: "\r\n".into(), quote_all: false, output_column_headers: true, output_blank_rows: true, columns: vec![], raw: false }
    }
}

#[derive(Clone, Deserialize)]
pub struct ColumnChoice {
    pub name: String,
}

#[derive(Clone, Deserialize, Default)]
#[serde(default)]
pub struct TemplateOptions {
    pub template: String,
    pub prefix: String,
    pub suffix: String,
    pub separator: String,
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SqlOptions {
    pub table_name: String,
    pub include_structure: bool,
    pub include_drop_statement: bool,
    pub include_if_exist_with_drop_statement: bool,
    pub include_content: bool,
    pub include_null_field_value: bool,
    #[serde(alias = "convertNulltoEmptyString")]
    pub convert_null_to_empty_string: bool,
    pub columns: Vec<SqlColumn>,
}

impl Default for SqlOptions {
    fn default() -> Self {
        SqlOptions {
            table_name: "data".into(),
            include_structure: true,
            include_drop_statement: false,
            include_if_exist_with_drop_statement: false,
            include_content: true,
            include_null_field_value: true,
            convert_null_to_empty_string: false,
            columns: vec![],
        }
    }
}

#[derive(Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct SqlColumn {
    pub name: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub size: J,
    pub allow_null: bool,
}

/// Export `rows` of `t`. `params` holds the exporter's own options as OpenRefine sends them
/// (`options` as a JSON string or object, or the template fields at top level).
pub fn export(t: &Table, rows: &[u32], format: &str, params: &J, project_name: &str) -> Result<Vec<u8>> {
    let mut out = vec![];
    export_to(t, rows, format, params, project_name, &mut out)?;
    Ok(out)
}

/// Export straight into a file: delimited text is written as it is made, never whole in memory.
pub fn export_file(t: &Table, rows: &[u32], format: &str, params: &J, project_name: &str, path: &std::path::Path) -> Result<()> {
    let f = std::fs::File::create(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut w = std::io::BufWriter::new(f);
    export_to(t, rows, format, params, project_name, &mut w)?;
    std::io::Write::flush(&mut w).map_err(|e| format!("{}: {e}", path.display()))
}

pub fn export_to(t: &Table, rows: &[u32], format: &str, params: &J, project_name: &str, w: &mut dyn std::io::Write) -> Result<()> {
    let options = |p: &J| -> J {
        match p.get("options") {
            Some(J::String(s)) => serde_json::from_str(s).unwrap_or(J::Null),
            Some(o) => o.clone(),
            None => J::Null,
        }
    };
    let opts = options(params);
    let tabular = |sep: &str| -> Result<TabularOptions> {
        let mut o: TabularOptions = if opts.is_null() { TabularOptions::default() } else { serde_json::from_value(opts.clone()).map_err(err)? };
        if !sep.is_empty() {
            o.separator = sep.into();
        }
        Ok(o)
    };
    let bytes = match format {
        "csv" => return tabular_write(t, rows, &tabular(",")?, w),
        "tsv" => return tabular_write(t, rows, &TabularOptions { raw: true, ..tabular("\t")? }, w),
        "*sv" | "custom" => return tabular_write(t, rows, &tabular("")?, w),
        "html" => html(t, rows, project_name).into_bytes(),
        "template" => {
            let o: TemplateOptions = serde_json::from_value(params.clone()).map_err(err)?;
            template(t, rows, &o)?.into_bytes()
        }
        "sql" => {
            let o: SqlOptions = if opts.is_null() { SqlOptions::default() } else { serde_json::from_value(opts).map_err(err)? };
            sql(t, rows, &o)?.into_bytes()
        }
        "xlsx" => xlsx(t, rows, project_name)?,
        "xls" => xls(t, rows, project_name)?,
        "ods" => ods(t, rows, project_name)?,
        f => return Err(format!("unknown export format `{f}` (csv, tsv, *sv, html, template, sql, xlsx, xls, ods)")),
    };
    w.write_all(&bytes).map_err(err)
}

/// Export format from a file name: .tsv, .html/.htm, .xlsx, .xls, .ods, .sql, anything else CSV.
pub fn format_for(path: &str) -> &'static str {
    let ext = path.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
    match ext.as_str() {
        "tsv" | "tab" => "tsv",
        "html" | "htm" => "html",
        "xlsx" => "xlsx",
        "xls" => "xls",
        "ods" => "ods",
        "sql" => "sql",
        _ => "csv",
    }
}

fn columns(t: &Table, chosen: &[ColumnChoice]) -> Result<Vec<usize>> {
    if chosen.is_empty() {
        return Ok((0..t.names.len()).collect());
    }
    chosen.iter().map(|c| t.col(&c.name)).collect()
}

fn quote(s: &str, o: &TabularOptions) -> String {
    if o.quote_all || !o.raw && (s.contains(o.separator.as_str()) || s.contains(['"', '\n', '\r'])) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

pub fn tabular_text(t: &Table, rows: &[u32], o: &TabularOptions) -> Result<String> {
    let mut out = vec![];
    tabular_write(t, rows, o, &mut out)?;
    Ok(String::from_utf8(out).map_err(err)?)
}

fn tabular_write(t: &Table, rows: &[u32], o: &TabularOptions, w: &mut dyn std::io::Write) -> Result<()> {
    let cols = columns(t, &o.columns)?;
    // what the grid shows, trimmed like OpenRefine does; (quoted text, blank)
    let render = |v: &Value| {
        let s = java_strip(&v.display()).to_string();
        let blank = s.is_empty();
        (quote(&s, o), blank)
    };
    // render each distinct value once per column when values repeat; columns of mostly unique values
    // (and previews of a few rows) are rendered row by row instead of keeping every value's text
    let text: Vec<Option<Vec<(String, bool)>>> =
        cols.iter().map(|&c| (rows.len() >= 2 * t.columns[c].dict.len()).then(|| t.columns[c].dict.iter().map(render).collect())).collect();
    let mut out = String::new();
    if o.output_column_headers {
        out += &cols.iter().map(|&c| quote(&t.names[c], o)).collect::<Vec<_>>().join(&o.separator);
        out += &o.line_separator;
    }
    let mut cells: Vec<std::borrow::Cow<(String, bool)>> = Vec::with_capacity(cols.len());
    for &r in rows {
        cells.clear();
        cells.extend(cols.iter().zip(&text).map(|(&c, txt)| {
            let code = t.columns[c].codes[r as usize] as usize;
            match txt {
                Some(txt) => std::borrow::Cow::Borrowed(&txt[code]),
                None => std::borrow::Cow::Owned(render(&t.columns[c].dict[code])),
            }
        }));
        if !o.output_blank_rows && cells.iter().all(|c| c.1) {
            continue;
        }
        for (i, c) in cells.iter().enumerate() {
            if i > 0 {
                out += &o.separator;
            }
            out += &c.0;
        }
        out += &o.line_separator;
        // hand the text over in chunks of about 64 KB
        if out.len() >= 1 << 16 {
            w.write_all(out.as_bytes()).map_err(err)?;
            out.clear();
        }
    }
    w.write_all(out.as_bytes()).map_err(err)
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn html(t: &Table, rows: &[u32], name: &str) -> String {
    let mut out = format!("<html>\n<head>\n<title>{}</title>\n<meta charset=\"utf-8\" />\n</head>\n<body>\n<table>\n", escape(name));
    out += "<tr>";
    for n in &t.names {
        out += &format!("<th>{}</th>", escape(n));
    }
    out += "</tr>\n";
    for &r in rows {
        out += "<tr>";
        for c in &t.columns {
            out += &format!("<td>{}</td>", escape(&c.get(r as usize).display()));
        }
        out += "</tr>\n";
    }
    out + "</table>\n</body>\n</html>\n"
}

enum Piece {
    Text(String),
    Expr(grel::Program),
}

/// Split a template into literal text and {{ GREL }} pieces.
fn compile_template(src: &str) -> Result<Vec<Piece>> {
    let mut out = vec![];
    let mut rest = src;
    while let Some(start) = rest.find("{{") {
        out.push(Piece::Text(rest[..start].to_string()));
        let end = rest[start + 2..].find("}}").ok_or("a {{ in the template has no closing }}")?;
        out.push(Piece::Expr(grel::compile(&rest[start + 2..start + 2 + end])?));
        rest = &rest[start + 2 + end + 2..];
    }
    out.push(Piece::Text(rest.to_string()));
    Ok(out)
}

pub fn template(t: &Table, rows: &[u32], o: &TemplateOptions) -> Result<String> {
    let pieces = compile_template(&o.template)?;
    let ctx = Ctx::default();
    let rendered: Vec<String> = rows
        .iter()
        .map(|&r| {
            let row = t.row_ctx(r as usize, &ctx);
            pieces
                .iter()
                .map(|p| match p {
                    Piece::Text(s) => s.clone(),
                    // expressions see the row, not a single cell; null and errors print as "null"
                    Piece::Expr(e) => match e.eval(Value::Null, &row) {
                        Value::Null | Value::Error(_) => "null".into(),
                        v => v.to_java_string(),
                    },
                })
                .collect()
        })
        .collect();
    Ok(format!("{}{}{}", o.prefix, rendered.join(&o.separator), o.suffix))
}

fn sql_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

/// SQL dump. OpenRefine 3.10's exporter drops numeric values and null fields (shifting columns);
/// we write NULL and unquoted numbers instead, so the result loads.
pub fn sql(t: &Table, rows: &[u32], o: &SqlOptions) -> Result<String> {
    let specs: Vec<SqlColumn> = if o.columns.is_empty() {
        t.names.iter().map(|n| SqlColumn { name: n.clone(), kind: "VARCHAR".into(), size: J::from(255), allow_null: true }).collect()
    } else {
        o.columns.clone()
    };
    let idx = specs.iter().map(|c| t.col(&c.name)).collect::<Result<Vec<_>>>()?;
    let ident = |n: &str| format!("\"{}\"", n.replace('"', "\"\""));
    let mut out = String::new();
    if o.include_drop_statement {
        out += &format!("DROP TABLE {}{};\n", if o.include_if_exist_with_drop_statement { "IF EXISTS " } else { "" }, o.table_name);
    }
    if o.include_structure {
        let cols: Vec<String> = specs
            .iter()
            .map(|c| {
                let size = match &c.size {
                    J::String(s) if !s.is_empty() => format!("({s})"),
                    J::Number(n) => format!("({n})"),
                    _ => String::new(),
                };
                format!("{} {}{} {}", ident(&c.name), if c.kind.is_empty() { "VARCHAR" } else { &c.kind }, size, if c.allow_null { "NULL" } else { "NOT NULL" })
            })
            .collect();
        out += &format!("CREATE TABLE {} (\n{}\n);\n", o.table_name, cols.join(",\n"));
    }
    if o.include_content && !rows.is_empty() {
        let names: Vec<String> = specs.iter().map(|c| ident(&c.name)).collect();
        out += &format!("INSERT INTO {} ({}) VALUES \n", o.table_name, names.join(","));
        let lines: Vec<String> = rows
            .iter()
            .map(|&r| {
                let vals: Vec<String> = idx
                    .iter()
                    .zip(&specs)
                    .map(|(&c, spec)| {
                        let v = t.columns[c].get(r as usize);
                        let numeric = matches!(spec.kind.to_ascii_uppercase().as_str(), "INT" | "INTEGER" | "BIGINT" | "SMALLINT" | "DOUBLE" | "FLOAT" | "REAL" | "DECIMAL" | "NUMERIC");
                        match v {
                            v if v.is_blank() && o.convert_null_to_empty_string => "''".into(),
                            v if v.is_blank() => "NULL".into(),
                            Value::Int(_) | Value::Num(_) if numeric => v.display(),
                            v => sql_quote(&v.display()),
                        }
                    })
                    .collect();
                format!("( {} )", vals.join(","))
            })
            .collect();
        out += &lines.join(",\n");
        out += ";\n";
    }
    Ok(out)
}

pub fn xlsx(t: &Table, rows: &[u32], name: &str) -> Result<Vec<u8>> {
    use rust_xlsxwriter::{ExcelDateTime, Format, Workbook};
    let mut wb = Workbook::new();
    let date_fmt = Format::new().set_num_format("yyyy-mm-dd hh:mm:ss");
    let ws = wb.add_worksheet();
    ws.set_name(sheet_name(name)).map_err(err)?;
    for (c, n) in t.names.iter().enumerate() {
        ws.write_string(0, c as u16, n).map_err(err)?;
    }
    for (i, &r) in rows.iter().enumerate() {
        let row = i as u32 + 1;
        for (c, col) in t.columns.iter().enumerate() {
            let c = c as u16;
            match col.get(r as usize) {
                Value::Null => {}
                Value::Int(n) => drop(ws.write_number(row, c, *n as f64).map_err(err)?),
                Value::Num(n) => drop(ws.write_number(row, c, *n).map_err(err)?),
                Value::Bool(b) => drop(ws.write_boolean(row, c, *b).map_err(err)?),
                Value::Date(d) => {
                    use chrono::{Datelike, Timelike};
                    let dt = ExcelDateTime::from_ymd(d.year() as u16, d.month() as u8, d.day() as u8)
                        .and_then(|x| x.and_hms(d.hour() as u16, d.minute() as u8, d.second() as f64))
                        .map_err(err)?;
                    ws.write_datetime_with_format(row, c, &dt, &date_fmt).map_err(err)?;
                }
                v => drop(ws.write_string(row, c, v.display()).map_err(err)?),
            }
        }
    }
    wb.save_to_buffer().map_err(err)
}

/// The sheet name OpenRefine gives a workbook: the project's name, at most 31 characters, none of []:*?/\
fn sheet_name(name: &str) -> String {
    let sheet: String = name.chars().filter(|c| !"[]:*?/\\".contains(*c)).take(31).collect();
    if sheet.trim().is_empty() { "Sheet1".to_string() } else { sheet }
}

/// OpenRefine's Excel 97 export (`.xls`, BIFF8, as its XlsExporter writes through POI): one sheet named
/// after the project, a header row, then numbers, booleans and dates as typed cells and everything else
/// as text, cut at Excel's 32,767 characters; blank cells stay empty. A sheet holds at most 65,536 rows
/// and 256 columns.
pub fn xls(t: &Table, rows: &[u32], name: &str) -> Result<Vec<u8>> {
    use std::io::Write;
    const MAX_ROWS: usize = 65_536;
    if rows.len() + 1 > MAX_ROWS {
        return Err(format!("an Excel 97 sheet holds at most {MAX_ROWS} rows; this export has {} with the header. Use .xlsx", rows.len() + 1));
    }
    if t.names.len() > 256 {
        return Err(format!("an Excel 97 sheet holds at most 256 columns; this table has {}. Use .xlsx", t.names.len()));
    }
    // cells: (row, column, what), strings as indexes into the shared string table
    enum Cell {
        Text(u32),
        Number(f64),
        Bool(bool),
        Date(f64),
    }
    let mut strings: Vec<String> = vec![];
    let mut index: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
    let mut text = |s: String| -> Cell {
        // Excel's limit, in UTF-16 units as Java counts them
        let s = if s.encode_utf16().count() > 32_767 { String::from_utf16_lossy(&s.encode_utf16().take(32_767).collect::<Vec<_>>()) } else { s };
        let next = strings.len() as u32;
        let i = *index.entry(s.clone()).or_insert_with(|| {
            strings.push(s);
            next
        });
        Cell::Text(i)
    };
    let mut cells: Vec<(u16, u16, Cell)> = vec![];
    for (c, n) in t.names.iter().enumerate() {
        cells.push((0, c as u16, text(n.clone())));
    }
    let epoch = chrono::NaiveDate::from_ymd_opt(1899, 12, 30).unwrap().and_hms_opt(0, 0, 0).unwrap();
    for (i, &r) in rows.iter().enumerate() {
        let row = (i + 1) as u16;
        for (c, col) in t.columns.iter().enumerate() {
            let cell = match col.get(r as usize) {
                Value::Null => continue,
                Value::Int(n) => Cell::Number(*n as f64),
                Value::Num(n) => Cell::Number(*n),
                Value::Bool(b) => Cell::Bool(*b),
                Value::Date(d) => Cell::Date((*d - epoch).num_milliseconds() as f64 / 86_400_000.0),
                v => text(v.display()),
            };
            cells.push((row, c as u16, cell));
        }
    }

    // BIFF8 records: id, length, data; a record holds at most 8,224 bytes, CONTINUE records carry the rest
    const MAX: usize = 8224;
    fn record(out: &mut Vec<u8>, id: u16, data: &[u8]) {
        out.extend_from_slice(&id.to_le_bytes());
        out.extend_from_slice(&(data.len() as u16).to_le_bytes());
        out.extend_from_slice(data);
    }
    let u16s = |v: &[u16]| v.iter().flat_map(|x| x.to_le_bytes()).collect::<Vec<u8>>();
    // a string as Excel stores it: 8-bit when every character fits in a byte, else UTF-16
    let units = |s: &str| -> (bool, Vec<u8>) {
        let u: Vec<u16> = s.encode_utf16().collect();
        if u.iter().all(|&c| c < 0x100) { (false, u.iter().map(|&c| c as u8).collect()) } else { (true, u.iter().flat_map(|c| c.to_le_bytes()).collect()) }
    };
    let bof = |dt: u16| [0x0600u16.to_le_bytes(), dt.to_le_bytes(), 0x0DBBu16.to_le_bytes(), 0x07CCu16.to_le_bytes(), [0, 0], [0, 0], [6, 0], [0, 0]].concat();

    let mut wb: Vec<u8> = vec![];
    record(&mut wb, 0x0809, &bof(0x0005));
    record(&mut wb, 0x0042, &1200u16.to_le_bytes()); // CODEPAGE: UTF-16
    record(&mut wb, 0x003D, &u16s(&[360, 270, 14940, 9150, 0x0038, 0, 0, 1, 600])); // WINDOW1
    let mut font = u16s(&[200, 0, 0x7FFF, 400, 0]);
    font.extend_from_slice(&[0, 0, 0, 0, 5, 0]);
    font.extend_from_slice(b"Arial");
    for _ in 0..4 {
        record(&mut wb, 0x0031, &font); // FONT (index 4 is never used, as in every Excel file)
    }
    let date_format = "yyyy-mm-dd hh:mm:ss";
    let mut format = u16s(&[164, date_format.len() as u16]);
    format.push(0);
    format.extend_from_slice(date_format.as_bytes());
    record(&mut wb, 0x041E, &format); // FORMAT 164
    // XF: 15 style records, the default cell format (15) and dates (16)
    let xf = |fmt: u16, kind: u16, used: u8| [u16s(&[0, fmt, kind]), vec![0x20, 0, 0, used], vec![0; 8], vec![0xC0, 0x20]].concat();
    for _ in 0..15 {
        record(&mut wb, 0x00E0, &xf(0, 0xFFF5, 0));
    }
    record(&mut wb, 0x00E0, &xf(0, 0x0001, 0));
    record(&mut wb, 0x00E0, &xf(164, 0x0001, 0x04));
    record(&mut wb, 0x0293, &[0x00, 0x80, 0x00, 0xFF]); // STYLE: Normal
    // BOUNDSHEET: where the sheet's BOF is, filled in once the globals are written
    let sheet = sheet_name(name);
    let (wide, chars) = units(&sheet);
    let boundsheet_at = wb.len() + 4;
    let mut bs = vec![0, 0, 0, 0, 0, 0, sheet.encode_utf16().count() as u8, wide as u8];
    bs.extend_from_slice(&chars);
    record(&mut wb, 0x0085, &bs);

    // SST, split into CONTINUE records; a string's header never splits, its characters may, and each
    // CONTINUE that goes on with characters starts with the string's 8-bit/UTF-16 flag again
    let total = cells.iter().filter(|c| matches!(c.2, Cell::Text(_))).count() as u32;
    let mut current: (u16, Vec<u8>) = (0x00FC, [total.to_le_bytes(), (strings.len() as u32).to_le_bytes()].concat());
    let dsst = (strings.len().div_ceil(128)).max(8);
    let mut buckets: Vec<(u32, u16)> = vec![];
    let flush = |wb: &mut Vec<u8>, current: &mut (u16, Vec<u8>)| {
        record(wb, current.0, &current.1);
        *current = (0x003C, vec![]);
    };
    for (k, s) in strings.iter().enumerate() {
        let (wide, mut chars) = units(s);
        if current.1.len() + 3 > MAX {
            flush(&mut wb, &mut current);
        }
        if k % dsst == 0 {
            buckets.push(((wb.len() + 4 + current.1.len()) as u32, (4 + current.1.len()) as u16));
        }
        current.1.extend_from_slice(&(s.encode_utf16().count() as u16).to_le_bytes());
        current.1.push(wide as u8);
        let unit = if wide { 2 } else { 1 };
        while !chars.is_empty() {
            let room = (MAX - current.1.len()) / unit * unit;
            if room == 0 {
                flush(&mut wb, &mut current);
                current.1.push(wide as u8);
                continue;
            }
            let take = room.min(chars.len());
            current.1.extend(chars.drain(..take));
        }
    }
    flush(&mut wb, &mut current);
    let mut extsst = (dsst as u16).to_le_bytes().to_vec();
    for (ib, cb) in buckets {
        extsst.extend_from_slice(&ib.to_le_bytes());
        extsst.extend_from_slice(&cb.to_le_bytes());
        extsst.extend_from_slice(&[0, 0]);
    }
    record(&mut wb, 0x00FF, &extsst);
    record(&mut wb, 0x000A, &[]);

    // the sheet
    let at = wb.len() as u32;
    wb[boundsheet_at..boundsheet_at + 4].copy_from_slice(&at.to_le_bytes());
    record(&mut wb, 0x0809, &bof(0x0010));
    let mut dims = ((rows.len() + 1) as u32).to_le_bytes().to_vec();
    dims.splice(0..0, 0u32.to_le_bytes());
    dims.extend_from_slice(&u16s(&[0, t.names.len() as u16, 0]));
    record(&mut wb, 0x0200, &dims); // DIMENSIONS
    record(&mut wb, 0x023E, &[u16s(&[0x06B6, 0, 0, 0x40, 0, 0, 0]), vec![0, 0, 0, 0]].concat()); // WINDOW2
    for (row, col, cell) in &cells {
        let head = |xf: u16| u16s(&[*row, *col, xf]);
        match cell {
            Cell::Text(i) => record(&mut wb, 0x00FD, &[head(15), i.to_le_bytes().to_vec()].concat()),
            Cell::Number(n) => record(&mut wb, 0x0203, &[head(15), n.to_le_bytes().to_vec()].concat()),
            Cell::Date(n) => record(&mut wb, 0x0203, &[head(16), n.to_le_bytes().to_vec()].concat()),
            Cell::Bool(b) => record(&mut wb, 0x0205, &[head(15), vec![*b as u8, 0]].concat()),
        }
    }
    record(&mut wb, 0x000A, &[]);

    // the OLE container Excel 97 files are, with the workbook as its "Workbook" stream
    let mut cf = cfb::CompoundFile::create_with_version(cfb::Version::V3, std::io::Cursor::new(Vec::new())).map_err(err)?;
    let mut stream = cf.create_stream("/Workbook").map_err(err)?;
    stream.write_all(&wb).map_err(err)?;
    drop(stream);
    cf.flush().map_err(err)?;
    Ok(cf.into_inner().into_inner())
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// OpenRefine's ODS export: one sheet named after the project, a header row, then text, numbers
/// (floats), booleans and dates as typed cells; blank cells stay empty.
pub fn ods(t: &Table, rows: &[u32], name: &str) -> Result<Vec<u8>> {
    use std::io::Write;
    let cell = |v: &Value| -> String {
        match v {
            Value::Null => "<table:table-cell/>".into(),
            Value::Int(n) => format!("<table:table-cell office:value-type=\"float\" office:value=\"{n}\"><text:p>{n}</text:p></table:table-cell>"),
            Value::Num(n) => {
                let s = crate::grel::value::java_double(*n);
                format!("<table:table-cell office:value-type=\"float\" office:value=\"{s}\"><text:p>{s}</text:p></table:table-cell>")
            }
            Value::Bool(b) => format!("<table:table-cell office:value-type=\"boolean\" office:boolean-value=\"{b}\"><text:p>{b}</text:p></table:table-cell>"),
            Value::Date(d) => {
                let s = if d.time() == chrono::NaiveTime::MIN { d.format("%Y-%m-%d").to_string() } else { d.format("%Y-%m-%dT%H:%M:%S").to_string() };
                format!("<table:table-cell office:value-type=\"date\" office:date-value=\"{s}\"><text:p>{s}</text:p></table:table-cell>")
            }
            v => {
                let s = xml_escape(&v.display());
                format!("<table:table-cell office:value-type=\"string\" office:string-value=\"{s}\"><text:p>{s}</text:p></table:table-cell>")
            }
        }
    };
    let sheet = if name.trim().is_empty() { "Sheet1".to_string() } else { xml_escape(name) };
    let mut body = format!("<table:table table:name=\"{sheet}\"><table:table-column table:number-columns-repeated=\"{}\"/>", t.names.len().max(1));
    body += "<table:table-row>";
    for n in &t.names {
        body += &cell(&Value::Str(n.clone()));
    }
    body += "</table:table-row>";
    for &r in rows {
        body += "<table:table-row>";
        for c in &t.columns {
            body += &cell(c.get(r as usize));
        }
        body += "</table:table-row>";
    }
    body += "</table:table>";
    let content = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><office:document-content xmlns:office=\"urn:oasis:names:tc:opendocument:xmlns:office:1.0\" \
         xmlns:table=\"urn:oasis:names:tc:opendocument:xmlns:table:1.0\" xmlns:text=\"urn:oasis:names:tc:opendocument:xmlns:text:1.0\" \
         office:version=\"1.2\"><office:body><office:spreadsheet>{body}</office:spreadsheet></office:body></office:document-content>"
    );
    let manifest = "<?xml version=\"1.0\" encoding=\"UTF-8\"?><manifest:manifest xmlns:manifest=\"urn:oasis:names:tc:opendocument:xmlns:manifest:1.0\" manifest:version=\"1.2\">\
        <manifest:file-entry manifest:full-path=\"/\" manifest:media-type=\"application/vnd.oasis.opendocument.spreadsheet\"/>\
        <manifest:file-entry manifest:full-path=\"content.xml\" manifest:media-type=\"text/xml\"/></manifest:manifest>";
    let mut z = zip::ZipWriter::new(std::io::Cursor::new(vec![]));
    // the mimetype comes first and uncompressed, so tools can tell the file type from its first bytes
    let stored = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let deflated = zip::write::SimpleFileOptions::default();
    z.start_file("mimetype", stored).map_err(err)?;
    z.write_all(b"application/vnd.oasis.opendocument.spreadsheet").map_err(err)?;
    z.start_file("META-INF/manifest.xml", deflated).map_err(err)?;
    z.write_all(manifest.as_bytes()).map_err(err)?;
    z.start_file("content.xml", deflated).map_err(err)?;
    z.write_all(content.as_bytes()).map_err(err)?;
    Ok(z.finish().map_err(err)?.into_inner())
}
