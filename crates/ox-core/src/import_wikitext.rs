//! Wikitext table import, pinned to the actual OpenRefine 3.10.1 importer fixtures.
use super::{decode, Builder, Field, ImportOptions};
use crate::{grel::Value, jobs, recon::{Recon, ReconCol, StandardConfig}, table::Table, Result};
use parse_wiki_text::{Configuration, Node, Positioned, TableCell, TableRow, WarningMessage};
use regex::Regex;
use serde_json::{json, Value as J};
use std::{collections::{HashMap, HashSet}, path::Path, sync::{Arc, OnceLock}, time::Duration};

const WIKI_BASE: &str = "https://en.wikipedia.org/wiki/";
const SERVICE: &str = "https://wikidata.reconci.link/en/api";
const MAX_CELLS: usize = 5_000_000;
const MAX_COLUMNS: usize = 10_000;
const MAX_LINKS: usize = 100_000;
const MAX_TEXT: usize = 64 * 1024 * 1024;
const MAX_NESTING: usize = 128;

#[derive(Clone, Default)]
struct Cell {
    value: Option<String>,
    links: Vec<String>,
    reference: Option<String>,
    recon: Option<Arc<Recon>>,
}

struct Span {
    cell: Cell,
    row: usize,
    col: usize,
    rows: usize,
    cols: usize,
}

#[derive(Default)]
struct Budget { cells: usize, text: usize }
impl Budget {
    fn cell(&mut self, cell: &Cell) -> Result<()> {
        self.cells = self.cells.checked_add(1).filter(|n| *n <= MAX_CELLS).ok_or("Wikitext tables exceed 5,000,000 expanded cells")?;
        let bytes = cell.value.as_ref().map_or(0, String::len) + cell.reference.as_ref().map_or(0, String::len)
            + cell.links.iter().map(String::len).sum::<usize>();
        self.text = self.text.checked_add(bytes).filter(|n| *n <= MAX_TEXT).ok_or("Wikitext spans exceed the 64 MiB expanded text limit")?;
        Ok(())
    }
}

fn attributes(text: &str) -> HashMap<&str, &str> {
    static ATTR: OnceLock<Regex> = OnceLock::new();
    let regex = ATTR.get_or_init(|| Regex::new(r#"([A-Za-z][\w:-]*)\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s>]+))"#).unwrap());
    regex.captures_iter(text).map(|m| (m.get(1).unwrap().as_str(), m.get(2).or_else(|| m.get(3)).or_else(|| m.get(4)).unwrap().as_str())).collect()
}

fn reference_url(text: &str) -> Option<String> {
    static URL: OnceLock<Regex> = OnceLock::new();
    URL.get_or_init(|| Regex::new(r"(?i)\b(https?|ftp)://[-a-zA-Z0-9+&@#/%?=~_!:,.;]*[-a-zA-Z0-9+&@#/%=~_]").unwrap())
        .find_iter(text).last().map(|m| m.as_str().to_string())
}

fn plain(text: &str) -> String {
    static URL: OnceLock<Regex> = OnceLock::new();
    URL.get_or_init(|| Regex::new(r"(?i)https?://[^\s<>\[\]{}]+|&#(?:[0-9]+|x[0-9a-f]+);").unwrap()).replace_all(text, "").into_owned()
}

struct Renderer<'a, 'b> {
    text: &'a str,
    raw: bool,
    named: &'b mut HashMap<String, String>,
    cell: Cell,
    output: String,
    suppressed: bool,
}

impl Renderer<'_, '_> {
    fn write(&mut self, text: &str) {
        if !self.suppressed { self.output.push_str(text); }
    }

    fn nodes(&mut self, nodes: &[Node<'_>], depth: usize) -> Result<()> {
        if depth > MAX_NESTING { return Err("Wikitext markup exceeds 128 nested elements".into()); }
        for (i, node) in nodes.iter().enumerate() {
            if i % 64 == 0 { jobs::tick(0)?; }
            match node {
                Node::Text { value, .. } => self.write(&plain(value)),
                Node::Link { target, text, start, end } => {
                    self.cell.links.push((*target).into());
                    let source = &self.text[*start..*end];
                    let close = source.rfind("]]").unwrap_or(source.len());
                    if !source[..close].contains('|') {
                        // Sweble's no-title visitor bypasses ordinary text/reference suppression.
                        self.output.push_str(target);
                    } else {
                        let title: Vec<&Node> = text.iter().filter(|n| n.start() < start + close).collect();
                        for item in title { self.nodes(std::slice::from_ref(item), depth + 1)?; }
                    }
                }
                Node::Category { target, ordinal, .. } => {
                    self.cell.links.push((*target).into());
                    if ordinal.is_empty() { self.output.push_str(target); }
                    else { self.nodes(ordinal, depth + 1)?; }
                }
                Node::ExternalLink { start, end, .. } => {
                    let raw = &self.text[*start..*end];
                    let inside = raw.strip_prefix('[').unwrap_or(raw).trim_start();
                    if inside.starts_with("http://") || inside.starts_with("https://") {
                        self.output.push_str(inside.split(|c: char| c.is_whitespace() || c == ']').next().unwrap_or(""));
                    } else {
                        self.write(raw);
                    }
                }
                Node::Tag { name, start, end, .. } if name == "ref" => {
                    let raw = &self.text[*start..*end];
                    let opening = raw.split_once('>').map_or(raw, |(s, _)| s);
                    let attrs = attributes(opening);
                    if let Some(name) = attrs.get("name") {
                        self.cell.reference = self.named.get(*name).cloned();
                    }
                    let body = raw.split_once('>').map_or("", |(_, body)| body.rsplit_once("</").map_or(body, |(body, _)| body));
                    if let Some(url) = reference_url(body) { self.cell.reference = Some(url); }
                    if let Some(name) = attrs.get("name").filter(|name| !name.is_empty()) {
                        if let Some(url) = &self.cell.reference { self.named.insert((*name).into(), url.clone()); }
                    }
                    // OpenRefine suppresses ordinary text after a reference until the next cell.
                    self.suppressed = true;
                }
                Node::StartTag { name, start, end } | Node::EndTag { name, start, end } if name == "br" => {
                    let original = self.text[*start..*end].trim_start_matches(['<', '/']).split([' ', '\t', '\n', '/', '>']).next();
                    if original == Some("br") { self.write("\n"); }
                }
                Node::Template { name, parameters, .. } if self.raw => {
                    self.write("{{");
                    self.nodes(name, depth + 1)?;
                    for arg in parameters {
                        self.write("|");
                        // The reference importer visits the argument's children, omitting its name.
                        self.nodes(&arg.value, depth + 1)?;
                    }
                    self.write("}}");
                }
                Node::Image { target, .. } if self.raw => self.write(&format!("[[{target}]]")),
                Node::ParagraphBreak { start, end } => self.write(&self.text[*start..*end]),
                Node::Table { .. } => return Err("Nested wikitext tables are not supported (OpenRefine 3.10.1 also fails to import them)".into()),
                // Like WikitextTableVisitor, do not descend into lists, entities, nowiki or other extensions.
                _ => {}
            }
        }
        Ok(())
    }
}

fn render(text: &str, nodes: &[Node<'_>], raw: bool, named: &mut HashMap<String, String>) -> Result<Cell> {
    let mut r = Renderer { text, raw, named, cell: Cell::default(), output: String::new(), suppressed: false };
    r.nodes(nodes, 0)?;
    r.cell.value = Some(r.output.trim().to_string());
    Ok(r.cell)
}

fn span_size(cell: &TableCell<'_>, text: &str) -> (usize, usize) {
    let raw = cell.attributes.as_ref().and_then(|a| Some(&text[a.first()?.start()..a.last()?.end()]));
    let attrs = attributes(raw.unwrap_or(""));
    let span = |name| attrs.get(name).and_then(|v| v.parse::<i32>().ok()).filter(|v| *v > 1).unwrap_or(1) as usize;
    (span("rowspan"), span("colspan"))
}

fn add_spans(row: &mut Vec<Cell>, spans: &mut Vec<Span>, index: &mut usize, row_id: usize, blank: bool, budget: &mut Budget) -> Result<()> {
    while *index < spans.len() && row.len() >= spans[*index].col {
        let span = &spans[*index];
        if span.row + span.rows > row_id {
            let width = span.col.checked_add(span.cols).filter(|n| *n <= MAX_COLUMNS).ok_or("Wikitext spans exceed 10,000 columns")?;
            while row.len() < width {
                if row.len() % 64 == 0 { jobs::tick(0)?; }
                let cell = if blank { Cell::default() } else { budget.cell(&span.cell)?; span.cell.clone() };
                if blank { budget.cell(&cell)?; }
                row.push(cell);
            }
        }
        if span.row + span.rows <= row_id + 1 { spans.remove(*index); } else { *index += 1; }
    }
    Ok(())
}

fn table_rows(text: &str, tables: &[&Vec<TableRow<'_>>], o: &ImportOptions) -> Result<Vec<Vec<Cell>>> {
    let mut out = vec![];
    let mut spans = vec![];
    let mut named = HashMap::new();
    let mut budget = Budget::default();
    for table in tables {
        for source in *table {
            jobs::tick(0)?;
            let mut row = vec![];
            let mut index = 0;
            add_spans(&mut row, &mut spans, &mut index, out.len(), o.blank_spanning_cells, &mut budget)?;
            for (i, source) in source.cells.iter().enumerate() {
                if i % 64 == 0 { jobs::tick(0)?; }
                let cell = render(text, &source.content, o.include_raw_templates, &mut named)?;
                let col = row.len();
                let (rows, cols) = span_size(source, text);
                budget.cell(&cell)?;
                row.push(cell.clone());
                if rows > 1 || cols > 1 { spans.insert(index, Span { cell, row: out.len(), col, rows, cols }); }
                add_spans(&mut row, &mut spans, &mut index, out.len(), o.blank_spanning_cells, &mut budget)?;
                if row.len() > MAX_COLUMNS { return Err("Wikitext table exceeds 10,000 columns".into()); }
            }
            if !row.is_empty() { out.push(row); }
        }
    }
    Ok(out)
}

fn literal_end(text: &str, at: usize) -> Option<usize> {
    let tail = &text[at..];
    if let Some(comment) = tail.strip_prefix("<!--") {
        return Some(comment.find("-->").map_or(text.len(), |end| at + 4 + end + 3));
    }
    let bytes = tail.as_bytes();
    for name in [b"nowiki".as_slice(), b"math".as_slice()] {
        let end_name = name.len() + 1;
        if bytes.first() != Some(&b'<') || !bytes.get(1..end_name).is_some_and(|s| s.eq_ignore_ascii_case(name))
            || !bytes.get(end_name).is_some_and(|c| matches!(c, b' ' | b'\t' | b'\n' | b'/' | b'>')) { continue; }
        let opening_end = at + bytes.iter().position(|&c| c == b'>')? + 1;
        if text.as_bytes()[opening_end - 2] == b'/' { return Some(opening_end); }
        static CLOSE: OnceLock<Regex> = OnceLock::new();
        let close = CLOSE.get_or_init(|| Regex::new(r"(?i)</(nowiki|math)[\t\n ]*>").unwrap());
        return close.captures_iter(&text[opening_end..]).find(|c| c[1].as_bytes().eq_ignore_ascii_case(name))
            .map(|c| opening_end + c.get(0).unwrap().end());
    }
    None
}

fn check_markup(text: &str) -> Result<()> {
    let bytes = text.as_bytes();
    let mut templates = vec![];
    let mut at = 0;
    while at < bytes.len() {
        if at % 4096 == 0 { jobs::tick(0)?; }
        if bytes[at] == b'<' {
            if let Some(end) = literal_end(text, at) { at = end; continue; }
        }
        // The parser makes one nested AST node per leading list marker. Its recursive Drop
        // runs before rendering, so the renderer's depth check cannot protect this boundary.
        if (at == 0 || bytes[at - 1] == b'\n')
            && bytes[at..].iter().take(MAX_NESTING + 1).take_while(|b| matches!(b, b'#' | b'*' | b':' | b';')).count() > MAX_NESTING {
                return Err("Wikitext lists exceed 128 nested elements".into());
        }
        if bytes[at..].starts_with(b"{{") {
            let width = if bytes.get(at + 2) == Some(&b'{') { 3 } else { 2 };
            templates.push(width);
            if templates.len() > MAX_NESTING { return Err("Wikitext templates exceed 128 nested elements".into()); }
            at += width;
        } else if bytes[at..].starts_with(b"}}") {
            if let Some(width) = templates.pop() {
                if width == 3 && bytes.get(at + 2) != Some(&b'}') { return Err("Wikitext template/parameter closing delimiters do not match".into()); }
                at += width;
            } else { at += 2; }
        } else { at += 1; }
    }
    // Unclosed nested templates trigger the foreign parser's overlapping EOF rewinds, which
    // can grow its warnings exponentially. Reject malformed delimiters before entering it.
    if !templates.is_empty() { return Err("Wikitext template/parameter has no closing delimiter".into()); }
    Ok(())
}

fn source(path: &Path, o: &ImportOptions) -> Result<String> {
    let text = decode(path, &o.encoding)?;
    if text.len() > 32 * 1024 * 1024 { return Err("Wikitext input exceeds 32 MiB".into()); }
    let mut text = text.replace("\r\n", "\n").replace('\r', "\n");
    check_markup(&text)?;
    // Sweble accepts tables ended by EOF; this parser otherwise rewinds them to plain text.
    let missing = Configuration::default().parse(&text).warnings.iter().filter(|w|
        w.message == WarningMessage::MissingEndTagRewinding && text[w.start..].starts_with("{|")) .count();
    if missing > 0 { text.push_str(&"\n|}\n".repeat(missing)); }
    jobs::tick(0)?;
    Ok(text)
}

pub(super) fn inspect(path: &Path, o: &ImportOptions) -> Result<J> {
    let text = source(path, o)?;
    let parsed = Configuration::default().parse(&text);
    let mut tables = vec![];
    for node in &parsed.nodes {
        if let Node::Table { rows, captions, .. } = node {
            jobs::tick(0)?;
            let caption = captions.last().map(|c| render(&text, &c.content, false, &mut HashMap::new()).map(|c| c.value)).transpose()?.flatten();
            let expanded = table_rows(&text, &[rows], o)?;
            tables.push(json!({ "index": tables.len(), "caption": caption,
                "rows": expanded.len(), "columns": expanded.iter().map(Vec::len).max().unwrap_or(0) }));
        }
    }
    Ok(json!({ "tables": tables, "wikiBaseUrl": WIKI_BASE,
        "options": { "blankSpanningCells": true, "includeRawTemplates": false,
            "parseReferences": true, "wikiUrl": null, "reconService": null } }))
}

fn http_url(value: &str, name: &str) -> Result<()> {
    let url = url::Url::parse(value).map_err(|e| format!("Invalid {name}: {e}"))?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() || !url.username().is_empty() || url.password().is_some() {
        return Err(format!("{name} must be an HTTP or HTTPS URL without credentials"));
    }
    Ok(())
}

fn reconcile(rows: &mut [Vec<Cell>], wiki: &str, service: &str) -> Result<StandardConfig> {
    http_url(wiki, "wiki page prefix")?;
    http_url(service, "reconciliation service")?;
    let cfg = StandardConfig::from_json(&json!({ "service": service,
        "identifierSpace": "http://www.wikidata.org/entity/", "schemaSpace": "http://www.wikidata.org/prop/direct/",
        "type": { "id": "", "name": "entity" }, "autoMatch": true, "limit": 1 }))?;
    let mut links = vec![];
    for (r, row) in rows.iter().enumerate() {
        for (c, cell) in row.iter().enumerate() {
            if let [target] = cell.links.as_slice() {
                if links.len() >= MAX_LINKS { return Err("Wikitext reconciliation exceeds 100,000 linked cells".into()); }
                links.push((r, c, format!("{wiki}{target}")));
            }
        }
    }
    let agent = ureq::Agent::new_with_config(ureq::Agent::config_builder()
        .max_redirects(3).timeout_connect(Some(Duration::from_secs(10))).timeout_global(Some(Duration::from_secs(30)))
        .user_agent(concat!("OxRefine/", env!("CARGO_PKG_VERSION"))).build());
    jobs::progress("Resolving wikitext links", links.len() as u64);
    for batch in links.chunks(50) {
        jobs::tick(0)?;
        let queries: serde_json::Map<String, J> = batch.iter().enumerate().map(|(i, (_, _, q))| (format!("q{i}"), json!({ "query": q }))).collect();
        let body = J::Object(queries).to_string();
        if body.len() > 8 * 1024 * 1024 { return Err("Wikitext reconciliation query batch exceeds 8 MiB".into()); }
        let mut response = agent.post(service).send_form([("queries", body)]).map_err(|e| format!("Wikitext reconciliation request failed: {e}"))?;
        let body = response.body_mut().with_config().limit(8 * 1024 * 1024).read_to_string().map_err(|e| format!("Wikitext reconciliation response exceeded its limits or could not be read: {e}"))?;
        jobs::tick(0)?;
        let reply: J = serde_json::from_str(&body).map_err(|e| format!("Invalid wikitext reconciliation response: {e}"))?;
        for (i, (r, c, query)) in batch.iter().enumerate() {
            let result = reply.get(format!("q{i}")).and_then(|q| q.get("result")).and_then(J::as_array)
                .ok_or_else(|| format!("Reconciliation service omitted results for wikitext query q{i}"))?;
            if result.iter().any(|r| r.get("id").and_then(J::as_str).is_none_or(str::is_empty)
                || r.get("name").and_then(J::as_str).is_none()) {
                return Err("Reconciliation service returned an invalid entity candidate".into());
            }
            let mut recon = cfg.results(query, result, 0);
            recon.service = service.into();
            rows[*r][*c].recon = Some(Arc::new(recon));
        }
        jobs::tick(batch.len() as u64)?;
    }
    Ok(cfg)
}

pub(super) fn read(path: &Path, o: &ImportOptions) -> Result<Table> {
    let text = source(path, o)?;
    let parsed = Configuration::default().parse(&text);
    let all: Vec<_> = parsed.nodes.iter().filter_map(|n| if let Node::Table { rows, .. } = n { Some(rows) } else { None }).collect();
    let mut selected = HashSet::new();
    for &i in &o.table_indexes {
        if i >= all.len() { return Err(format!("No wikitext table at index {i}; found {} tables", all.len())); }
        if !selected.insert(i) { return Err(format!("Wikitext table index {i} is selected more than once")); }
    }
    let tables: Vec<_> = all.into_iter().enumerate().filter_map(|(i, t)| (selected.is_empty() || selected.contains(&i)).then_some(t)).collect();
    let mut rows = table_rows(&text, &tables, o)?;
    let config = o.wiki_url.as_deref().filter(|url| !url.trim().is_empty()).map(|wiki|
        reconcile(&mut rows, wiki.trim(), o.recon_service.as_deref().unwrap_or(SERVICE).trim())).transpose()?;
    if rows.len().saturating_sub(o.ignore_lines.max(0) as usize) < o.header_lines.max(0) as usize {
        return Ok(Table::default());
    }
    let width = rows.iter().map(Vec::len).max().unwrap_or(0);
    let mut referenced = vec![false; width];
    let mut original_configured = vec![false; width];
    for row in &rows {
        jobs::tick(0)?;
        for (c, cell) in row.iter().enumerate() {
            referenced[c] |= o.parse_references && cell.reference.is_some();
            original_configured[c] |= cell.recon.is_some();
        }
    }
    let output_width = width + referenced.iter().filter(|&&b| b).count();
    if output_width > MAX_COLUMNS { return Err("Wikitext table with reference columns exceeds 10,000 columns".into()); }
    let first_data = (o.ignore_lines.max(0) + o.header_lines.max(0) + o.skip_data_lines.max(0)) as usize;
    let data_rows = rows.len().saturating_sub(first_data);
    let data_rows = if o.limit > 0 { data_rows.min(o.limit as usize) } else { data_rows };
    if output_width.checked_mul(data_rows).is_none_or(|n| n > MAX_CELLS) {
        return Err("Wikitext table exceeds 5,000,000 cells after padding short rows".into());
    }
    let mut configured = vec![];
    for (c, &reference) in referenced.iter().enumerate() {
        configured.push(original_configured[c]);
        if reference { configured.push(false); }
    }
    let mut options = o.clone();
    options.limit = if options.limit == 0 { -1 } else { options.limit };
    options.store_blank_rows = true; // The reference's Cell("") counts as data.
    let mut builder = Builder::new(&options);
    let mut emitted = vec![];
    for row in &rows {
        jobs::tick(0)?;
        let mut fields = vec![];
        let mut recons = vec![];
        let mut has_data = false;
        for (c, cell) in row.iter().enumerate() {
            if c % 64 == 0 { jobs::tick(0)?; }
            has_data |= cell.value.is_some();
            fields.push(Field::Val(cell.value.clone().map(Value::Str).unwrap_or_else(|| if o.store_blank_cells_as_nulls { Value::Null } else { Value::Str(String::new()) })));
            recons.push(cell.recon.clone());
            if referenced[c] {
                has_data |= cell.reference.is_some();
                fields.push(Field::Val(cell.reference.clone().map(Value::Str).unwrap_or_else(|| if o.store_blank_cells_as_nulls { Value::Null } else { Value::Str(String::new()) })));
                recons.push(None);
            }
        }
        if builder.line >= first_data && !has_data && !o.store_blank_rows { builder.line += 1; continue; }
        let before = builder.rows;
        if !builder.push(fields.into_iter()) { break; }
        if builder.rows > before { emitted.push(recons); }
    }
    let mut table = builder.finish()?;
    for c in 0..table.columns.len() {
        if c % 64 == 0 { jobs::tick(0)?; }
        let cells: Vec<_> = emitted.iter().map(|row| row.get(c).cloned().flatten()).collect();
        if configured.get(c).copied().unwrap_or(false) || cells.iter().any(Option::is_some) {
            let col = Arc::get_mut(&mut table.columns[c]).expect("newly imported column is owned");
            col.recon = Some(Arc::new(ReconCol { cells, config: config.as_ref().map(StandardConfig::to_json), stats: None }));
        }
    }
    if !o.store_blank_columns {
        let keep: Vec<_> = table.columns.iter().enumerate().filter_map(|(c, col)| (0..table.rows).any(|r| !col.get(r).is_blank()).then_some(c)).collect();
        table.names = keep.iter().map(|&c| table.names[c].clone()).collect();
        table.columns = keep.iter().map(|&c| table.columns[c].clone()).collect();
        table.key = table.names.first().cloned();
    }
    Ok(table)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deeply_nested_lists_fail_before_the_parser_can_build_a_recursive_drop_tree() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("fuzz.wiki");
        // Preserved default-seed fuzz input, SHA-256 139667ed86a317955bf76f1cefbe6a6058063deb2b760f2f4d620fd8de8d5d8f.
        let input = include_bytes!("../../../tests/wikitext/regressions/definition-list-stack-overflow.wiki");
        std::fs::write(&path, input).unwrap();
        assert!(crate::import::inspect(&path).unwrap_err().contains("128 nested"));
        assert!(crate::import::import(&path, Some("wikitext"), &ImportOptions::default()).err().unwrap().contains("128 nested"));
        for marker in [";", ":", "#", "*", ";:#*"] {
            let text = format!("{{|\n! A\n|-\n| row\n{}text\n|}}", marker.repeat(4096));
            std::fs::write(&path, text).unwrap();
            assert!(inspect(&path, &ImportOptions::default()).unwrap_err().contains("128 nested"));
            assert!(read(&path, &ImportOptions::default()).err().unwrap().contains("128 nested"));
        }
    }

    #[test]
    fn list_depth_guard_preserves_ordinary_markers_and_the_supported_boundary() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ordinary.wiki");
        let punctuation = ";:#*".repeat(1024);
        let text = format!("{{|\n! A\n|-\n| data {punctuation}\n|}}\n");
        std::fs::write(&path, &text).unwrap();
        let table = read(&path, &ImportOptions::default()).unwrap();
        assert_eq!(table.rows, 1);
        assert_eq!(table.columns[0].get(0).as_str(), Some(format!("data {punctuation}").as_str()));
        for prefix in ["", " ", "\t"] {
            let text = format!("{prefix}{} term\n", ";".repeat(if prefix.is_empty() { MAX_NESTING } else { 4096 }));
            std::fs::write(&path, text).unwrap();
            assert!(inspect(&path, &ImportOptions::default()).is_ok(), "ordinary/preformatted marker input must remain accepted");
        }
        assert!(check_markup(&format!("{} term\n", ";".repeat(MAX_NESTING + 1))).is_err());
    }

    #[test]
    fn template_rewind_fuzz_input_and_malformed_delimiters_fail_before_parsing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rewind.wiki");
        // Preserved fuzz seed, SHA-256 f5016b8f3784106191d4e20263fc5ce9e0667d589e3fc3c22a0ac60908968306.
        std::fs::write(&path, include_bytes!("../../../tests/wikitext/regressions/template-rewind-memory.wiki")).unwrap();
        let started = std::time::Instant::now();
        assert!(crate::import::inspect(&path).unwrap_err().contains("128 nested"));
        assert!(crate::import::import(&path, Some("wikitext"), &ImportOptions::default()).err().unwrap().contains("128 nested"));
        assert!(started.elapsed() < Duration::from_secs(2), "a tiny malformed source must not enter exponential parser rewinds");
        for text in ["{{T", "{{{p}}", "{{T|{{Inner|x}}", "{{{p|{{Inner|x}}"] {
            std::fs::write(&path, text).unwrap();
            assert!(inspect(&path, &ImportOptions::default()).unwrap_err().contains("delimiter"), "{text}");
            assert!(read(&path, &ImportOptions::default()).err().unwrap().contains("delimiter"), "{text}");
        }
    }

    #[test]
    fn template_depth_boundary_and_literal_blocks_are_preserved() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("balanced.wiki");
        for depth in [MAX_NESTING - 1, MAX_NESTING] {
            for (open, close) in [("{{T|", "}}"), ("{{{p|", "}}}")] {
                let text = format!("{}x{}", open.repeat(depth), close.repeat(depth));
                std::fs::write(&path, &text).unwrap();
                assert!(inspect(&path, &ImportOptions::default()).is_ok(), "{depth} balanced {open} levels");
                assert!(read(&path, &ImportOptions::default()).is_ok());
            }
        }
        assert!(check_markup(&format!("{}x{}", "{{T|".repeat(MAX_NESTING + 1), "}}".repeat(MAX_NESTING + 1))).is_err());
        for text in [
            "plain { brace and unmatched }} }} } remain text",
            "<!-- {{ unclosed parameter {{{ and }}} -->",
            "<nowiki>{{ unclosed literal {{{</nowiki>",
            "<NoWiKi class=x>{{{ literal</NOWIKI >",
            "<math>{{ literal formula</math>",
            "{{Outer|{{{p|{{Inner|x}}}}}}}",
        ] {
            std::fs::write(&path, text).unwrap();
            assert!(inspect(&path, &ImportOptions::default()).is_ok(), "{text}");
        }
        let literal = format!("<nowiki>\n{}\n{}\n</nowiki>", ";".repeat(4096), "{".repeat(4096));
        std::fs::write(&path, literal).unwrap();
        assert!(inspect(&path, &ImportOptions::default()).is_ok(), "literal list markers and braces are not nested syntax");
    }

    #[test]
    fn cancellation_stops_span_expansion_before_cloning_text() {
        let mut budget = Budget::default();
        let result = jobs::with_owner(Some("wikitext-span-test"), || jobs::run_result("Expanding spans", || {
            jobs::cancel_owned("wikitext-span-test", 0);
            let mut row = vec![];
            let mut spans = vec![Span { cell: Cell { value: Some("x".repeat(1024)), ..Cell::default() }, row: 0, col: 0, rows: 2, cols: 1000 }];
            add_spans(&mut row, &mut spans, &mut 0, 1, false, &mut budget)
        }));
        assert!(result.as_ref().is_err_and(|e| jobs::is_cancelled(e)));
        assert_eq!(budget.cells, 0);
    }
}
