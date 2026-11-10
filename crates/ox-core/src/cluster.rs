//! Key-collision clustering (OpenRefine's fingerprint and n-gram fingerprint keyers).
use crate::Result;
use std::{cmp::Reverse, collections::HashMap};
use unicode_normalization::{char::is_combining_mark, UnicodeNormalization};

fn asciify(s: &str) -> String {
    s.nfd().filter(|c| !is_combining_mark(*c)).collect()
}

fn keep(c: char) -> bool {
    !c.is_ascii_punctuation() && !c.is_control()
}

pub fn fingerprint(s: &str) -> String {
    let s: String = asciify(&s.trim().to_lowercase()).chars().filter(|&c| keep(c)).collect();
    let mut t: Vec<&str> = s.split_whitespace().collect();
    t.sort_unstable();
    t.dedup();
    t.join(" ")
}

pub fn ngram_fingerprint(s: &str, n: usize) -> String {
    let c: Vec<char> = asciify(&s.to_lowercase()).chars().filter(|&c| keep(c) && !c.is_whitespace()).collect();
    if c.len() <= n {
        return c.into_iter().collect();
    }
    let mut g: Vec<String> = c.windows(n).map(|w| w.iter().collect()).collect();
    g.sort_unstable();
    g.dedup();
    g.concat()
}

/// Groups of distinct values sharing a key, biggest groups first; each group is
/// ordered by count, so the first value is the suggested merge target.
pub fn cluster<'a>(values: &[(&'a str, u32)], method: &str, n: usize) -> Result<Vec<Vec<(&'a str, u32)>>> {
    let ngram = match method {
        "fingerprint" => false,
        "ngram-fingerprint" => true,
        m => return Err(format!("unknown clustering method `{m}` (use fingerprint or ngram-fingerprint)")),
    };
    let mut groups: HashMap<String, Vec<(&str, u32)>> = HashMap::new();
    for &(v, c) in values {
        let key = if ngram { ngram_fingerprint(v, n.max(1)) } else { fingerprint(v) };
        if !key.is_empty() {
            groups.entry(key).or_default().push((v, c)); // values arrive count-desc, so groups stay sorted
        }
    }
    let mut out: Vec<_> = groups.into_values().filter(|g| g.len() > 1).collect();
    out.sort_by_key(|g| (Reverse(g.len()), Reverse(g.iter().map(|x| x.1).sum::<u32>()), g[0].0));
    Ok(out)
}
