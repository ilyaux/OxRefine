//! A closed set of tabular output charsets. UTF-16 and Latin-1 deliberately do not use
//! encoding_rs's web aliases (UTF-16 encodes as UTF-8 there, and Latin-1 aliases Windows-1252).
use crate::{err, Result};
use serde::Deserialize;
use serde_json::Value;
use std::io::{self, Write};

#[derive(Clone, Copy)]
enum Charset { Utf8, Utf16(bool), Legacy(&'static encoding_rs::Encoding), Latin1, Ascii }

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct Options { encoding: String, bom: Option<bool>, encoding_errors: String }
impl Default for Options {
    fn default() -> Self { Self { encoding: "UTF-8".into(), bom: None, encoding_errors: "error".into() } }
}

pub(super) struct Encoding {
    charset: Charset,
    name: &'static str,
    bom: &'static [u8],
    eager_bom: bool,
    replace: bool,
}
impl Encoding {
    pub fn parse(params: &Value) -> Result<Self> {
        let options: Options = if params.is_null() { Options::default() } else { serde_json::from_value(params.clone()).map_err(err)? };
        let (charset, name, default_bom) = match options.encoding.trim().to_ascii_lowercase().as_str() {
            "utf-8" | "utf8" => (Charset::Utf8, "UTF-8", false),
            "utf-16" | "utf16" => (Charset::Utf16(false), "UTF-16", true),
            "utf-16le" | "utf16le" => (Charset::Utf16(true), "UTF-16LE", false),
            "utf-16be" | "utf16be" => (Charset::Utf16(false), "UTF-16BE", false),
            "windows-1251" | "cp1251" => (Charset::Legacy(encoding_rs::WINDOWS_1251), "windows-1251", false),
            "windows-1252" | "cp1252" => (Charset::Legacy(encoding_rs::WINDOWS_1252), "windows-1252", false),
            "iso-8859-1" | "latin1" | "latin-1" => (Charset::Latin1, "ISO-8859-1", false),
            "us-ascii" | "ascii" => (Charset::Ascii, "US-ASCII", false),
            _ => return Err(format!("Unsupported export encoding '{}'. Choose UTF-8, UTF-16/LE/BE, windows-1251/1252, ISO-8859-1 or US-ASCII.", options.encoding)),
        };
        let replace = match options.encoding_errors.as_str() {
            "error" => false, "replace" => true,
            _ => return Err("encodingErrors must be 'error' or 'replace'".into()),
        };
        let bom = if options.bom.unwrap_or(default_bom) {
            match charset {
                Charset::Utf8 => b"\xef\xbb\xbf".as_slice(),
                Charset::Utf16(true) => b"\xff\xfe".as_slice(),
                Charset::Utf16(false) => b"\xfe\xff".as_slice(),
                _ => return Err("A BOM is available only for UTF-8 and UTF-16. Disable BOM for a legacy encoding.".into()),
            }
        } else { &[] };
        Ok(Self { charset, name, bom, eager_bom: options.bom == Some(true), replace })
    }

    pub fn needs_preflight(&self) -> bool { !self.replace && !matches!(self.charset, Charset::Utf8 | Charset::Utf16(_)) }
    pub fn passthrough(&self) -> bool { matches!(self.charset, Charset::Utf8) && self.bom.is_empty() }

    pub fn writer<'a>(&self, output: &'a mut dyn Write) -> Result<Writer<'a>> {
        let mut writer = Writer { output, charset: self.charset, name: self.name, bom: self.bom, started: false, replace: self.replace };
        // Explicit BOM includes an otherwise empty output. Java's implicit UTF-16 BOM is lazy.
        if self.eager_bom { writer.start().map_err(err)?; }
        Ok(writer)
    }

    pub fn decode(&self, bytes: &[u8]) -> Result<String> {
        let bytes = if !self.bom.is_empty() { bytes.strip_prefix(self.bom).unwrap_or(bytes) } else { bytes };
        match self.charset {
            Charset::Utf8 | Charset::Ascii => String::from_utf8(bytes.to_vec()).map_err(err),
            Charset::Utf16(little) => {
                if bytes.len() % 2 != 0 { return Err("Invalid UTF-16 export byte length".into()); }
                let units = bytes.chunks_exact(2).map(|b| if little { u16::from_le_bytes([b[0],b[1]]) } else { u16::from_be_bytes([b[0],b[1]]) }).collect::<Vec<_>>();
                String::from_utf16(&units).map_err(err)
            }
            Charset::Legacy(encoding) => Ok(encoding.decode_without_bom_handling(bytes).0.into_owned()),
            Charset::Latin1 => Ok(bytes.iter().map(|&b| char::from(b)).collect()),
        }
    }
}

pub(super) struct Writer<'a> {
    output: &'a mut dyn Write,
    charset: Charset,
    name: &'static str,
    bom: &'static [u8],
    started: bool,
    replace: bool,
}
impl Writer<'_> {
    fn start(&mut self) -> io::Result<()> {
        if !self.started { self.output.write_all(self.bom)?; self.started = true; }
        Ok(())
    }
    fn unmappable(&self, ch: char) -> io::Error {
        io::Error::new(io::ErrorKind::InvalidData, format!("Character U+{:04X} cannot be encoded as {}. Choose UTF-8/UTF-16 or explicitly allow replacement with '?'.", ch as u32, self.name))
    }
}
impl Write for Writer<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let mut text = std::str::from_utf8(bytes).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        if text.is_empty() { return Ok(0); }
        self.start()?;
        let mut buffer = [0u8; 8192];
        match self.charset {
            Charset::Utf8 => self.output.write_all(bytes)?,
            Charset::Utf16(little) => {
                let mut used = 0;
                for unit in text.encode_utf16() {
                    let pair = if little { unit.to_le_bytes() } else { unit.to_be_bytes() };
                    buffer[used..used+2].copy_from_slice(&pair); used += 2;
                    if used == buffer.len() { self.output.write_all(&buffer)?; used = 0; }
                }
                self.output.write_all(&buffer[..used])?;
            }
            Charset::Legacy(encoding) => {
                // WHATWG maps these undefined Windows bytes to literal C1 controls. Java's
                // charset encoder rejects those scalars; pin that difference to the OR fixtures.
                let undefined = |ch| if self.name == "windows-1251" { ch == '\u{0098}' } else { matches!(ch, '\u{0081}' | '\u{008d}' | '\u{008f}' | '\u{0090}' | '\u{009d}') };
                let corrected = if let Some(ch) = text.chars().find(|&ch| undefined(ch)) {
                    if !self.replace { return Err(self.unmappable(ch)); }
                    Some(text.chars().map(|ch| if undefined(ch) { '?' } else { ch }).collect::<String>())
                } else { None };
                if let Some(corrected) = &corrected { text = corrected; }
                // These two Windows encodings are stateless; every incoming chunk ends at UTF-8
                // scalar boundaries. Use the no-replacement API so '?' is opt-in, never an NCR.
                let mut encoder = encoding.new_encoder();
                loop {
                    let (result, read, written) = encoder.encode_from_utf8_without_replacement(text, &mut buffer, true);
                    self.output.write_all(&buffer[..written])?;
                    text = &text[read..];
                    match result {
                        encoding_rs::EncoderResult::InputEmpty => break,
                        encoding_rs::EncoderResult::OutputFull => {},
                        encoding_rs::EncoderResult::Unmappable(ch) => {
                            if !self.replace { return Err(self.unmappable(ch)); }
                            self.output.write_all(b"?")?;
                        }
                    }
                }
            }
            Charset::Latin1 | Charset::Ascii => {
                let max = if matches!(self.charset, Charset::Ascii) { 127 } else { 255 };
                let mut used = 0;
                for ch in text.chars() {
                    buffer[used] = if ch as u32 <= max { ch as u8 } else if self.replace { b'?' } else { return Err(self.unmappable(ch)); };
                    used += 1;
                    if used == buffer.len() { self.output.write_all(&buffer)?; used = 0; }
                }
                self.output.write_all(&buffer[..used])?;
            }
        }
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> { self.output.flush() }
}
