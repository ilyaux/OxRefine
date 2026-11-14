//! Rust adaptation of Optimaize language-detector 0.6's detection algorithm (Apache-2.0).
//! Original algorithm: Nakatani Shuyo, Fabian Kessler and Elmer Garduno. Changed: sparse Rust
//! model, bundled reference lookup data and Java-compatible seeded RNG; no JVM at runtime.
//! Original NGram notice: Copyright 2011 Nakatani Shuyo.
//! See assets/language-detector/LICENSE.txt and provenance.json for profiles, sources and hashes.
use flate2::read::GzDecoder;
use std::{collections::HashMap, io::{Cursor, Read}, sync::OnceLock};

const CONVERGED: f64 = 0.99999;
const CONFIDENCE: f64 = 0.9999;

struct Model {
    languages: Vec<String>,
    grams: HashMap<u64, Vec<(u8, f64)>>,
    characters: Vec<u8>,
    common: u8,
    inherited: u8,
    unknown: u8,
}

fn key(units: &[u16]) -> u64 {
    units.iter().enumerate().fold((units.len() as u64) << 48, |v, (i, &c)| v | ((c as u64) << (i * 16)))
}

fn model() -> &'static Model {
    static MODEL: OnceLock<Model> = OnceLock::new();
    MODEL.get_or_init(|| {
        let meta: serde_json::Value = serde_json::from_str(include_str!("../../assets/language-detector/provenance.json")).expect("bundled language metadata");
        let languages: Vec<String> = meta["languageOrder"].as_array().unwrap().iter().map(|v| v.as_str().unwrap().into()).collect();
        let mut archive = zip::ZipArchive::new(Cursor::new(include_bytes!("../../assets/language-detector/profiles.zip"))).expect("bundled language profiles");
        let mut grams: HashMap<u64, Vec<(u8, f64)>> = HashMap::new();
        for (language, name) in languages.iter().enumerate() {
            let profile: serde_json::Value = serde_json::from_reader(archive.by_name(&format!("languages/{name}")).unwrap()).expect("bundled language profile JSON");
            let entries: Vec<_> = profile["freq"].as_object().unwrap().iter().filter_map(|(s, freq)| {
                let units: Vec<u16> = s.encode_utf16().collect();
                (1..=3).contains(&units.len()).then(|| (key(&units), units.len(), freq.as_u64().unwrap()))
            }).collect();
            let mut totals = [0u64; 4];
            for &(_, size, count) in &entries { totals[size] += count; }
            for (gram, size, count) in entries { grams.entry(gram).or_default().push((language as u8, count as f64 / totals[size] as f64)); }
        }
        let mut characters = vec![];
        GzDecoder::new(&include_bytes!("../../assets/language-detector/characters.bin.gz")[..]).read_to_end(&mut characters).expect("bundled character tables");
        assert_eq!(characters.len(), 65536 * 3);
        Model { languages, grams, characters,
            common: meta["scripts"]["COMMON"].as_u64().unwrap() as u8,
            inherited: meta["scripts"]["INHERITED"].as_u64().unwrap() as u8,
            unknown: meta["scripts"]["UNKNOWN"].as_u64().unwrap() as u8 }
    })
}

impl Model {
    fn script(&self, c: u16) -> u8 { self.characters[c as usize * 3 + 2] }
    fn normalize(&self, c: u16) -> u16 {
        let at = c as usize * 3;
        u16::from_be_bytes([self.characters[at], self.characters[at + 1]])
    }
    fn text(&self, text: &str) -> Vec<u16> {
        use regex::Regex;
        static URL: OnceLock<Regex> = OnceLock::new();
        static MAIL: OnceLock<Regex> = OnceLock::new();
        let text = URL.get_or_init(|| Regex::new(r"https?://[-_.?&~;+=/#0-9A-Za-z]+").unwrap()).replace_all(text, " ");
        let text = MAIL.get_or_init(|| Regex::new(r"[-_.0-9A-Za-z]+@[-_0-9A-Za-z]+[-_.0-9A-Za-z]+").unwrap()).replace_all(&text, " ");
        let mut counts = [0u64; 256];
        let mut last = None;
        for c in text.encode_utf16() {
            let script = self.script(c);
            if script == self.inherited { if let Some(previous) = last { counts[previous as usize] += 1; } }
            else if script != self.common && script != self.unknown { counts[script as usize] += 1; last = Some(script); }
        }
        let most = *counts.iter().max().unwrap();
        let mut remove = [false; 256];
        if counts.iter().filter(|&&n| n > 0).count() > 1 {
            for (script, &count) in counts.iter().enumerate() { remove[script] = count > 0 && count != most && count as f64 / most as f64 <= 0.3; }
        }
        let mut last = None;
        let mut previous = 0;
        let mut out = Vec::with_capacity(text.len().min(10000));
        for c in text.encode_utf16() {
            let script = self.script(c);
            if script == self.inherited && last.is_some_and(|s| remove[s as usize]) { continue; }
            last = Some(script);
            if remove[script as usize] { continue; }
            let c = self.normalize(c);
            if c != 32 || previous != 32 { out.push(c); }
            previous = c;
            if out.len() == 10000 { break; }
        }
        out
    }
    fn update(&self, probabilities: &mut [f64], gram: u64, count: u32, alpha: f64) {
        let Some(frequencies) = self.grams.get(&gram) else { return };
        let mut frequencies = frequencies.iter().peekable();
        for (language, probability) in probabilities.iter_mut().enumerate() {
            let frequency = if frequencies.peek().is_some_and(|v| v.0 as usize == language) { frequencies.next().unwrap().1 } else { 0.0 };
            let factor = alpha / 10000.0 + frequency;
            for _ in 0..count { *probability *= factor; }
        }
    }
}

fn normalize(probabilities: &mut [f64]) -> f64 {
    let sum: f64 = probabilities.iter().sum();
    let mut largest = 0.0;
    for p in probabilities { *p /= sum; if largest < *p { largest = *p; } }
    largest
}

fn grams(mut text: Vec<u16>) -> Vec<u64> {
    if text.is_empty() { return vec![]; }
    if text.first() != Some(&32) { text.insert(0, 32); }
    if text.last() != Some(&32) { text.push(32); }
    let mut out = Vec::with_capacity(text.len() * 3);
    for size in 1..=3 {
        for gram in text.windows(size) {
            if (size == 1 && gram[0] == 32) || (size == 3 && gram[1] == 32) { continue; }
            out.push(key(gram));
        }
    }
    out
}

/// OpenRefine returns the language part, so both Chinese locale profiles return `zh`.
pub(super) fn detect(text: &str) -> Option<&'static str> {
    let model = model();
    let text = model.text(text);
    let short = text.len() <= 50;
    let grams = grams(text);
    if grams.is_empty() { return None; }
    let languages = model.languages.len();
    let mut result = vec![0.0; languages];
    if short {
        let mut order = vec![];
        let mut counts = HashMap::<u64, usize>::new();
        for gram in grams {
            if let Some(&i) = counts.get(&gram) { let (_, n): &mut (u64, u32) = &mut order[i]; *n += 1; }
            else { counts.insert(gram, order.len()); order.push((gram, 1)); }
        }
        result.fill(1.0 / languages as f64);
        for (gram, count) in order {
            model.update(&mut result, gram, count, 0.5);
            if normalize(&mut result) > CONVERGED { break; }
        }
        normalize(&mut result);
    } else {
        let mut random = JavaRandom::new(41);
        for _ in 0..7 {
            let mut probabilities = vec![1.0 / languages as f64; languages];
            let alpha = 0.5 + random.gaussian() * 0.05;
            for i in 0..1000 {
                model.update(&mut probabilities, grams[random.index(grams.len())], 1, alpha);
                if i % 5 == 0 && normalize(&mut probabilities) > CONVERGED { break; }
            }
            for (result, p) in result.iter_mut().zip(probabilities) { *result += p / 7.0; }
        }
    }
    let mut best = None;
    for (i, &p) in result.iter().enumerate() { if p >= CONFIDENCE && best.is_none_or(|(_, previous)| p > previous) { best = Some((i, p)); } }
    best.map(|(i, _)| model.languages[i].split('-').next().unwrap())
}

struct JavaRandom { seed: u64, gaussian: Option<f64> }
impl JavaRandom {
    fn new(seed: u64) -> Self { Self { seed: (seed ^ 0x5deece66d) & ((1u64 << 48) - 1), gaussian: None } }
    fn bits(&mut self, bits: u32) -> u32 {
        self.seed = self.seed.wrapping_mul(0x5deece66d).wrapping_add(11) & ((1u64 << 48) - 1);
        (self.seed >> (48 - bits)) as u32
    }
    fn double(&mut self) -> f64 { (((self.bits(26) as u64) << 27) + self.bits(27) as u64) as f64 / (1u64 << 53) as f64 }
    fn index(&mut self, bound: usize) -> usize {
        if bound.is_power_of_two() { return (bound as u64 * self.bits(31) as u64 >> 31) as usize; }
        loop {
            let bits = self.bits(31) as usize;
            let value = bits % bound;
            if bits - value + bound - 1 < 1 << 31 { return value; }
        }
    }
    fn gaussian(&mut self) -> f64 {
        if let Some(value) = self.gaussian.take() { return value; }
        loop {
            let x = 2.0 * self.double() - 1.0;
            let y = 2.0 * self.double() - 1.0;
            let s = x * x + y * y;
            if s >= 1.0 || s == 0.0 { continue; }
            let factor = libm::sqrt(-2.0 * libm::log(s) / s);
            self.gaussian = Some(y * factor);
            return x * factor;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn java_seed_and_gaussian_match_reference_outputs() {
        let data: serde_json::Value = serde_json::from_str(include_str!("../../assets/language-detector/provenance.json")).unwrap();
        let mut random = JavaRandom::new(41);
        for (bound, expected) in data["randomBounds"].as_array().unwrap().iter().zip(data["randomIntegers"].as_array().unwrap()) {
            assert_eq!(random.index(bound.as_u64().unwrap() as usize), expected.as_u64().unwrap() as usize);
        }
        let mut random = JavaRandom::new(41);
        for expected in data["gaussianBits"].as_array().unwrap() { assert_eq!(random.gaussian().to_bits(), u64::from_str_radix(expected.as_str().unwrap(), 16).unwrap()); }
    }

    #[test]
    fn text_pipeline_matches_reference_utf16_traces() {
        let fixtures: serde_json::Value = serde_json::from_str(include_str!("../../../../tests/detect_language/fixtures.json")).unwrap();
        for case in fixtures["cases"].as_array().unwrap().iter().filter(|c| c.get("normalizedUnits").is_some()) {
            let expected: Vec<u16> = case["normalizedUnits"].as_array().unwrap().iter().map(|v| v.as_u64().unwrap() as u16).collect();
            assert_eq!(model().text(case["text"].as_str().unwrap()), expected, "{}", case["name"]);
        }
    }

    #[test]
    #[ignore = "isolated cold-start/throughput measurement; run explicitly"]
    fn model_cost() {
        let start = std::time::Instant::now();
        let model = model();
        let cold = start.elapsed();
        let table = model.grams.capacity() * (std::mem::size_of::<u64>() + std::mem::size_of::<Vec<(u8, f64)>>() + 1);
        let entries: usize = model.grams.values().map(|v| v.capacity() * std::mem::size_of::<(u8, f64)>()).sum();
        let text = "This is an English paragraph about cleaning data and comparing different place names before saving the new table.";
        let start = std::time::Instant::now();
        for _ in 0..1000 { assert_eq!(std::hint::black_box(detect(text)), Some("en")); }
        eprintln!("model: {} languages, {} grams, cold {:?}, estimated retained buffers {} bytes (allocator overhead excluded), 1000 warm detections {:?}",
            model.languages.len(), model.grams.len(), cold, table + entries + model.characters.capacity(), start.elapsed());
    }
}
