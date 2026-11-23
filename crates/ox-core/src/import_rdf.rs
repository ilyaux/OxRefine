//! RDF files (N-Triples, N3, Turtle, RDF/XML, JSON-LD) as OpenRefine's RdfTripleImporter reads them
//! through Apache Jena: a `subject` column, then one column per predicate IRI. Each subject is a
//! record; a predicate with several values continues on the record's next rows. Literals show as
//! Jena prints them: the lexical form with `"` escaped, then `@lang` or `^^datatype` (none for
//! xsd:string). Identical triples count once, as in Jena's graph.
//!
//! Jena orders rows and columns by its in-memory hash tables and names blank nodes with random
//! ids. OxRefine keeps the order of the file and gives blank nodes stable ids of the same shape.
use super::{Format, ImportOptions};
use crate::{err, grel::Value, table::{Column, Table}, Result};
use md5::{Digest, Md5};
use oxjsonld::{JsonLdLoadDocumentOptions, JsonLdParser, JsonLdRemoteDocument};
use oxrdf::{vocab::xsd, NamedOrBlankNode, Quad, Term};
use oxrdfio::{RdfFormat, RdfParser};
use std::{collections::{HashMap, HashSet}, io::Read, path::Path};

/// A remote JSON-LD context, as Jena loads it: over HTTP(S), at most 8 MiB.
fn fetch_context(url: &str, _: &JsonLdLoadDocumentOptions) -> std::result::Result<JsonLdRemoteDocument, Box<dyn std::error::Error + Send + Sync>> {
    let parsed = url::Url::parse(url)?;
    if !matches!(parsed.scheme(), "http" | "https") { return Err(format!("{url}: only http and https contexts are loaded").into()); }
    let mut response = ureq::get(url).header("Accept", "application/ld+json, application/json;q=0.9").call()?;
    let mut document = vec![];
    response.body_mut().as_reader().take(8 << 20).read_to_end(&mut document)?;
    Ok(JsonLdRemoteDocument { document, document_url: url.to_string() })
}

/// OpenRefine's format ids, OxRefine's names and OpenRefine's file extensions.
pub(super) fn format(name: &str) -> Option<Format> {
    Some(match name {
        "text/rdf/nt" | "rdf-ntriples" | "nt" | "ntriples" => Format::RdfNTriples,
        "text/rdf/n3" | "rdf-n3" | "n3" => Format::RdfN3,
        "text/rdf/ttl" | "rdf-turtle" | "ttl" | "turtle" => Format::RdfTurtle,
        "text/rdf/xml" | "rdf-xml" | "rdf" => Format::RdfXml,
        "text/rdf/ld+json" | "json-ld" | "jsonld" => Format::JsonLd,
        _ => return None,
    })
}

pub(super) fn read(path: &Path, format: Format, o: &ImportOptions) -> Result<Table> {
    let text = super::decode(path, &o.encoding)?;
    super::parsing(o);
    // Jena resolves relative IRIs against OpenRefine's working folder; the file's own folder is the stable equivalent
    let base = path.canonicalize().ok().as_deref().and_then(Path::parent).and_then(|d| url::Url::from_directory_path(d).ok()).map(String::from);
    let quads: Box<dyn Iterator<Item = std::result::Result<Quad, String>> + '_> = if format == Format::JsonLd {
        // Jena fetches a @context given by a web address
        let mut parser = JsonLdParser::new();
        if let Some(base) = &base { parser = parser.with_base_iri(base).map_err(err)?; }
        Box::new(parser.for_slice(text.as_bytes()).with_load_document_callback(fetch_context).map(|q| q.map_err(|e| e.to_string())))
    } else {
        let mut parser = RdfParser::from_format(match format {
            Format::RdfNTriples => RdfFormat::NTriples,
            Format::RdfXml => RdfFormat::RdfXml,
            // Jena reads N3 as Turtle
            _ => RdfFormat::Turtle,
        });
        if let Some(base) = &base { parser = parser.with_base_iri(base).map_err(err)?; }
        Box::new(parser.for_slice(text.as_bytes()).map(|q| q.map_err(|e| e.to_string())))
    };
    let salt = Md5::digest(text.as_bytes());
    let mut blanks: HashMap<String, String> = HashMap::new();
    let mut blank = |label: &str| {
        let n = blanks.len();
        blanks.entry(label.to_string()).or_insert_with(|| format!("{:x}", Md5::new().chain_update(salt).chain_update(n.to_le_bytes()).finalize())).clone()
    };
    let mut languages = Languages::new(&text);
    let mut names = vec!["subject".to_string()];
    let mut columns: HashMap<String, usize> = HashMap::new();
    let mut subjects: HashMap<String, usize> = HashMap::new();
    let mut records: Vec<Vec<Vec<Option<String>>>> = vec![];
    let mut seen = HashSet::new();
    for (i, quad) in quads.enumerate() {
        if i % 10_000 == 0 { crate::jobs::tick(0)?; }
        let quad = quad.map_err(|e| format!("{}: {e}", path.display()))?;
        // a model holds the default graph; Jena drops named graphs too
        if !quad.graph_name.is_default_graph() { continue; }
        let subject = match &quad.subject {
            NamedOrBlankNode::NamedNode(n) => n.as_str().to_string(),
            NamedOrBlankNode::BlankNode(b) => blank(b.as_str()),
        };
        let object = match &quad.object {
            Term::NamedNode(n) => n.as_str().to_string(),
            Term::BlankNode(b) => blank(b.as_str()),
            Term::Literal(l) => {
                let value = if l.datatype() == oxrdf::vocab::rdf::XML_LITERAL { used_namespaces(l.value()) } else { l.value().to_string() };
                let value = value.replace('"', "\\\"");
                match l.language() {
                    Some(language) => format!("{value}@{}", languages.original(language)),
                    None if l.datatype() == xsd::STRING => value,
                    None => format!("{value}^^{}", l.datatype().as_str()),
                }
            }
            #[allow(unreachable_patterns)]
            other => other.to_string(),
        };
        let predicate = quad.predicate.as_str();
        if !seen.insert((subject.clone(), predicate.to_string(), object.clone())) { continue; }
        let column = *columns.entry(predicate.to_string()).or_insert_with(|| {
            names.push(predicate.to_string());
            names.len() - 1
        });
        let record = *subjects.entry(subject.clone()).or_insert_with(|| {
            records.push(vec![vec![Some(subject)]]);
            records.len() - 1
        });
        // the record's first row without a value for this predicate, else a new row
        let rows = &mut records[record];
        let row = match rows.iter().position(|row| row.get(column).is_none_or(Option::is_none)) {
            Some(r) => &mut rows[r],
            None => { rows.push(vec![]); rows.last_mut().unwrap() }
        };
        if row.len() <= column { row.resize(column + 1, None); }
        row[column] = Some(object);
    }
    // OpenRefine's RDF importer has no row limit; previews ask for one
    let limit = usize::try_from(o.limit).ok().filter(|&l| l > 0).unwrap_or(usize::MAX);
    let rows: Vec<&Vec<Option<String>>> = records.iter().flatten().take(limit).collect();
    let budget = crate::storage::current_budget();
    let cols = (0..names.len())
        .map(|c| Column::try_from_values(rows.iter().map(|row| row.get(c).cloned().flatten().map_or(Value::Null, |s| Value::Str(s.into()))), &budget))
        .collect::<Result<Vec<_>>>()?;
    Ok(Table::new(names, cols, rows.len()))
}

/// An RDF/XML parseType="Literal" value as Jena keeps it: the RDF/XML parser declares every
/// namespace in scope on the literal's elements, Jena (exclusive canonical XML) only those the
/// markup uses. ponytail: a prefix used anywhere in the literal keeps every declaration of it.
fn used_namespaces(xml: &str) -> String {
    let mut out = xml.to_string();
    let mut from = 0;
    while let Some(at) = out[from..].find(" xmlns:").map(|i| from + i) {
        let name_end = out[at + 7..].find('=').map_or(out.len(), |i| at + 7 + i);
        let prefix = out[at + 7..name_end].to_string();
        let quote = out.as_bytes().get(name_end + 1).copied().unwrap_or(b'"') as char;
        let Some(end) = out[name_end + 2..].find(quote).map(|i| name_end + 2 + i + 1) else { break };
        let rest = format!("{}{}", &out[..at], &out[end..]);
        if rest.contains(&format!("<{prefix}:")) || rest.contains(&format!(" {prefix}:")) || rest.contains(&format!("</{prefix}:")) {
            from = end;
        } else {
            out = rest;
            from = at;
        }
    }
    out
}

/// The parsers lowercase language tags; Jena keeps them as written ("en-GB"). The first spelling of
/// a tag in the text, after `@` (Turtle) or a quote (RDF/XML, JSON-LD), gives it back.
/// ponytail: a tag spelled two ways in one file shows its first spelling everywhere.
struct Languages<'a> { text: &'a str, lower: String, found: HashMap<String, String> }
impl<'a> Languages<'a> {
    fn new(text: &'a str) -> Self { Self { text, lower: text.to_ascii_lowercase(), found: HashMap::new() } }

    fn original(&mut self, tag: &str) -> String {
        let (text, lower) = (self.text, &self.lower);
        self.found.entry(tag.to_string()).or_insert_with(|| {
            let bytes = lower.as_bytes();
            lower.match_indices(tag).find(|&(at, _)| {
                let end = at + tag.len();
                at > 0 && matches!(bytes[at - 1], b'@' | b'"' | b'\'') && bytes.get(end).is_none_or(|b| !b.is_ascii_alphanumeric() && *b != b'-')
            }).map_or_else(|| tag.to_string(), |(at, _)| text[at..at + tag.len()].to_string())
        }).clone()
    }
}
