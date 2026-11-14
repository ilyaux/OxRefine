//! ISO2709 framing and MARC-8/UTF-8 to MARCXML, independently implemented from the MARC standards.
//! Code-point mapping facts and reference provenance live in assets/marc8; no MARC4J code is ported.
use super::ImportOptions;
use crate::{jobs, Result};
use flate2::read::GzDecoder;
use std::{collections::HashMap, fmt::Write as _, fs::File, io::Read, path::Path, sync::OnceLock};

fn mappings() -> &'static HashMap<u32, (char, bool)> {
    static MAP: OnceLock<HashMap<u32, (char, bool)>> = OnceLock::new();
    MAP.get_or_init(|| {
        let mut bytes = vec![];
        GzDecoder::new(&include_bytes!("../assets/marc8/tables.bin.gz")[..]).read_to_end(&mut bytes).expect("bundled MARC-8 mappings");
        assert_eq!(bytes.len() % 9, 0);
        bytes.chunks_exact(9).map(|entry| {
            let key = u32::from_be_bytes(entry[..4].try_into().unwrap());
            let value = u32::from_be_bytes(entry[4..8].try_into().unwrap());
            (key, (char::from_u32(value).expect("bundled MARC-8 Unicode scalar"), entry[8] != 0))
        }).collect()
    })
}

fn marc8(bytes: &[u8]) -> Result<String> {
    let (mut g0, mut g1, mut multibyte) = (0x42u8, 0x45u8, false);
    let mut out = String::new();
    let mut marks = vec![];
    let mut at = 0;
    while at < bytes.len() {
        if at % 4096 == 0 { jobs::tick(0)?; }
        if bytes[at] == 0x1b {
            let start = at;
            at += 1;
            let first = *bytes.get(at).ok_or("incomplete MARC-8 escape at end of value")?;
            at += 1;
            match first {
                b'g' | b'b' | b'p' => { g0 = first; multibyte = false; }
                b's' => { g0 = 0x42; multibyte = false; }
                b'(' | b',' | b')' | b'-' | b'$' => {
                    let mut target_g1 = first == b')' || first == b'-';
                    multibyte = first == b'$';
                    if first == b'$' && bytes.get(at).is_some_and(|c| matches!(c, b'(' | b',' | b')' | b'-')) {
                        target_g1 = matches!(bytes[at], b')' | b'-'); at += 1;
                    }
                    if bytes.get(at) == Some(&b'!') { at += 1; }
                    let set = *bytes.get(at).ok_or("incomplete MARC-8 character set designation")?;
                    at += 1;
                    if !b"BEgbp2NQ34S1".contains(&set) || (multibyte && set != b'1') {
                        return Err(format!("unsupported MARC-8 character set 0x{set:02X} at value byte {start}"));
                    }
                    if target_g1 { g1 = set; } else { g0 = set; }
                }
                _ => return Err(format!("unsupported MARC-8 escape at value byte {start}")),
            }
            continue;
        }
        let (set, code, size) = if multibyte && bytes[at] != b' ' {
            let part = bytes.get(at..at + 3).ok_or("incomplete three-byte MARC-8 EACC character")?;
            (0x31, ((part[0] as u32) << 16) | ((part[1] as u32) << 8) | part[2] as u32, 3)
        } else { (if multibyte || bytes[at] <= 0x7e { g0 } else { g1 }, bytes[at] as u32, 1) };
        let value = if bytes[at] == b' ' { (' ', false) } else {
            *mappings().get(&(((set as u32) << 24) | code)).ok_or_else(|| format!("unmapped MARC-8 code 0x{code:X} in set 0x{set:02X} at value byte {at}"))?
        };
        at += size;
        if value.1 { marks.push(value.0); }
        else {
            out.push(if matches!(value.0, '\r' | '\n') { ' ' } else { value.0 });
            out.extend(marks.drain(..));
        }
    }
    if !marks.is_empty() { return Err("MARC-8 combining mark has no following base character".into()); }
    // MARC's lossless hexadecimal references are distinct from general HTML entities.
    static NCR: OnceLock<regex::Regex> = OnceLock::new();
    let re = NCR.get_or_init(|| regex::Regex::new(r"&#x([0-9A-Fa-f]+);|<U\+([0-9A-Fa-f]{4})>").unwrap());
    Ok(re.replace_all(&out, |capture: &regex::Captures| {
        let hex = capture.get(1).or_else(|| capture.get(2)).unwrap().as_str();
        u32::from_str_radix(hex, 16).ok().and_then(char::from_u32).map_or_else(|| capture[0].to_string(), |c| c.to_string())
    }).into_owned())
}

fn entities(value: String) -> String {
    // Match Marc4j's guarded five-entity repair, including its non-DOTALL suffix.
    let Some(first) = value.find('&') else { return value };
    let tail = &value[first + 1..];
    let letters = tail.bytes().take_while(u8::is_ascii_lowercase).count();
    if tail.as_bytes().get(letters) != Some(&b';') || tail[letters + 1..].chars().any(|c| matches!(c, '\n' | '\r' | '\u{85}' | '\u{2028}' | '\u{2029}')) { return value; }
    value.replace("&lt;", "<").replace("&gt;", ">").replace("&amp;", "&").replace("&apos;", "'").replace("&quot;", "\"")
}

fn escaped(value: &str) -> Result<String> {
    if let Some(c) = value.chars().find(|&c| !matches!(c, '\t' | '\n' | '\r' | '\u{20}'..='\u{d7ff}' | '\u{e000}'..='\u{fffd}' | '\u{10000}'..='\u{10ffff}')) {
        return Err(format!("value contains a character not permitted in XML 1.0: U+{:04X}", c as u32));
    }
    Ok(quick_xml::escape::escape(value).into_owned().replace('\r', "&#13;"))
}

fn number(bytes: &[u8], label: &str) -> Result<usize> {
    if !bytes.iter().all(u8::is_ascii_digit) { return Err(format!("{label} is not decimal ASCII")); }
    Ok(bytes.iter().fold(0, |n, b| n * 10 + (b - b'0') as usize))
}

fn convert(record: &[u8]) -> Result<String> {
    let base = number(&record[12..17], "base address")?;
    if base < 25 || base >= record.len() || (base - 25) % 12 != 0 { return Err("base address does not delimit a 12-byte MARC directory".into()); }
    if record[base - 1] != 0x1e { return Err("directory has no field terminator".into()); }
    if record.last() != Some(&0x1d) { return Err("record has no record terminator".into()); }
    if &record[10..12] != b"22" || &record[20..24] != b"4500" { return Err("only standard MARC indicator/subfield sizes 22 and directory entry map 4500 are supported".into()); }
    let body = &record[base..record.len() - 1];
    let designated = body.windows(2).any(|p| p[0] == 0x1b && b"(,)-$gbsp".contains(&p[1]));
    let use_marc8 = if record[9] == b'a' {
        std::str::from_utf8(body).map_err(|e| format!("invalid UTF-8 at record byte {} (leader declares UTF-8)", base + e.valid_up_to()))?;
        false
    } else { designated || std::str::from_utf8(body).is_err() };
    let decode = |bytes: &[u8]| -> Result<String> {
        let value = if use_marc8 { marc8(bytes)? } else { std::str::from_utf8(bytes).map_err(|e| format!("invalid UTF-8: {e}"))?.to_string() };
        escaped(&entities(value))
    };
    let mut leader = record[..24].to_vec(); leader[9] = b'a';
    let leader = std::str::from_utf8(&leader).map_err(|_| "leader must be ASCII")?;
    if !leader.is_ascii() { return Err("leader must be ASCII".into()); }
    let (mut controls, mut data) = (String::new(), String::new());
    let mut ranges = vec![];
    for entry in record[24..base - 1].chunks_exact(12) {
        jobs::tick(0)?;
        let tag = std::str::from_utf8(&entry[..3]).map_err(|_| "field tag must be ASCII")?;
        if !tag.bytes().all(|b| b.is_ascii_alphanumeric()) { return Err("field tag must have three ASCII letters or digits".into()); }
        let length = number(&entry[3..7], "field length")?;
        let offset = number(&entry[7..12], "field offset")?;
        let end = offset.checked_add(length).ok_or("field range overflow")?;
        if length == 0 || end > body.len() { return Err(format!("field {tag} points outside record data")); }
        if ranges.iter().any(|&(a, b)| offset < b && a < end) { return Err(format!("field {tag} overlaps another field")); }
        ranges.push((offset, end));
        let field = &body[offset..end];
        if field.last() != Some(&0x1e) { return Err(format!("field {tag} has no field terminator")); }
        let field = &field[..field.len() - 1];
        if field.contains(&0x1e) { return Err(format!("field {tag} contains an unexpected field terminator")); }
        let result = (|| -> Result<()> {
            if tag.bytes().all(|b| b.is_ascii_digit()) && tag < "010" {
                write!(controls, "<marc:controlfield tag=\"{tag}\">{}</marc:controlfield>", decode(field)?).unwrap();
            } else {
                if field.len() < 2 { return Err("data field has fewer than two indicators".into()); }
                let ind1 = escaped(std::str::from_utf8(&field[..1]).map_err(|_| "indicator must be ASCII")?)?;
                let ind2 = escaped(std::str::from_utf8(&field[1..2]).map_err(|_| "indicator must be ASCII")?)?;
                write!(data, "<marc:datafield tag=\"{tag}\" ind1=\"{ind1}\" ind2=\"{ind2}\">").unwrap();
                let mut at = 2;
                while at < field.len() {
                    if field[at] != 0x1f { return Err("data precedes the first subfield delimiter".into()); }
                    let code = *field.get(at + 1).ok_or("subfield delimiter has no identifier")?;
                    if !code.is_ascii_alphanumeric() { return Err("subfield identifier must be an ASCII letter or digit".into()); }
                    at += 2;
                    let end = field[at..].iter().position(|&b| b == 0x1f).map_or(field.len(), |n| at + n);
                    if end > at { write!(data, "<marc:subfield code=\"{}\">{}</marc:subfield>", code.to_ascii_lowercase() as char, decode(&field[at..end])?).unwrap(); }
                    at = end;
                }
                data.push_str("</marc:datafield>");
            }
            Ok(())
        })();
        result.map_err(|e| format!("field {tag}: {e}"))?;
    }
    Ok(format!("<marc:record><marc:leader>{leader}</marc:leader>{controls}{data}</marc:record>"))
}

/// Record framing is streamed; the resulting MARCXML is held in memory by the shared XML importer.
/// Encoding comes from MARC's leader/escape sequences, as in OpenRefine; `o.encoding` is ignored.
pub(super) fn xml(path: &Path, _o: &ImportOptions) -> Result<String> {
    let file = File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    jobs::progress_units("Converting MARC records", file.metadata().map_or(0, |m| m.len()), "bytes");
    let mut reader = jobs::Counted(file);
    let mut xml = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?><marc:collection xmlns:marc=\"http://www.loc.gov/MARC21/slim\">");
    let (mut count, mut offset) = (0, 0);
    loop {
        jobs::tick(0)?;
        let mut leader = [0u8; 24];
        let read = reader.read(&mut leader[..1]).map_err(|e| if jobs::is_cancel(&e) { jobs::CANCELLED.into() } else { e.to_string() })?;
        if read == 0 { break; }
        count += 1;
        let context = |message: &str| format!("MARC record {count} at byte {offset}: {message}");
        reader.read_exact(&mut leader[1..]).map_err(|e| if jobs::is_cancel(&e) { jobs::CANCELLED.into() } else { context("truncated 24-byte leader") })?;
        let length = number(&leader[..5], "record length").map_err(|e| context(&e))?;
        if !(26..=99999).contains(&length) { return Err(context("record length must be between 26 and 99999 bytes")); }
        let mut record = vec![0; length]; record[..24].copy_from_slice(&leader);
        reader.read_exact(&mut record[24..]).map_err(|e| if jobs::is_cancel(&e) { jobs::CANCELLED.into() } else { context("record is shorter than its declared length") })?;
        xml.push_str(&convert(&record).map_err(|e| if e == jobs::CANCELLED { e } else { context(&e) })?);
        offset += length;
    }
    if count == 0 { return Err("MARC input contains no ISO2709 records".into()); }
    xml.push_str("</marc:collection>");
    Ok(xml)
}
