//! Mutation fuzzing: real inputs from the test suites (GREL expressions, files of every format),
//! damaged at random, through the GREL parser and evaluator and every importer. Any panic fails
//! the test and its input is saved under target/fuzz/. Before each run the input is also written to
//! target/fuzz/last-<target>-<pid>, so a hard crash (stack overflow, out of memory) leaves its culprit.
//!
//!   cargo test --release -p ox-core --test fuzz                        # a short run, as in CI
//!   FUZZ_ITERS=1000000 FUZZ_SEED=7 cargo test --release -p ox-core --test fuzz -- --nocapture
use ox_core::{grel, import, table::Column, table::Table};
use std::{
    panic::{catch_unwind, AssertUnwindSafe},
    path::{Path, PathBuf},
    sync::Arc,
    time::Instant,
};

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        if n == 0 { 0 } else { (self.next() % n as u64) as usize }
    }
    fn pick<'a, T>(&mut self, xs: &'a [T]) -> &'a T {
        &xs[self.below(xs.len())]
    }
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn out_dir() -> PathBuf {
    let d = root().join("target/fuzz");
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn iterations() -> usize {
    std::env::var("FUZZ_ITERS").ok().and_then(|s| s.parse().ok()).unwrap_or(3000)
}

fn seed() -> u64 {
    std::env::var("FUZZ_SEED").ok().and_then(|s| s.parse().ok()).unwrap_or(0x5eed_f0e7)
}

/// One random change to bytes: flip, insert, delete, duplicate a run, truncate, or splice in another input.
fn mutate(rng: &mut Rng, data: &mut Vec<u8>, others: &[Vec<u8>], alphabet: &[u8]) {
    let n = data.len();
    match rng.below(9) {
        0 if n > 0 => {
            let i = rng.below(n);
            data[i] ^= 1 << rng.below(8);
        }
        1 => {
            let i = rng.below(n + 1);
            data.insert(i, *rng.pick(alphabet));
        }
        2 if n > 0 => {
            let i = rng.below(n);
            let k = 1 + rng.below((n - i).min(16));
            data.drain(i..i + k);
        }
        3 if n > 0 => {
            let i = rng.below(n);
            let k = 1 + rng.below((n - i).min(64));
            let run: Vec<u8> = data[i..i + k].to_vec();
            let at = rng.below(n + 1);
            for _ in 0..1 + rng.below(4) {
                data.splice(at..at, run.iter().copied());
            }
        }
        4 if n > 0 => data.truncate(rng.below(n)),
        5 if !others.is_empty() => {
            let o = rng.pick(others);
            let cut = rng.below(n + 1);
            let from = rng.below(o.len() + 1);
            data.truncate(cut);
            data.extend_from_slice(&o[from..]);
        }
        6 if n > 0 => {
            let i = rng.below(n);
            data[i] = *rng.pick(alphabet);
        }
        7 => {
            // a long run of one byte: deep nesting, huge fields, many separators
            let i = rng.below(n + 1);
            let b = *rng.pick(alphabet);
            let k = 1 + rng.below(4096);
            data.splice(i..i, std::iter::repeat_n(b, k));
        }
        _ if n > 1 => {
            let (a, b) = (rng.below(n), rng.below(n));
            data.swap(a, b);
        }
        _ => data.push(*rng.pick(alphabet)),
    }
}

/// Runs `f`, turning a panic into its message.
fn guarded(f: impl FnOnce()) -> Option<String> {
    catch_unwind(AssertUnwindSafe(f)).err().map(|e| {
        e.downcast_ref::<String>().cloned().or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string())).unwrap_or_else(|| "panic".into())
    })
}

/// The fuzz loop for one target: mutate a seed, remember it, run it, record panics and slow inputs.
fn fuzz(target: &str, seeds: Vec<Vec<u8>>, alphabet: &[u8], mut run: impl FnMut(&[u8])) {
    std::panic::set_hook(Box::new(|_| {}));
    let mut rng = Rng(seed() ^ target.bytes().fold(0u64, |h, b| h.wrapping_mul(31).wrapping_add(b as u64)) | 1);
    let last = out_dir().join(format!("last-{target}-{}", std::process::id()));
    let (mut crashes, mut slow) = (vec![], 0);
    let start = Instant::now();
    for i in 0..iterations() {
        let mut data = rng.pick(&seeds).clone();
        for _ in 0..1 + rng.below(6) {
            mutate(&mut rng, &mut data, &seeds, alphabet);
        }
        std::fs::write(&last, &data).ok();
        let t = Instant::now();
        if let Some(msg) = guarded(|| run(&data)) {
            let file = out_dir().join(format!("crash-{target}-{i}"));
            std::fs::write(&file, &data).ok();
            crashes.push(format!("{target} #{i}: {msg} (input in {})", file.display()));
        }
        if t.elapsed().as_secs_f64() > 2.0 {
            slow += 1;
            std::fs::write(out_dir().join(format!("slow-{target}-{i}")), &data).ok();
        }
    }
    let _ = std::panic::take_hook();
    println!("{target}: {} runs in {:.1} s, {} panics, {slow} slower than 2 s", iterations(), start.elapsed().as_secs_f64(), crashes.len());
    assert!(crashes.is_empty(), "\n{}\n", crashes.join("\n"));
}

/// A small table whose rows the expressions run against (`cells`, `row`, facets).
fn sample_table() -> Table {
    let values = |v: Vec<grel::Value>| Arc::new(Column::from_values(v));
    use grel::Value::*;
    Table {
        names: vec!["name".into(), "n".into(), "when".into()],
        columns: vec![
            values(vec![Str(" Hello World ".into()), Str("café Ünïcödé".into()), Null, Str("a,b,,c".into())]),
            values(vec![Int(3), Num(2.5), Str("x".into()), Int(-17)]),
            values(vec![Str("2024-03-15".into()), Null, Str("not a date".into()), Str("1999-12-31T23:59:59Z".into())]),
        ],
        rows: 4,
        starred: Arc::new(vec![false, true, false, false]),
        flagged: Arc::new(vec![false, false, true, false]),
        key: None,
    }
}

#[test]
fn fuzz_grel() {
    let fx: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(root().join("tests/grel/fixtures.json")).unwrap()).unwrap();
    // minus the deliberately heavy ones (ten million elements), which are slow but not wrong
    let mut seeds: Vec<Vec<u8>> = fx["cases"].as_array().unwrap().iter().map(|c| c["expr"].as_str().unwrap()).filter(|e| !e.contains("0000000")).map(|e| e.as_bytes().to_vec()).collect();
    seeds.push(b"forEach(value.split(','), v, if(v.length() > 1, v.toUppercase(), v)).join('|')".to_vec());
    // every function with arguments of every kind, so splicing recombines calls rather than mostly breaking syntax
    let args = [
        "", "value", "1", "-1", "0", "2147483647", "9223372036854775807", "1.5", "''", "'a,b'", "null", "true", "[1, 'a', null]", "[]",
        "'2024-03-15'.toDate()", "'{\"a\":[1,{\"b\":2}]}'.parseJson()", "'<p>x<b>y</b></p>'.parseHtml()", "/a(b)?/", "cells", "row", "value, 3", "value, 'x', 'y'",
    ];
    for f in grel::funcs::FUNCTIONS {
        for a in args {
            seeds.push(format!("{f}({a})").into_bytes());
            seeds.push(format!("value.{f}({a})").into_bytes());
        }
    }
    let alphabet = br#"()[]{},.'"+-*/%<>=!&|:;\ 0123456789aeEnvxlurtpsfic_value cells row  "#.to_vec();
    let table = sample_table();
    let ctx = ox_core::engine::Ctx::default();
    let inputs: Vec<grel::Value> = (0..table.rows).map(|r| table.columns[0].get(r).clone()).collect();
    fuzz("grel", seeds, &alphabet, |src| {
        let Ok(text) = std::str::from_utf8(src) else { return };
        let Ok(p) = grel::compile(text) else { return };
        for (r, v) in inputs.iter().enumerate() {
            let _ = p.eval(v.clone(), &table.row_ctx(r, &ctx)).render();
        }
        let _ = p.eval(grel::Value::Null, &grel::NoRow);
    });
}

/// Inputs the fuzzer found that once crashed, hung or ran out of memory; each must now end with a value or an error.
#[test]
fn fuzz_finds() {
    let run = |src: &str| grel::compile(src).map(|p| p.eval(grel::Value::Str("x".into()), &grel::NoRow).render());
    for src in [
        format!("value.replace({}1444444444444444,055)5)5)", "(".repeat(4800)),
        "range(1444444444444444444444,055)".into(),
        "range(2147483647)".into(),
        "range(-2147483648, 2147483647, 1)".into(),
        "range(0, 9223372036854775807, 4611686018427387904)".into(),
        "forRange(0, 1000000000000, 1, i, i)".into(),
        "forRange(10000000000000000, 10000000000000004, 0.5, i, i)".into(),
        "value.iurrrrre.if('o')".into(),
        "multinomial(214748367)".into(),
        "factn(9223372036854775807, 1)".into(),
        "factn(5, 0)".into(),
        "combin(9223372036854775807, 4611686018427387903)".into(),
        "multinomial(9223372036854775807, 9223372036854775807)".into(),
    ] {
        let t = Instant::now();
        let _ = run(&src);
        assert!(t.elapsed().as_secs() < 2, "{} took {:?}", &src[..src.len().min(60)], t.elapsed());
    }
}

#[test]
fn fuzz_import() {
    let mut files: Vec<PathBuf> = vec![];
    for dir in ["tests/import", "tests/corpus/files"] {
        for e in std::fs::read_dir(root().join(dir)).unwrap().flatten() {
            let p = e.path();
            let name = p.file_name().unwrap().to_string_lossy();
            // every sample file, not the suite's own scripts and fixtures
            if p.is_file() && !name.ends_with(".mjs") && name != "fixtures.json" && name != "known_failures.txt" {
                files.push(p);
            }
        }
    }
    files.sort();
    let exts: Vec<String> = files.iter().map(|p| p.file_name().unwrap().to_string_lossy().split_once('.').map(|(_, e)| e.to_string()).unwrap_or_default()).collect();
    let seeds: Vec<Vec<u8>> = files.iter().map(|p| std::fs::read(p).unwrap()).collect();
    let alphabet = b",;\t\"'\n\r {}[]<>/=:0123456789abcxyz\x00\xff\xc3\xa9PK\x03\x04".to_vec();
    let dir = std::env::temp_dir().join("oxrefine-fuzz-import");
    std::fs::create_dir_all(&dir).unwrap();
    let formats = [None, Some("csv"), Some("tsv"), Some("json"), Some("xml"), Some("xlsx"), Some("ods"), Some("lines"), Some("fixed-width")];
    let mut rng = Rng(seed() | 1);
    fuzz("import", seeds, &alphabet, |data| {
        // the file keeps the extension of some seed, so format guessing is exercised too
        let ext = rng.pick(&exts).clone();
        let path = dir.join(format!("input.{ext}"));
        std::fs::write(&path, data).unwrap();
        let _ = import::inspect(&path);
        let format = *rng.pick(&formats);
        let _ = import::import(&path, format, &import::ImportOptions::default());
    });
}
