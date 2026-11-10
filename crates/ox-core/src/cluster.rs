//! Key-collision clustering with OpenRefine's fingerprint and n-gram fingerprint keyers.
use crate::{grel::funcs, Result};
use std::{cmp::Reverse, collections::HashMap};
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
