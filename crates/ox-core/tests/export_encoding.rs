//! Charset bytes are captured from the verified official OpenRefine 3.10.1 release artifact.
use base64::Engine as _;
use ox_core::{engine::{filter, EngineConfig}, export::{export, export_file, export_to, preview_text}, grel::Value, table::{Column, Table}, Project};
use serde_json::{json, Value as J};
use std::{io::{self, Write}, path::PathBuf, sync::atomic::{AtomicU64, Ordering}};

fn work() -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let path = std::env::temp_dir().join(format!("ox-encoding-{}-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos(), NEXT.fetch_add(1, Ordering::Relaxed)));
    std::fs::create_dir(&path).unwrap(); path
}
fn table(name: &str, values: &[&str]) -> Table { Table::new(vec![name.into()], vec![Column::from_values(values.iter().map(|&s| Value::Str(s.into())))], values.len()) }

#[test]
fn charset_exports_match_real_openrefine_bytes() {
    let fixture: J = serde_json::from_str(include_str!("../../../tests/export/encoding-fixtures.json")).unwrap();
    let root = work();
    let mut failures=vec![];
    for (i, case) in fixture["cases"].as_array().unwrap().iter().enumerate() {
        let path = root.join(format!("{i}.csv"));
        std::fs::write(&path, case["corpus"].as_str().unwrap_or(fixture["corpus"].as_str().unwrap())).unwrap();
        let mut project = Project::open(&path).unwrap();
        for op in case["setup"].as_array().into_iter().flatten() { project.apply(serde_json::from_value(op.clone()).unwrap()).unwrap(); }
        let t = project.table();
        let engine: EngineConfig = serde_json::from_value(case["engine"].clone()).unwrap();
        let rows = filter(t, &engine, None).unwrap();
        let want = base64::engine::general_purpose::STANDARD.decode(case["outputBase64"].as_str().unwrap()).unwrap();
        let got = export(t, &rows, case["format"].as_str().unwrap(), &case["params"], "encoding").unwrap();
        if got!=want {
            let first=got.iter().zip(&want).position(|(g,w)| g!=w).unwrap_or(got.len().min(want.len()));
            failures.push(format!("{}: first byte difference {first}; got {} bytes, want {}",case["name"],got.len(),want.len()));
        }
    }
    assert!(failures.is_empty(),"{}",failures.join("\n"));
}

#[test]
fn unicode_bom_is_once_endian_correct_and_hidden_only_in_decoded_preview() {
    let t = table("имя", &["😀 🦀", "Москва"]); let rows = [0,1];
    let text = "имя\r\n😀 🦀\r\nМосква\r\n";
    for (encoding, bom, little) in [("UTF-8",b"\xef\xbb\xbf".as_slice(),None),("UTF-16LE",b"\xff\xfe".as_slice(),Some(true)),("UTF-16BE",b"\xfe\xff".as_slice(),Some(false))] {
        let params = json!({"encoding":encoding,"bom":true});
        let got = export(&t,&rows,"csv",&params,"test").unwrap();
        let body = match little {
            None => text.as_bytes().to_vec(),
            Some(little) => text.encode_utf16().flat_map(|u| if little { u.to_le_bytes() } else { u.to_be_bytes() }).collect(),
        };
        assert_eq!(got,[bom,body.as_slice()].concat(),"{encoding}");
        assert_eq!(preview_text(&t,&rows,"csv",&params,"test").unwrap(),text,"{encoding} preview");
        let without = export(&t,&rows,"csv",&json!({"encoding":encoding,"bom":false}),"test").unwrap(); assert_eq!(without,body);
    }
    assert_eq!(export(&t,&[],"custom",&json!({"encoding":"UTF-16","options":{"outputColumnHeaders":false}}),"test").unwrap(),b"");
    assert_eq!(export(&t,&[],"custom",&json!({"encoding":"UTF-8","bom":true,"options":{"outputColumnHeaders":false}}),"test").unwrap(),b"\xef\xbb\xbf");
    let leading = table("\u{feff}name", &["value"]);
    assert!(preview_text(&leading,&[0],"csv",&json!({"encoding":"UTF-8","bom":true}),"test").unwrap().starts_with('\u{feff}'), "strip only the transport BOM, not a real leading character");
}

#[test]
fn invalid_or_unrepresentable_encoding_never_truncates_or_creates_the_destination() {
    let root = work(); let existing = root.join("existing.csv"); let new = root.join("new.csv");
    std::fs::write(&existing,b"keep this file").unwrap();
    let t = table("name", &["ASCII", "Москва", "東京", "😀"]);
    for params in [json!({"encoding":"unknown"}),json!({"encoding":false}),json!({"encoding":"US-ASCII"}),json!({"encoding":"windows-1251"}),json!({"encoding":"windows-1252"}),json!({"encoding":"ISO-8859-1"}),json!({"encoding":"windows-1251","bom":true}),json!({"encodingErrors":"silent"})] {
        for path in [&existing,&new] { assert!(export_file(&t,&[0,1,2,3],"csv",&params,"test",path).is_err(),"{params}"); }
        assert_eq!(std::fs::read(&existing).unwrap(),b"keep this file"); assert!(!new.exists());
    }
    // Preflight checks exactly the selected, trimmed output, not unrelated rows/dictionary values.
    export_file(&t,&[0],"csv",&json!({"encoding":"US-ASCII"}),"test",&new).unwrap();
    assert_eq!(std::fs::read(&new).unwrap(),b"name\r\nASCII\r\n");
    let trimmed=table("name", &["\u{2003}ASCII\u{2003}"]);
    assert_eq!(export(&trimmed,&[0],"csv",&json!({"encoding":"US-ASCII"}),"test").unwrap(),b"name\r\nASCII\r\n");
    let unicode_header=table("имя", &["ASCII"]);
    assert!(export_file(&unicode_header,&[0],"csv",&json!({"encoding":"US-ASCII"}),"test",&existing).is_err());
    assert_eq!(std::fs::read(&existing).unwrap(),b"keep this file");
    assert!(export_file(&t,&[0],"*sv",&json!({"encoding":"US-ASCII","options":{"lineSeparator":"λ"}}),"test",&existing).is_err());
    assert_eq!(std::fs::read(&existing).unwrap(),b"keep this file");
    let mut values=vec!["ASCII";12]; values.push("Москва"); let late=table("name",&values);
    let rows=(0..late.rows as u32).collect::<Vec<_>>();
    assert!(preview_text(&late,&rows[..10],"csv",&json!({"encoding":"US-ASCII"}),"test").is_ok());
    assert!(export_file(&late,&rows,"csv",&json!({"encoding":"US-ASCII"}),"test",&existing).is_err());
    assert_eq!(std::fs::read(&existing).unwrap(),b"keep this file","a value past the preview cannot corrupt the destination");
}

#[test]
fn replacement_is_explicit_and_decoded_preview_shows_the_actual_substitutions() {
    let t=table("name", &["Илья 😀 €"]);
    assert!(preview_text(&t,&[0],"csv",&json!({"encoding":"US-ASCII"}),"test").unwrap_err().contains("U+"));
    let params=json!({"encoding":"US-ASCII","encodingErrors":"replace"});
    assert_eq!(export(&t,&[0],"csv",&params,"test").unwrap(),b"name\r\n???? ? ?\r\n");
    assert_eq!(preview_text(&t,&[0],"csv",&params,"test").unwrap(),"name\r\n???? ? ?\r\n");
    let cyrillic=table("name", &["Илья"]);
    assert_eq!(preview_text(&cyrillic,&[0],"csv",&json!({"encoding":"windows-1251"}),"test").unwrap(),"name\r\nИлья\r\n");
    for (encoding,controls) in [("windows-1251","\u{0098}"),("windows-1252","\u{0081}\u{008d}\u{008f}\u{0090}\u{009d}")] {
        let t=table("name",&[controls]);
        assert!(export(&t,&[0],"csv",&json!({"encoding":encoding}),"test").is_err(),"{encoding} must use Java's unmappable policy for undefined C1 controls");
    }
}

#[test]
fn encoded_file_path_keeps_streaming_across_many_utf8_and_utf16_chunks() {
    struct Sink { bytes: usize, largest: usize, writes: usize }
    impl Write for Sink {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> { self.bytes+=bytes.len(); self.largest=self.largest.max(bytes.len()); self.writes+=1; Ok(bytes.len()) }
        fn flush(&mut self) -> io::Result<()> { Ok(()) }
    }
    let t=table("name",&vec!["Москва 😀";50_000]); let rows=(0..t.rows as u32).collect::<Vec<_>>();
    let mut sink=Sink{bytes:0,largest:0,writes:0};
    export_to(&t,&rows,"csv",&json!({"encoding":"UTF-16LE","bom":true}),"test",&mut sink).unwrap();
    let expected="name\r\n".encode_utf16().count()+50_000*"Москва 😀\r\n".encode_utf16().count();
    assert_eq!(sink.bytes,expected*2+2); assert!(sink.writes>50); assert!(sink.largest<=8192,"transcoding buffers must remain bounded");
    struct Failure;
    impl Write for Failure {
        fn write(&mut self,_: &[u8]) -> io::Result<usize> { Err(io::Error::other("test write failure")) }
        fn flush(&mut self) -> io::Result<()> { Ok(()) }
    }
    assert!(export_to(&t,&rows,"csv",&json!({"encoding":"UTF-16BE"}),"test",&mut Failure).unwrap_err().contains("test write failure"));
}
