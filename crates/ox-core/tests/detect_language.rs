use ox_core::grel::{self, NoRow, Value};
use serde_json::{json, Value as Json};

fn fixture() -> Json { serde_json::from_str(include_str!("../../../tests/detect_language/fixtures.json")).unwrap() }
fn shown(value: Value) -> Json {
    match value { Value::Str(s) => json!(s), Value::Null => Json::Null, Value::Error(message) => json!({ "message": message }), other => panic!("unexpected detector output: {}", other.display()) }
}

#[test]
fn detection_matches_real_openrefine_for_all_profiles_noise_types_and_boundaries() {
    let fixtures = fixture();
    let value_program = grel::compile("value.detectLanguage()").unwrap();
    for case in fixtures["cases"].as_array().unwrap() {
        let result = if let Some(text) = case["text"].as_str() { value_program.eval(Value::Str(text.into()), &NoRow) }
        else { grel::compile(case["expression"].as_str().unwrap()).unwrap().eval(Value::Null, &NoRow) };
        assert_eq!(shown(result), case["result"], "{}", case["name"]);
    }
}

#[test]
fn repeated_and_parallel_calls_keep_the_reference_seed_and_share_immutable_profiles() {
    let fixtures = fixture();
    let cases: Vec<_> = fixtures["cases"].as_array().unwrap().iter().filter(|c| c["name"].as_str().unwrap().starts_with("paragraph ")).step_by(9)
        .map(|c| (c["text"].as_str().unwrap().to_string(), c["result"].clone())).collect();
    std::thread::scope(|scope| {
        for _ in 0..4 {
            let cases = &cases;
            scope.spawn(move || {
                let program = grel::compile("value.detectLanguage()").unwrap();
                for _ in 0..3 { for (text, expected) in cases { assert_eq!(&shown(program.eval(Value::Str(text.clone()), &NoRow)), expected); } }
            });
        }
    });
}
