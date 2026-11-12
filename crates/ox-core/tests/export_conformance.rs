//! Every case in tests/export/fixtures.json must export the same bytes as OpenRefine 3.10.1
//! (Excel files: the same cells, read back with calamine). Exceptions live in known_failures.txt.
use base64::Engine as _;
use calamine::{Data, Reader};
use ox_core::{
    engine::{filter, EngineConfig},
    export::export,
    ops::Op,
    Project,
};
use serde_json::Value as J;
use std::collections::BTreeSet;
use std::io::Cursor;

/// The sheet names, then the cells of the first sheet as text, so ours and OpenRefine's compare whatever
/// number types each picked.
fn sheet(bytes: &[u8]) -> Vec<Vec<String>> {
    let mut wb = calamine::open_workbook_auto_from_rs(Cursor::new(bytes.to_vec())).unwrap();
    let names = wb.sheet_names();
    let range = wb.worksheet_range_at(0).unwrap().unwrap();
    std::iter::once(names).chain(range
        .rows()
        .map(|r| {
            r.iter()
                .map(|c| match c {
                    Data::Float(f) if f.fract() == 0.0 => format!("{}", *f as i64),
                    Data::DateTime(d) => d.as_datetime().map_or(d.to_string(), |d| d.to_string()),
                    c => c.to_string(),
                })
                .collect()
        }))
        .collect()
}

#[test]
fn exports_match_openrefine() {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/export/");
    let fx: J = serde_json::from_str(&std::fs::read_to_string(format!("{root}fixtures.json")).unwrap()).unwrap();
    let known: BTreeSet<String> = std::fs::read_to_string(format!("{root}known_failures.txt"))
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
        .map(|l| l.split(" #").next().unwrap().trim_end().to_string())
        .collect();
    let corpus = std::env::temp_dir().join("oxrefine-export-corpus.csv");
    std::fs::write(&corpus, fx["corpus"].as_str().unwrap()).unwrap();
    let mut p = Project::open(&corpus).unwrap();
    for op in fx["setup"].as_array().unwrap() {
        p.apply(serde_json::from_value::<Op>(op.clone()).unwrap()).unwrap();
    }
    let t = p.table();
    let name = fx["project"].as_str().unwrap();

    let cases = fx["cases"].as_array().unwrap();
    let (mut failing, mut report) = (BTreeSet::new(), vec![]);
    for case in cases {
        let case_name = case["name"].as_str().unwrap();
        let engine: EngineConfig = serde_json::from_value(case["engine"].clone()).unwrap();
        let rows = filter(t, &engine, None).unwrap();
        let params = case.get("params").cloned().unwrap_or(J::Null);
        let why = match export(t, &rows, case["format"].as_str().unwrap(), &params, name) {
            Err(e) => Some(format!("error: {e}")),
            Ok(got) if case["binary"].as_bool() == Some(true) => {
                let want = base64::engine::general_purpose::STANDARD.decode(case["output"].as_str().unwrap()).unwrap();
                let (w, g) = (sheet(&want), sheet(&got));
                (w != g).then(|| format!("expected cells {w:?}\n    got {g:?}"))
            }
            Ok(got) => {
                let (w, g) = (case["output"].as_str().unwrap(), String::from_utf8_lossy(&got));
                (w != g).then(|| format!("expected {w:?}\n    got      {g:?}"))
            }
        };
        match why {
            Some(w) => {
                failing.insert(case_name.to_string());
                if !known.contains(case_name) {
                    report.push(format!("NEW FAILURE  {case_name}\n    {w}"));
                }
            }
            None if known.contains(case_name) => report.push(format!("NOW PASSES (remove from known_failures.txt)  {case_name}")),
            None => {}
        }
    }
    println!("Export conformance: {}/{} cases match OpenRefine 3.10.1 ({} known failures)", cases.len() - failing.len(), cases.len(), failing.len());
    assert!(report.is_empty(), "\n{}\n", report.join("\n"));
}

/// Excel 97: thousands of distinct strings (the shared string table runs over many CONTINUE records),
/// characters that need UTF-16, and a cell past Excel's 32,767 characters all read back.
#[test]
fn xls_strings_survive_continue_records() {
    let dir = std::env::temp_dir().join(format!("oxrefine-xls-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut csv = String::from("id,text\n");
    let text = |i: usize| match i {
        7 => "x".repeat(40_000),
        _ if i % 3 == 0 => format!("Москва {i}"),
        _ if i % 3 == 1 => format!("東京 {i} — ü"),
        _ => format!("plain {i}"),
    };
    for i in 0..5000 {
        csv += &format!("{i},{}\n", text(i));
    }
    std::fs::write(dir.join("in.csv"), csv).unwrap();
    let p = Project::open(&dir.join("in.csv")).unwrap();
    let t = p.table();
    let rows: Vec<u32> = (0..t.rows as u32).collect();
    let got = sheet(&export(t, &rows, "xls", &J::Null, "strings").unwrap());
    assert_eq!(got[0], vec!["strings"]);
    assert_eq!(got.len(), 5002, "sheet names, header, 5000 rows");
    for i in 0..5000 {
        let want = text(i);
        let want: String = if want.len() > 32_767 { want.chars().take(32_767).collect() } else { want };
        assert_eq!(got[i + 2][1], want, "row {i}");
    }
    let _ = std::fs::remove_dir_all(dir);
}
