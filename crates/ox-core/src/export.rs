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
    Ok(match format {
        "csv" => tabular_text(t, rows, &tabular(",")?)?.into_bytes(),
        "tsv" => tabular_text(t, rows, &TabularOptions { raw: true, ..tabular("\t")? })?.into_bytes(),
        "*sv" | "custom" => tabular_text(t, rows, &tabular("")?)?.into_bytes(),
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
        "ods" => ods(t, rows, project_name)?,
        f => return Err(format!("unknown export format `{f}` (csv, tsv, *sv, html, template, sql, xlsx, ods)")),
    })
}

/// Export format from a file name: .tsv, .html/.htm, .xlsx, .ods, .sql, anything else CSV.
pub fn format_for(path: &str) -> &'static str {
    let ext = path.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
    match ext.as_str() {
        "tsv" | "tab" => "tsv",
        "html" | "htm" => "html",
        "xlsx" => "xlsx",
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
    let cols = columns(t, &o.columns)?;
    // what the grid shows, trimmed like OpenRefine does; (quoted text, blank)
    let render = |v: &Value| {
        let s = java_strip(&v.display()).to_string();
        let blank = s.is_empty();
        (quote(&s, o), blank)
    };
    // render each distinct value once per column, unless there are fewer rows than values (previews)
    let text: Vec<Option<Vec<(String, bool)>>> =
        cols.iter().map(|&c| (rows.len() >= t.columns[c].dict.len()).then(|| t.columns[c].dict.iter().map(render).collect())).collect();
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
    }
    Ok(out)
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
    // sheet names: at most 31 characters, none of []:*?/\
    let sheet: String = name.chars().filter(|c| !"[]:*?/\\".contains(*c)).take(31).collect();
    ws.set_name(if sheet.trim().is_empty() { "Sheet1".to_string() } else { sheet }).map_err(err)?;
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
