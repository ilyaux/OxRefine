//! PC-Axis's tabular view, pinned to the bundled OpenRefine 3.10.1 importer.
//! Metadata names are case-sensitive; labels come from VALUES, not CODES or localized VALUES.
use super::{decode, parsing, Builder, Field, ImportOptions};
use crate::{jobs, table::Table, Result};
use std::{borrow::Cow, collections::HashMap, path::Path};

struct Dimension { name: String, values: Vec<String>, index: usize }
struct Metadata<'a> { lines: Vec<&'a str>, line: usize }
impl Metadata<'_> {
    fn current(&self) -> Option<&str> { self.lines.get(self.line).copied() }
    fn values(&mut self, mut pos: usize) -> Result<Vec<String>> {
        let mut values = vec![];
        while let Some(line) = self.current() {
            jobs::tick(0)?;
            if pos >= line.len() { break; }
            let mut value = String::new();
            if line.as_bytes()[pos] == b'"' {
                loop {
                    let line = self.current().ok_or("PC-Axis: unfinished quoted metadata")?;
                    let close = line[pos + 1..].find('"').map(|i| pos + 1 + i)
                        .ok_or_else(|| format!("PC-Axis: missing closing quotation mark on line {}", self.line + 1))?;
                    value.push_str(&line[pos + 1..close]);
                    pos = close + 1;
                    if pos < line.len() { break; }
                    // PC-Axis joins adjacent quoted fragments at column zero, without a newline.
                    self.line += 1; pos = 0;
                    if !self.current().is_some_and(|s| s.starts_with('"')) { break; }
                }
            } else {
                let end = line.as_bytes()[pos + 1..].iter().position(|b| matches!(b, b',' | b';' | b' ')).map_or(line.len(), |i| pos + 1 + i);
                value.push_str(&line[pos..end]); pos = end;
            }
            values.push(value);
            let Some(line) = self.current() else { break; };
            if pos == line.len() { self.line += 1; pos = 0; }
            else { match line.as_bytes()[pos] {
                b';' | b')' => break,
                b',' | b' ' | b'-' => { pos += 1; if pos == line.len() { self.line += 1; pos = 0; } },
                _ => return Err(format!("PC-Axis: unrecognized metadata syntax on line {}", self.line + 1)),
            }}
        }
        Ok(values)
    }
}

pub(super) fn read(path: &Path, options: &ImportOptions) -> Result<Table> {
    let encoding = if options.encoding.is_empty() { super::guess_encoding(&super::read_head(path)?) } else { &options.encoding };
    let text = decode(path, encoding)?;
    jobs::tick(0)?;
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
    let text = if text.contains('\r') { Cow::Owned(text.replace("\r\n", "\n").replace('\r', "\n")) } else { Cow::Borrowed(text) };
    let mut metadata = Metadata { lines: text.lines().collect(), line: 0 };
    let mut dimensions = vec![];
    let mut order = vec![];
    while let Some(line) = metadata.current() {
        jobs::tick(0)?;
        if line.starts_with("DATA=") { break; }
        let Some(equal) = line.find('=') else { return Err(format!("PC-Axis: expected metadata or DATA= on line {}", metadata.line + 1)); };
        let key = line[..equal].to_string();
        let values = metadata.values(equal + 1)?;
        if let Some(name) = key.strip_prefix("VALUES(\"").and_then(|s| s.strip_suffix("\")")) {
            if values.is_empty() { return Err(format!("PC-Axis: dimension {name} has no values")); }
            dimensions.push(Dimension { name: name.into(), values, index: 0 });
        } else if key == "STUB" { order.splice(0..0, values); }
        else if key == "HEADING" { order.extend(values); }
        metadata.line += 1;
    }
    let ranks: HashMap<_, _> = order.iter().enumerate().map(|(i, s)| (s.as_str(), i)).collect();
    // The reference only consults ranks when sorting more than one dimension.
    if dimensions.len() > 1 && dimensions.iter().any(|d| !ranks.contains_key(d.name.as_str())) {
        return Err("PC-Axis: every VALUES dimension must be listed in STUB or HEADING".into());
    }
    dimensions.sort_by_key(|d| ranks.get(d.name.as_str()).copied().unwrap_or(0));
    let options = ImportOptions { ignore_lines: -1, header_lines: 1, store_blank_rows: true,
        store_blank_cells_as_nulls: true, limit: if options.limit == 0 { -1 } else { options.limit }, ..options.clone() };
    parsing(&options);
    let mut builder = Builder::new(&options);
    builder.push(dimensions.iter().map(|d| Field::Text(d.name.as_str())).chain(std::iter::once(Field::Text("value"))));
    'data: for (i, line) in metadata.lines[metadata.line.min(metadata.lines.len())..].iter().enumerate() {
        let line = if i == 0 { line.strip_prefix("DATA=").unwrap_or(line) } else { line };
        let mut rest = line;
        while !rest.is_empty() {
            let end = rest.find([' ', '\t', ';']).unwrap_or(rest.len());
            if end > 0 {
                jobs::tick(1)?;
                let fields = dimensions.iter().map(|d| Field::Text(d.values[d.index].as_str()))
                    .chain(std::iter::once(Field::Text(&rest[..end])));
                if !builder.push(fields) { break 'data; }
                // Odometer order is STUB then HEADING, with the rightmost dimension fastest.
                for d in dimensions.iter_mut().rev() {
                    d.index += 1;
                    if d.index < d.values.len() { break; }
                    d.index = 0;
                }
            }
            // Only an ASCII boundary starts whitespace skipping; an embedded em-space is data.
            rest = rest[end..].trim_start_matches(|c: char| matches!(c, '\u{1c}'..='\u{1f}') ||
                (c.is_whitespace() && !matches!(c, '\u{85}' | '\u{a0}' | '\u{2007}' | '\u{202f}')));
            if rest.starts_with(';') { break; }
        }
    }
    builder.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cancelled_metadata_parsing_returns_no_values() {
        let owner = "pcaxis-metadata-cancel";
        let result = jobs::with_owner(Some(owner), || jobs::run_result("Parsing PC-Axis", || {
            let list = jobs::list_owned(owner);
            let id = list.as_array().unwrap().iter().find(|j| j["state"] == "running").unwrap()["id"].as_u64().unwrap();
            assert!(jobs::cancel_owned(owner, id));
            Metadata { lines: vec!["VALUES(\"Kind\")=\"A\",\"B\";"], line: 0 }.values(15)
        }));
        assert!(jobs::is_cancelled(&result.unwrap_err()));
    }
}
