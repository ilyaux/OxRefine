//! OpenRefine's PPM distance for nearest-neighbour clustering (vicino's `PPMDistance`): how much
//! longer two strings compress together than each with itself, under Bob Carpenter's arithcode
//! (an order-8 PPM model with exclusions over a 27-bit arithmetic coder), which OpenRefine bundles.
//! Ported from arithcode 1.2 as OpenRefine 3.10.1 ships it, quirks included (the context buffer keeps
//! one stale byte once it has wrapped around), because the distance depends on the exact number of
//! bytes the coder writes. Only that number is computed: the coder counts bits instead of writing them.

/// vicino's PseudoMetricDistance over PPM: 10 × ((c(xy) + c(yx)) / (c(xx) + c(yy)) − 1), where c is
/// the compressed length in bytes of the strings' UTF-8 (Java's default encoding) one after the other.
pub fn distance(x: &str, y: &str) -> f64 {
    let c = |a: &str, b: &str| compressed_len(&[a.as_bytes(), b.as_bytes()].concat()) as f64;
    10.0 * ((c(x, y) + c(y, x)) / (c(x, x) + c(y, y)) - 1.0)
}

/// Bytes `ArithCodeOutputStream(PPMModel(8))` writes for `bytes` followed by its end-of-file symbol.
pub fn compressed_len(bytes: &[u8]) -> usize {
    let mut model = Model::new(8);
    let mut coder = Coder::default();
    for &b in bytes {
        encode(&mut model, &mut coder, b as i32);
    }
    encode(&mut model, &mut coder, EOF);
    coder.close().div_ceil(8) as usize
}

const EOF: i32 = -1;
const ESCAPE: i32 = -2;

/// ArithCodeOutputStream.encode: escape down the contexts until one predicts the symbol, then code it.
fn encode(model: &mut Model, coder: &mut Coder, symbol: i32) {
    while model.escaped(symbol) {
        let iv = model.interval(ESCAPE);
        coder.encode(iv);
    }
    let iv = model.interval(symbol);
    coder.encode(iv);
}

// ---------------------------------------------------------------- the arithmetic coder

const HALF: i64 = 67_108_864;
const FIRST_QUARTER: i64 = 33_554_432;
const THIRD_QUARTER: i64 = 100_663_296;
const TOP: i64 = 134_217_727;

/// ArithEncoder with 27-bit code values; `bits` counts what BitOutput would write.
struct Coder {
    low: i64,
    high: i64,
    follow: u64,
    bits: u64,
}

impl Default for Coder {
    fn default() -> Self {
        Coder { low: 0, high: TOP, follow: 0, bits: 0 }
    }
}

impl Coder {
    fn bit(&mut self) {
        self.bits += 1 + self.follow;
        self.follow = 0;
    }
    fn encode(&mut self, (low, high, total): (i32, i32, i32)) {
        let range = self.high - self.low + 1;
        self.high = self.low + range * high as i64 / total as i64 - 1;
        self.low += range * low as i64 / total as i64;
        loop {
            if self.high < HALF {
                self.bit();
            } else if self.low >= HALF {
                self.bit();
                self.low -= HALF;
                self.high -= HALF;
            } else if self.low >= FIRST_QUARTER && self.high < THIRD_QUARTER {
                self.follow += 1;
                self.low -= FIRST_QUARTER;
                self.high -= FIRST_QUARTER;
            } else {
                return;
            }
            self.low <<= 1;
            self.high = (self.high << 1) + 1;
        }
    }
    /// The closing bit and its followers; the total number of bits written.
    fn close(mut self) -> u64 {
        self.follow += 1;
        self.bit();
        self.bits
    }
}

// ---------------------------------------------------------------- the model

/// PPMNode: a byte in a context trie, its count and how many distinct bytes followed it.
struct Node {
    byte: u8,
    count: i16,
    outcomes: i16,
    first: Option<u32>,
    next: Option<u32>,
}

/// ByteBuffer: the last bytes seen, in a 32× wider array that is moved down when full.
struct Buffer {
    bytes: Vec<u8>,
    max: usize,
    offset: usize,
    length: usize,
}

impl Buffer {
    fn new(max: usize) -> Buffer {
        Buffer { bytes: vec![0; 32 * max], max, offset: 0, length: 0 }
    }
    fn push(&mut self, b: u8) {
        if self.offset + self.length > self.bytes.len() - 1 {
            // tampDown: moves length - 1 bytes and leaves the next one as it was (arithcode does this)
            for i in 0..self.length.saturating_sub(1) {
                self.bytes[i] = self.bytes[self.offset + i + 1];
            }
            self.offset = 0;
        }
        self.bytes[self.offset + self.length] = b;
        if self.length < self.max {
            self.length += 1;
        } else {
            self.offset += 1;
        }
    }
}

struct Model {
    nodes: Vec<Node>,
    /// a trie per first byte of the buffer
    contexts: [Option<u32>; 256],
    max_len: usize,
    context_len: usize,
    context: Option<u32>,
    buffer: Buffer,
    excluded: [bool; 256],
    /// ExcludingAdaptiveUnigramModel: every byte and end-of-file (256) start at 1
    unigram: [i32; 257],
}

impl Model {
    fn new(max_len: usize) -> Model {
        Model { nodes: vec![], contexts: [None; 256], max_len, context_len: 0, context: None, buffer: Buffer::new(max_len + 1), excluded: [false; 256], unigram: [1; 257] }
    }

    fn node(&mut self, byte: u8, next: Option<u32>) -> u32 {
        self.nodes.push(Node { byte, count: 1, outcomes: 0, first: None, next });
        (self.nodes.len() - 1) as u32
    }

    fn children(&self, n: u32) -> impl Iterator<Item = u32> + '_ {
        std::iter::successors(self.nodes[n as usize].first, |&c| self.nodes[c as usize].next)
    }

    fn escaped(&self, symbol: i32) -> bool {
        match self.context {
            Some(n) => symbol == EOF || !self.children(n).any(|c| self.nodes[c as usize].byte == symbol as u8),
            None => false,
        }
    }

    fn interval(&mut self, symbol: i32) -> (i32, i32, i32) {
        match symbol {
            EOF => self.unigram_interval(256),
            ESCAPE => self.interval_escape(),
            b => {
                let iv = match self.context {
                    Some(n) => self.node_interval(n, b as u8),
                    None => self.unigram_interval(b as usize),
                };
                self.increment(b as u8);
                iv
            }
        }
    }

    // --- PPMNode

    fn total(&self, n: u32) -> i32 {
        self.nodes[n as usize].outcomes as i32 + self.children(n).filter(|&c| !self.excluded[self.nodes[c as usize].byte as usize]).map(|c| self.nodes[c as usize].count as i32).sum::<i32>()
    }

    fn node_interval(&self, n: u32, b: u8) -> (i32, i32, i32) {
        let mut low = 0;
        let mut child = self.nodes[n as usize].first;
        while let Some(c) = child {
            let node = &self.nodes[c as usize];
            if !self.excluded[node.byte as usize] {
                if node.byte == b {
                    let high = low + node.count as i32;
                    let mut total = high + self.nodes[n as usize].outcomes as i32;
                    let mut rest = node.next;
                    while let Some(r) = rest {
                        if !self.excluded[self.nodes[r as usize].byte as usize] {
                            total += self.nodes[r as usize].count as i32;
                        }
                        rest = self.nodes[r as usize].next;
                    }
                    return (low, high, total);
                }
                low += node.count as i32;
            }
            child = node.next;
        }
        unreachable!("a byte the context doesn't predict is escaped first")
    }

    fn is_childless(&self, n: u32) -> bool {
        self.children(n).all(|c| self.excluded[self.nodes[c as usize].byte as usize])
    }

    /// Only one child, whatever is excluded (as arithcode checks it).
    fn is_deterministic(&self, n: u32) -> bool {
        let first = self.nodes[n as usize].first.expect("checked after is_childless");
        self.nodes[first as usize].next.is_none()
    }

    /// PPMNode.complete: a chain of new nodes for bytes[off..off + len].
    fn complete(&mut self, n: u32, bytes: &[u8], off: usize, len: usize) {
        let mut node = n;
        for &b in &bytes[off..off + len] {
            self.nodes[node as usize].outcomes += 1;
            let child = self.node(b, None);
            self.nodes[node as usize].first = Some(child);
            node = child;
        }
    }

    /// PPMNode.increment: count the path bytes[off..off + len] under n, moving a found byte to the front.
    fn node_increment(&mut self, n: u32, bytes: &[u8], off: usize, len: usize) {
        let Some(first) = self.nodes[n as usize].first else {
            self.nodes[n as usize].outcomes += 1;
            let child = self.node(bytes[off], None);
            self.nodes[n as usize].first = Some(child);
            if len > 1 {
                self.complete(child, bytes, off + 1, len - 1);
            }
            return;
        };
        let mut prev: Option<u32> = None;
        let mut child = first;
        loop {
            if self.nodes[child as usize].byte == bytes[off] {
                if len > 1 {
                    self.node_increment(child, bytes, off + 1, len - 1);
                }
                if let Some(p) = prev {
                    self.nodes[p as usize].next = self.nodes[child as usize].next;
                    self.nodes[child as usize].next = self.nodes[n as usize].first;
                    self.nodes[n as usize].first = Some(child);
                }
                self.nodes[child as usize].count += 1;
                if self.nodes[child as usize].count > 8192 {
                    self.rescale(n);
                }
                return;
            }
            match self.nodes[child as usize].next {
                Some(next) => {
                    prev = Some(child);
                    child = next;
                }
                None => {
                    self.nodes[n as usize].outcomes += 1;
                    let head = self.nodes[n as usize].first;
                    let new = self.node(bytes[off], head);
                    self.nodes[n as usize].first = Some(new);
                    if len > 1 {
                        self.complete(new, bytes, off + 1, len - 1);
                    }
                    return;
                }
            }
        }
    }

    fn rescale(&mut self, n: u32) {
        let outcomes = self.nodes[n as usize].outcomes;
        self.nodes[n as usize].outcomes = (outcomes + 1) / 2;
        let first = self.nodes[n as usize].first;
        self.nodes[n as usize].first = first.and_then(|f| self.rescale_siblings(f));
    }

    /// Halve every count; drop the nodes that fall under 128.
    fn rescale_siblings(&mut self, n: u32) -> Option<u32> {
        self.nodes[n as usize].count >>= 1;
        let keep = self.nodes[n as usize].count >= 128;
        match self.nodes[n as usize].next {
            None => keep.then_some(n),
            Some(next) if !keep => self.rescale_siblings(next),
            Some(next) => {
                let rest = self.rescale_siblings(next);
                self.nodes[n as usize].next = rest;
                Some(n)
            }
        }
    }

    // --- ExcludingAdaptiveUnigramModel

    fn unigram_interval(&mut self, i: usize) -> (i32, i32, i32) {
        let low: i32 = (0..i).filter(|&j| !self.excluded[j]).map(|j| self.unigram[j]).sum();
        let high = low + self.unigram[i];
        let mut total = high + (i + 1..256).filter(|&j| !self.excluded[j]).map(|j| self.unigram[j]).sum::<i32>();
        if i != 256 {
            total += self.unigram[256];
        }
        self.unigram[i] += 1;
        if self.unigram[i] > 8192 {
            for c in self.unigram.iter_mut() {
                *c = (*c + 1) / 2;
            }
        }
        (low, high, total)
    }

    // --- PPMModel

    fn interval_escape(&mut self) -> (i32, i32, i32) {
        let n = self.context.expect("an escape has a context");
        let total = self.total(n);
        let iv = (total - self.nodes[n as usize].outcomes as i32, total, total);
        if self.context_len >= 1 {
            let bytes: Vec<u8> = self.children(n).map(|c| self.nodes[c as usize].byte).collect();
            for b in bytes {
                self.excluded[b as usize] = true;
            }
        }
        self.context_len = self.context_len.saturating_sub(1);
        self.longest_to_shortest();
        iv
    }

    fn increment(&mut self, b: u8) {
        self.buffer.push(b);
        let (off, len) = (self.buffer.offset, self.buffer.length);
        let first = self.buffer.bytes[off];
        let root = match self.contexts[first as usize] {
            Some(r) => r,
            None => {
                let r = self.node(first, None);
                self.contexts[first as usize] = Some(r);
                r
            }
        };
        if len > 1 {
            // the trie update doesn't touch the buffer, so lend it its bytes instead of copying them
            let bytes = std::mem::take(&mut self.buffer.bytes);
            self.node_increment(root, &bytes, off + 1, len - 1);
            self.buffer.bytes = bytes;
        }
        self.context_len = self.max_len.min(len);
        self.binary_search();
        self.excluded = [false; 256];
    }

    /// The node for the last `len` bytes of the buffer, if that context was seen.
    fn lookup_node(&self, len: usize) -> Option<u32> {
        let b = &self.buffer;
        let at = b.offset + b.length - len;
        let mut node = self.contexts[b.bytes[at] as usize]?;
        let (mut off, mut left) = (at + 1, len - 1);
        if left == 0 {
            return Some(node);
        }
        let mut child = self.nodes[node as usize].first;
        while left > 0 {
            let c = child?;
            if b.bytes[off] == self.nodes[c as usize].byte {
                if left == 1 {
                    return Some(c);
                }
                node = c;
                child = self.nodes[node as usize].first;
                off += 1;
                left -= 1;
            } else {
                child = self.nodes[c as usize].next;
            }
        }
        None
    }

    /// getContextNodeBinarySearch: the longest context with children, preferring a deterministic one.
    fn binary_search(&mut self) {
        let (mut lo, mut hi) = (1usize, self.context_len);
        self.context_len = 0;
        self.context = None;
        let mut deterministic = false;
        while hi >= lo {
            let mid = (hi + lo) / 2;
            match self.lookup_node(mid) {
                Some(n) if !self.is_childless(n) => {
                    if self.is_deterministic(n) {
                        self.context_len = mid;
                        self.context = Some(n);
                        deterministic = true;
                        if mid < hi {
                            hi = mid;
                        } else {
                            hi -= 1;
                        }
                    } else {
                        if !deterministic {
                            self.context_len = mid;
                            self.context = Some(n);
                        }
                        if mid > lo {
                            lo = mid;
                        } else {
                            lo += 1;
                        }
                    }
                }
                _ => {
                    if mid < hi {
                        hi = mid;
                    } else {
                        hi -= 1;
                    }
                }
            }
        }
    }

    /// getContextNodeLongToShort: after an escape, the longest shorter context that still predicts
    /// something, then shorter ones while both it and the next are deterministic.
    fn longest_to_shortest(&mut self) {
        loop {
            if self.context_len < 1 {
                self.context = None;
                return;
            }
            self.context = self.lookup_node(self.context_len);
            match self.context {
                Some(n) if !self.is_childless(n) => break,
                _ => self.context_len -= 1,
            }
        }
        while self.context_len > 1 && self.context.is_some_and(|n| self.is_deterministic(n)) {
            // arithcode calls isDeterministic on the shorter node directly; a leaf there would throw in
            // Java, here it ends the walk
            match self.lookup_node(self.context_len - 1) {
                Some(m) if self.nodes[m as usize].first.is_some_and(|f| self.nodes[f as usize].next.is_none()) => {
                    self.context = Some(m);
                    self.context_len -= 1;
                }
                _ => return,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    /// Every string in tests/cluster/ppm-fixtures.json (made by OpenRefine's own arithcode and vicino jars):
    /// the same compressed length to the byte and the same distance to the next string.
    #[test]
    fn matches_arithcode() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/cluster/ppm-fixtures.json");
        let fx: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        let cases = fx["cases"].as_array().unwrap();
        let mut bad = vec![];
        for (i, c) in cases.iter().enumerate() {
            let s = c["s"].as_str().unwrap();
            let next = cases[(i + 1) % cases.len()]["s"].as_str().unwrap();
            let len = super::compressed_len(s.as_bytes());
            let want = c["len"].as_u64().unwrap() as usize;
            let d = super::distance(s, next);
            let want_d = c["distanceToNext"].as_f64().unwrap();
            if len != want || (d - want_d).abs() > 1e-12 {
                bad.push(format!("{:?} ({} bytes): length {len}, OpenRefine {want}; distance {d}, OpenRefine {want_d}", &s.chars().take(40).collect::<String>(), s.len()));
            }
        }
        assert!(bad.is_empty(), "{} of {} differ:\n{}", bad.len(), cases.len(), bad.join("\n"));
    }
}
