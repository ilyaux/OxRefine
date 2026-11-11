//! tests/cluster/fixtures.json: GREL phonetic() under every encoder, and the clusters every clusterer
//! finds, must match OpenRefine 3.10.1 (its one-core kNN answer). Exceptions live in known_failures.txt.
use ox_core::{
    cluster::cluster,
    grel::{self, Row, Value},
};
use serde_json::{json, Value as J};
use std::collections::{BTreeSet, HashMap};

/// Clusters as a set of sets of (value, count): order is not part of OpenRefine's contract.
fn as_set(clusters: &[Vec<(String, u64)>]) -> BTreeSet<BTreeSet<(String, u64)>> {
    clusters.iter().map(|c| c.iter().cloned().collect()).collect()
}

#[test]
fn clustering_matches_openrefine() {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/cluster/");
    let fx: J = serde_json::from_str(&std::fs::read_to_string(format!("{root}fixtures.json")).unwrap()).unwrap();
    let known: BTreeSet<String> = std::fs::read_to_string(format!("{root}known_failures.txt"))
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
        .map(|l| l.split(" #").next().unwrap().trim_end().to_string())
        .collect();
    let values: Vec<Option<String>> = fx["values"].as_array().unwrap().iter().map(|v| v.as_str().map(String::from)).collect();
    let (mut total, mut failing, mut report) = (0, BTreeSet::new(), vec![]);
    let mut check = |name: String, why: Option<String>| {
        total += 1;
        match why {
            Some(w) => {
                if !known.contains(&name) {
                    report.push(format!("NEW FAILURE  {name}\n    {w}"));
                }
                failing.insert(name);
            }
            None if known.contains(&name) => report.push(format!("NOW PASSES (remove from known_failures.txt)  {name}")),
            None => {}
        }
    };

    // GREL phonetic(), value by value: one case per (encoder, value), so a known difference names the value
    for (enc, expected) in fx["phonetic"].as_object().unwrap() {
        let program = grel::compile(&format!("value.phonetic('{enc}')")).unwrap();
        for (v, want) in values.iter().zip(expected.as_array().unwrap()) {
            let Some(v) = v else { continue };
            let got = program.eval(Value::Str(v.as_str().into()), &grel::NoRow);
            let ok = match (want, &got) {
                (J::Object(_), Value::Error(_)) => true,
                (J::Null, Value::Null) => true,
                (J::String(w), Value::Str(g)) => w == g.as_str(),
                _ => false,
            };
            check(format!("phonetic {enc} {v:?}"), (!ok).then(|| format!("expected {want}, got {:?}", got.render())));
        }
    }

    // clusterers
    let mut counts: HashMap<&str, u32> = HashMap::new();
    for v in values.iter().flatten() {
        *counts.entry(v.as_str()).or_default() += 1;
    }
    let mut vc: Vec<(&str, u32)> = counts.into_iter().collect();
    vc.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
    for case in fx["clusters"].as_array().unwrap() {
        let c = &case["clusterer"];
        let p = &c["params"];
        let name = format!("{} {}", c["function"].as_str().unwrap(), p);
        let got = cluster(
            &vc,
            c["function"].as_str().unwrap(),
            p["ngram-size"].as_u64().unwrap_or(2) as usize,
            p["radius"].as_f64().unwrap_or(1.0),
            p["blocking-ngram-size"].as_u64().unwrap_or(6) as usize,
        );
        let why = match (&case["clusters"], got) {
            (J::Array(want), Ok(got)) => {
                let want: Vec<Vec<(String, u64)>> = want.iter().map(|c| c.as_array().unwrap().iter().map(|e| (e[0].as_str().unwrap().to_string(), e[1].as_u64().unwrap())).collect()).collect();
                let got: Vec<Vec<(String, u64)>> = got.iter().map(|c| c.iter().map(|(v, n)| (v.to_string(), *n as u64)).collect()).collect();
                let (w, g) = (as_set(&want), as_set(&got));
                (w != g).then(|| {
                    let missing: Vec<_> = w.difference(&g).take(3).collect();
                    let extra: Vec<_> = g.difference(&w).take(3).collect();
                    format!("{} clusters expected, {} found; missing {missing:?}; extra {extra:?}", w.len(), g.len())
                })
            }
            (J::Array(_), Err(e)) => Some(format!("error: {e}")),
            (want, got) => (got.is_ok()).then(|| format!("OpenRefine failed ({want}) but we clustered")),
        };
        check(name, why);
    }
    let _ = (json!(null), &grel::NoRow as &dyn Row);
    println!("Cluster conformance: {}/{} cases match OpenRefine 3.10.1 ({} known failures)", total - failing.len(), total, failing.len());
    assert!(report.is_empty(), "\n{}\n", report.join("\n"));
}
