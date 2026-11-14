//! Bounded JSON path discovery. Counts describe completed nodes in this sample, never extrapolated.
use super::{format_for, pick, Format, ImportOptions};
use crate::{err, jobs, Result};
use serde::de::{DeserializeSeed, Error, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Serialize};
use std::{fs::File, io::{BufRead, BufReader, Read}, path::Path};

const BYTES: u64 = 256 * 1024;
const NODES: usize = 2000;
const DEPTH: usize = 16;
const PATHS: usize = 256;
const RECORDS: usize = 100;
const STOP: &str = "JSON path sample limit";

/// A streaming encoding decoder shared by discovery and JSON Lines import.
pub(super) fn reader(path: &Path, label: &str) -> Result<BufReader<Decoded>> {
    let encoding = if label.is_empty() { encoding_rs::UTF_8 } else {
        encoding_rs::Encoding::for_label(label.as_bytes()).ok_or_else(|| format!("unknown encoding {label}"))?
    };
    let file = File::open(path).map_err(err)?;
    jobs::progress_units("Reading JSON", file.metadata().map_or(0, |m| m.len()), "bytes");
    Ok(BufReader::new(Decoded { source: jobs::Counted(file), decoder: encoding.new_decoder_with_bom_removal(),
        input: [0; 8192], at: 0, len: 0, output: [0; 8192], out_at: 0, out_len: 0, eof: false, finished: false }))
}

pub(super) struct Decoded {
    source: jobs::Counted<File>, decoder: encoding_rs::Decoder,
    input: [u8; 8192], at: usize, len: usize,
    output: [u8; 8192], out_at: usize, out_len: usize,
    eof: bool, finished: bool,
}
impl Read for Decoded {
    fn read(&mut self, target: &mut [u8]) -> std::io::Result<usize> {
        if target.is_empty() { return Ok(0); }
        while self.out_at == self.out_len {
            if self.finished { return Ok(0); }
            if self.at == self.len && !self.eof {
                self.len = self.source.read(&mut self.input)?;
                self.at = 0;
                self.eof = self.len == 0;
            }
            let (result, read, written, _) = self.decoder.decode_to_utf8(&self.input[self.at..self.len], &mut self.output, self.eof);
            self.at += read;
            self.out_at = 0;
            self.out_len = written;
            self.finished = self.eof && result == encoding_rs::CoderResult::InputEmpty;
        }
        let n = target.len().min(self.out_len - self.out_at);
        target[..n].copy_from_slice(&self.output[self.out_at..self.out_at + n]);
        self.out_at += n;
        Ok(n)
    }
}

#[derive(Serialize)]
struct Candidate {
    path: Vec<String>, count: usize, #[serde(rename = "type")] kind: String,
    fields: Vec<String>, #[serde(skip)] array_item: bool,
}
#[derive(Default)]
struct Discovery { paths: Vec<Candidate>, nodes: usize, sampled: bool }
impl Discovery {
    fn note<E: Error>(&mut self, path: &[String], kind: &str, item: bool) -> std::result::Result<usize, E> {
        if let Some(i) = self.paths.iter().position(|p| p.path == path) {
            let entry = &mut self.paths[i];
            if entry.kind == "empty" { entry.kind = kind.into(); }
            else if kind != "empty" && entry.kind != kind { entry.kind = "mixed".into(); }
            entry.array_item |= item;
            return Ok(i);
        }
        if self.paths.len() == PATHS { self.sampled = true; return Err(E::custom(STOP)); }
        self.paths.push(Candidate { path: path.to_vec(), count: 0, kind: kind.into(), fields: vec![], array_item: item });
        Ok(self.paths.len() - 1)
    }
}
struct Node<'a> { state: &'a mut Discovery, path: Vec<String>, item: bool }
impl<'de> DeserializeSeed<'de> for Node<'_> {
    type Value = ();
    fn deserialize<D: serde::Deserializer<'de>>(self, de: D) -> std::result::Result<(), D::Error> {
        if self.state.nodes == NODES { self.state.sampled = true; return Err(D::Error::custom(STOP)); }
        self.state.nodes += 1;
        if self.path.len() > DEPTH {
            self.state.sampled = true;
            return serde::de::IgnoredAny::deserialize(de).map(|_| ());
        }
        de.deserialize_any(self)
    }
}
impl Node<'_> {
    fn scalar<E: Error>(self, kind: &str) -> std::result::Result<(), E> {
        // Scalar object fields are columns; containers and array items are record candidates.
        if self.item || self.path.len() == 1 {
            let i = self.state.note(&self.path, kind, self.item)?;
            self.state.paths[i].count += 1;
        }
        Ok(())
    }
}
impl<'de> Visitor<'de> for Node<'_> {
    type Value = ();
    fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result { f.write_str("JSON value") }
    fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> std::result::Result<(), M::Error> {
        let i = self.state.note(&self.path, "object", self.item)?;
        while let Some(key) = map.next_key::<String>()? {
            if key.chars().count() > 256 {
                self.state.sampled = true;
                map.next_value::<serde::de::IgnoredAny>()?;
                continue;
            }
            if self.state.paths[i].fields.len() < 8 && !self.state.paths[i].fields.contains(&key) { self.state.paths[i].fields.push(key.clone()); }
            let mut path = self.path.clone(); path.push(key);
            map.next_value_seed(Node { state: self.state, path, item: false })?;
        }
        self.state.paths[i].count += 1;
        Ok(())
    }
    fn visit_seq<S: SeqAccess<'de>>(self, mut seq: S) -> std::result::Result<(), S::Error> {
        let i = self.state.note(&self.path, "array", self.item)?;
        let mut path = self.path.clone(); path.push("_".into());
        self.state.note::<S::Error>(&path, "empty", true)?;
        while seq.next_element_seed(Node { state: self.state, path: path.clone(), item: true })?.is_some() {}
        self.state.paths[i].count += 1;
        Ok(())
    }
    fn visit_bool<E: Error>(self, _: bool) -> std::result::Result<(), E> { self.scalar("value") }
    fn visit_i64<E: Error>(self, _: i64) -> std::result::Result<(), E> { self.scalar("value") }
    fn visit_u64<E: Error>(self, _: u64) -> std::result::Result<(), E> { self.scalar("value") }
    fn visit_f64<E: Error>(self, _: f64) -> std::result::Result<(), E> { self.scalar("value") }
    fn visit_str<E: Error>(self, _: &str) -> std::result::Result<(), E> { self.scalar("value") }
    fn visit_unit<E: Error>(self) -> std::result::Result<(), E> { self.scalar("null") }
}

/// Record candidates from the first selected source, bounded independently of import row limits.
pub fn json_paths(path: &Path, format: Option<&str>, o: &ImportOptions) -> Result<serde_json::Value> {
    let u = crate::unpack::unpack(path)?;
    let (picked, _) = pick(&u, o)?;
    let first = picked.first().ok_or("no files to inspect")?.1;
    let lines = format_for(first, format) == Format::JsonLines;
    // Look ahead only far enough to keep the bounded UTF-8 prefix on a character boundary.
    let mut bytes = vec![];
    reader(first, &o.encoding)?.take(BYTES + 4).read_to_end(&mut bytes).map_err(err)?;
    let byte_sampled = bytes.len() > BYTES as usize;
    let mut end = bytes.len().min(BYTES as usize);
    if let Err(e) = std::str::from_utf8(&bytes[..end]) { end = e.valid_up_to(); }
    let prefix = std::str::from_utf8(&bytes[..end]).map_err(err)?;
    let mut state = Discovery::default();
    let mut records = 0;
    let outcome = if lines {
        let mut input = std::io::Cursor::new(prefix);
        let mut line = String::new();
        loop {
            line.clear();
            if records == RECORDS { state.sampled = true; break Ok(()); }
            if input.read_line(&mut line).map_err(err)? == 0 { break Ok(()); }
            if line.trim().is_empty() { continue; }
            records += 1;
            let mut de = serde_json::Deserializer::from_str(&line);
            let result = Node { state: &mut state, path: vec!["_".into()], item: true }.deserialize(&mut de).and_then(|_| de.end());
            if result.is_err() { break result; }
        }
    } else {
        let mut de = serde_json::Deserializer::from_str(prefix);
        Node { state: &mut state, path: vec!["_".into()], item: false }.deserialize(&mut de).and_then(|_| de.end())
    };
    if let Err(e) = outcome {
        if e.to_string().starts_with(STOP) || (byte_sampled && e.is_eof()) { state.sampled = true; }
        else { return Err(format!("JSON source: {e}")); }
    }
    state.sampled |= byte_sampled;
    let suggested = if lines { vec!["_".to_string()] } else {
        state.paths.iter().filter(|p| p.array_item && p.kind == "object")
            .min_by_key(|p| (p.path.len(), std::cmp::Reverse(p.count)))
            .or_else(|| state.paths.iter().find(|p| p.path.len() == 2 && p.array_item))
            .or_else(|| state.paths.first()).map_or_else(|| vec!["_".into()], |p| p.path.clone())
    };
    Ok(serde_json::json!({ "paths": state.paths, "suggestedPath": suggested, "sampled": state.sampled, "file": picked[0].0,
        "sampledRecords": records, "jsonLinesRoot": lines,
        "limits": { "bytes": BYTES, "nodes": NODES, "depth": DEPTH, "paths": PATHS, "records": RECORDS, "keyCharacters": 256 } }))
}
