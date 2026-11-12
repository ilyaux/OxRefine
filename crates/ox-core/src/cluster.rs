//! Clustering as OpenRefine does it: key collision with its keyers (fingerprint, n-gram fingerprint,
//! Metaphone 3, Cologne, Daitch-Mokotoff, Beider-Morse) and nearest neighbours with Levenshtein
//! distance and n-gram blocking. Pinned to OpenRefine 3.10.1 by tests/cluster_conformance.rs.
use crate::{grel::funcs, Result};
use rphonetic::{BeiderMorse, BeiderMorseBuilder, Cologne, ConfigFiles, DaitchMokotoffSoundex, DaitchMokotoffSoundexBuilder, Encoder, Metaphone, NameType, RuleType};
use std::{
    cell::RefCell,
    collections::{BTreeSet, HashMap},
    sync::OnceLock,
};
use unicode_normalization::{char::is_combining_mark, UnicodeNormalization};

/// Fold accents and common ligatures to ASCII, like OpenRefine's asciify.
pub fn asciify(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.nfd().filter(|c| !is_combining_mark(*c)) {
        match c {
            'ß' => out.push_str("ss"),
            'æ' => out.push_str("ae"),
            'Æ' => out.push_str("AE"),
            'œ' => out.push_str("oe"),
            'Œ' => out.push_str("OE"),
            'ø' => out.push('o'),
            'Ø' => out.push('O'),
            'đ' => out.push('d'),
            'Đ' => out.push('D'),
            'ł' => out.push('l'),
            'Ł' => out.push('L'),
            'þ' => out.push_str("th"),
            'Þ' => out.push_str("TH"),
            'ı' => out.push('i'),
            _ => out.push(c),
        }
    }
    out
}

pub use funcs::{fingerprint, ngram_fingerprint};

/// The fingerprint keyers' folding of lowercased text (OpenRefine's FingerprintKeyer): compatibility
/// decomposition without the accents, then its table for letters that don't decompose.
pub fn fingerprint_fold(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.nfkd().filter(|c| !is_combining_mark(*c)) {
        match c {
            'ß' => out.push_str("ss"),
            'æ' => out.push_str("ae"),
            'ø' | 'œ' => out.push_str("oe"),
            '©' => out.push('c'),
            'ð' | 'đ' | 'ɖ' => out.push('d'),
            'þ' => out.push_str("th"),
            'ƿ' => out.push('w'),
            'ħ' => out.push('h'),
            'ı' => out.push('i'),
            'ĸ' => out.push('k'),
            'ł' => out.push('l'),
            'ŋ' => out.push('n'),
            'ſ' | 'ẜ' | 'ẝ' => out.push('s'),
            'ŧ' => out.push('t'),
            _ => out.push(c),
        }
    }
    out
}

/// Clusters of distinct values, biggest first, members by count (the first is the merge suggestion).
/// `method` is a keyer name (key collision; `n` is the n-gram size) or a distance name (nearest
/// neighbours within `radius`, compared inside blocks of `block`-character n-grams).
pub fn cluster<'a>(values: &[(&'a str, u32)], method: &str, n: usize, radius: f64, block: usize) -> Result<Vec<Vec<(&'a str, u32)>>> {
    if matches!(method, "levenshtein" | "ppm") {
        return knn(values, method, radius, block);
    }
    key(method, "", n)?; // an unknown keyer fails before any work
    let keys = par_map(values, "Computing keys", |&(v, _)| key(method, v, n))?;
    let mut groups: HashMap<String, Vec<(&str, u32)>> = HashMap::new();
    for (&(v, c), k) in values.iter().zip(keys) {
        groups.entry(k?).or_default().push((v, c)); // an empty key is a key too
    }
    Ok(sorted(groups.into_values().filter(|g| g.len() > 1).collect()))
}

/// `f` over `items` on all cores, in rounds, so the current job shows progress and can be cancelled.
fn par_map<T: Sync, R: Send>(items: &[T], label: &str, f: impl Fn(&T) -> R + Sync) -> Result<Vec<R>> {
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get()).clamp(1, 16);
    crate::jobs::progress(label, items.len() as u64);
    let mut out = Vec::with_capacity(items.len());
    for round in items.chunks(threads * 512) {
        let part = round.len().div_ceil(threads);
        let f = &f;
        let done: Vec<Vec<R>> = std::thread::scope(|s| {
            let handles: Vec<_> = round.chunks(part).map(|c| s.spawn(move || c.iter().map(f).collect::<Vec<R>>())).collect();
            handles.into_iter().map(|h| h.join().expect("a clustering worker panicked")).collect()
        });
        out.extend(done.into_iter().flatten());
        crate::jobs::tick(round.len() as u64)?;
    }
    Ok(out)
}

// ---------------------------------------------------------------- keyers

/// Commons Codec 1.20.0's Beider-Morse rule files, written to a cache folder once: the encoder reads
/// them from disk.
static BM_RULES: &str = include_str!("../assets/commons-codec/bm-rules.txt");
static DM_RULES: &str = include_str!("../assets/commons-codec/dmrules.txt");

fn beider_morse_rules() -> std::result::Result<&'static ConfigFiles, String> {
    static CONFIG: OnceLock<std::result::Result<ConfigFiles, String>> = OnceLock::new();
    CONFIG
        .get_or_init(|| {
            let dir = std::env::temp_dir().join("oxrefine-beider-morse-commons-codec-1.20.0");
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            for part in BM_RULES.split("@@@ ").filter(|p| !p.is_empty()) {
                let (name, body) = part.split_once('\n').ok_or("bad rule bundle")?;
                let path = dir.join(name.trim());
                if std::fs::metadata(&path).map_or(true, |m| m.len() != body.len() as u64) {
                    std::fs::write(&path, body).map_err(|e| e.to_string())?;
                }
            }
            ConfigFiles::new(&dir).map_err(|e| format!("Beider-Morse rules: {e}"))
        })
        .as_ref()
        .map_err(Clone::clone)
}

/// Generic names, approximate rules, words joined: OpenRefine's Beider-Morse keyer.
fn beider_morse() -> std::result::Result<&'static BeiderMorse<'static>, String> {
    static BM: OnceLock<BeiderMorse<'static>> = OnceLock::new();
    let rules = beider_morse_rules()?;
    Ok(BM.get_or_init(|| BeiderMorseBuilder::new(rules).name_type(NameType::Generic).rule_type(RuleType::Approx).concat(true).build()))
}

thread_local! {
    static METAPHONE3: RefCell<crate::metaphone3::Metaphone3> = RefCell::new(crate::metaphone3::Metaphone3::new());
}

fn daitch_mokotoff() -> &'static DaitchMokotoffSoundex {
    static DM: OnceLock<DaitchMokotoffSoundex> = OnceLock::new();
    DM.get_or_init(|| DaitchMokotoffSoundexBuilder::with_rules(DM_RULES).build().expect("bundled Daitch-Mokotoff rules parse"))
}

/// OpenRefine's key-collision keyers by name; `n` is the n-gram size of ngram-fingerprint.
/// Phonetic keys are those OpenRefine computes: Metaphone 3's primary key, Daitch-Mokotoff's first
/// code, and the first Beider-Morse spelling (generic names, approximate rules, words joined).
pub fn key(keyer: &str, s: &str, n: usize) -> Result<String> {
    Ok(match keyer {
        "fingerprint" => fingerprint(s),
        "ngram-fingerprint" => ngram_fingerprint(s, n),
        "metaphone3" => METAPHONE3.with(|m| m.borrow_mut().encode(s).0),
        "cologne-phonetic" => Cologne.encode(s),
        "daitch-mokotoff" => daitch_mokotoff().encode(s),
        // OpenRefine keeps one spelling: the first of Commons Codec's sorted alternatives
        "beider-morse" => beider_morse()?.encode(s).split('|').min().unwrap_or_default().to_string(),
        k => return Err(format!("unknown keyer `{k}` (fingerprint, ngram-fingerprint, metaphone3, cologne-phonetic, daitch-mokotoff, beider-morse)")),
    })
}

/// Commons Codec's Metaphone and Double Metaphone (primary key), as GREL's phonetic() uses them.
pub fn metaphone(s: &str, double: bool) -> String {
    if double {
        crate::double_metaphone::DoubleMetaphone::new(Some(2000)).double_metaphone(s).primary()
    } else {
        // Metaphone encodes no non-ASCII letter; one byte per character keeps rphonetic's byte indexing right
        let ascii: String = s.to_uppercase().chars().map(|c| if c.is_ascii() { c } else { '\u{3}' }).collect();
        Metaphone::new(Some(2000)).encode(&ascii)
    }
}

// ---------------------------------------------------------------- nearest neighbours

/// Edit distance with unit costs (SecondString's Levenstein, as OpenRefine uses it).
pub fn levenshtein(a: &str, b: &str) -> usize {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    within(&a, &b, usize::MAX).unwrap_or(usize::MAX)
}

/// The edit distance of `a` and `b` when it is at most `max`, else None (stops as soon as it must be more).
fn within(a: &[char], b: &[char], max: usize) -> Option<usize> {
    if a.len().abs_diff(b.len()) > max {
        return None;
    }
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut diag = row[0];
        row[0] = i + 1;
        let mut best = row[0];
        for (j, cb) in b.iter().enumerate() {
            let next = (row[j + 1] + 1).min(row[j] + 1).min(diag + usize::from(ca != cb));
            diag = row[j + 1];
            row[j + 1] = next;
            best = best.min(next);
        }
        if best > max {
            return None;
        }
    }
    (row[b.len()] <= max).then_some(row[b.len()])
}

/// vicino's blocking key: trimmed, ASCII punctuation and control characters removed, whitespace runs
/// collapsed, lowercased; then every `n` characters long window. Shorter strings get no block.
fn blocks(s: &str, n: usize) -> Vec<String> {
    // Java's \p{Cntrl} takes tabs and newlines too, before whitespace runs are collapsed
    let mut norm = String::with_capacity(s.len());
    for c in s.trim_matches(|c: char| c <= ' ').chars().filter(|c| !(c.is_ascii_punctuation() || c.is_ascii_control())) {
        if c != ' ' || !norm.ends_with(' ') {
            norm.push(c);
        }
    }
    let chars: Vec<char> = norm.to_lowercase().chars().collect();
    if n == 0 || chars.len() < n {
        return vec![];
    }
    chars.windows(n).map(|w| w.iter().collect()).collect()
}

/// OpenRefine's nearest-neighbour clustering: values sharing an n-gram block are compared, and each
/// value's cluster is it plus every value within `radius` (any distance when negative). Clusters are
/// those neighbour sets, not merged with each other; identical sets count once. This is OpenRefine's
/// answer on one core; with more cores it splits the work and can miss neighbours across the parts.
/// The neighbour sets depend only on which pairs are close, so pairs are checked on all cores.
pub fn knn<'a>(values: &[(&'a str, u32)], distance: &str, radius: f64, block: usize) -> Result<Vec<Vec<(&'a str, u32)>>> {
    let ppm = match distance {
        "levenshtein" => false,
        "ppm" => true,
        m => return Err(format!("unknown distance `{m}` (levenshtein, ppm)")),
    };
    // SecondString compares the strings lowercased
    let lower: Vec<Vec<char>> = values.iter().map(|(v, _)| v.to_lowercase().chars().collect()).collect();
    let max = if radius < 0.0 { usize::MAX } else { radius.floor() as usize };
    let mut grouped: HashMap<String, Vec<u32>> = HashMap::new();
    for (i, &(v, _)) in values.iter().enumerate() {
        for g in blocks(v, block) {
            let ids = grouped.entry(g).or_default();
            if ids.last() != Some(&(i as u32)) {
                ids.push(i as u32);
            }
        }
    }
    // one task per (block, first value): compare it with the values after it in the block
    let blocks: Vec<Vec<u32>> = grouped.into_values().filter(|b| b.len() > 1).collect();
    let tasks: Vec<(usize, usize)> = blocks.iter().enumerate().flat_map(|(k, b)| (0..b.len() - 1).map(move |i| (k, i))).collect();
    let close = par_map(&tasks, "Comparing values", |&(k, i)| {
        let b = &blocks[k];
        let a = b[i];
        // Levenshtein compares lowercased text (SecondString); PPM compares the values as they are (vicino)
        let near = |c: u32| if ppm { crate::ppm::distance(values[a as usize].0, values[c as usize].0) <= radius || radius < 0.0 } else { within(&lower[a as usize], &lower[c as usize], max).is_some() };
        b[i + 1..].iter().filter(|&&c| near(c)).map(|&c| (a, c)).collect::<Vec<_>>()
    })?;
    let mut near: HashMap<u32, BTreeSet<&str>> = HashMap::new();
    for (a, b) in close.into_iter().flatten() {
        let (va, vb) = (values[a as usize].0, values[b as usize].0);
        near.entry(a).or_insert_with(|| BTreeSet::from([va])).insert(vb);
        near.entry(b).or_insert_with(|| BTreeSet::from([vb])).insert(va);
    }
    let counts: HashMap<&str, u32> = values.iter().copied().collect();
    let sets: BTreeSet<BTreeSet<&str>> = near.into_values().filter(|s| s.len() > 1).collect();
    Ok(sorted(sets.into_iter().map(|s| s.into_iter().map(|v| (v, counts[v])).collect()).collect()))
}

/// Biggest clusters first; members by count, then alphabetically, so the first is the merge suggestion.
fn sorted<'a>(mut clusters: Vec<Vec<(&'a str, u32)>>) -> Vec<Vec<(&'a str, u32)>> {
    for c in &mut clusters {
        c.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
    }
    clusters.sort_by(|a, b| b.len().cmp(&a.len()).then_with(|| b.iter().map(|x| x.1).sum::<u32>().cmp(&a.iter().map(|x| x.1).sum())).then(a[0].0.cmp(b[0].0)));
    clusters
}
