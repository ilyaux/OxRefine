//! Immutable row codes, with an explicit, shared budget for their heap allocations.
//! Dictionaries and operation scratch space are outside this budget; mapped pages are OS-managed.
use crate::{err, jobs, Result};
use memmap2::{Mmap, MmapOptions};
use serde::Serialize;
use std::{
    collections::BTreeMap,
    fs::File,
    io::{BufWriter, Write},
    ops::Deref,
    path::PathBuf,
    sync::{atomic::{AtomicUsize, Ordering::Relaxed}, Arc},
};

const BUFFER: usize = 64 * 1024;
const GROW_CODES: usize = BUFFER / 4;

#[derive(Default)]
struct Counter { current: AtomicUsize, peak: AtomicUsize }
impl Counter {
    fn add(&self, bytes: usize) {
        let now = self.current.fetch_add(bytes, Relaxed) + bytes;
        self.peak.fetch_max(now, Relaxed);
    }
    fn sub(&self, bytes: usize) { self.current.fetch_sub(bytes, Relaxed); }
}

struct Budget {
    limit: usize,
    dir: PathBuf,
    heap: Counter,
    mapped: Counter,
    buffers: Counter,
    files: AtomicUsize,
}

/// Clones share one reservation pool, including across CSV worker threads and history snapshots.
#[derive(Clone)]
pub struct CodeBudget(Arc<Budget>);

/// Tracked builder allocations, not total Codes/process RAM: infallible `From<Vec>` allocations
/// are excluded. Mapped pages and the filesystem cache may remain resident.
#[derive(Clone, Copy, Debug, Serialize)]
pub struct CodeStats {
    pub heap_bytes: usize,
    pub heap_peak_bytes: usize,
    pub mapped_bytes: usize,
    pub mapped_peak_bytes: usize,
    pub buffer_bytes: usize,
    pub buffer_peak_bytes: usize,
    pub spill_files: usize,
}

impl CodeBudget {
    pub fn new(bytes: usize, spill_dir: PathBuf) -> Self {
        Self(Arc::new(Budget { limit: bytes, dir: spill_dir, heap: Counter::default(), mapped: Counter::default(), buffers: Counter::default(), files: AtomicUsize::new(0) }))
    }
    pub fn heap_limit(&self) -> usize { self.0.limit }
    pub fn stats(&self) -> CodeStats {
        let b = &self.0;
        CodeStats { heap_bytes: b.heap.current.load(Relaxed), heap_peak_bytes: b.heap.peak.load(Relaxed),
            mapped_bytes: b.mapped.current.load(Relaxed), mapped_peak_bytes: b.mapped.peak.load(Relaxed),
            buffer_bytes: b.buffers.current.load(Relaxed), buffer_peak_bytes: b.buffers.peak.load(Relaxed),
            spill_files: b.files.load(Relaxed) }
    }
    fn reserve(&self, bytes: usize) -> Option<Charge> {
        let heap = &self.0.heap;
        let old = heap.current.fetch_update(Relaxed, Relaxed, |used| used.checked_add(bytes).filter(|&n| n <= self.0.limit)).ok()?;
        heap.peak.fetch_max(old + bytes, Relaxed);
        Some(Charge { budget: self.clone(), bytes, kind: Kind::Heap })
    }
    fn charge(&self, bytes: usize, kind: Kind) -> Charge {
        match kind { Kind::Heap => unreachable!(), Kind::Mapped => self.0.mapped.add(bytes), Kind::Buffer => self.0.buffers.add(bytes) }
        Charge { budget: self.clone(), bytes, kind }
    }
}

enum Kind { Heap, Mapped, Buffer }
struct Charge { budget: CodeBudget, bytes: usize, kind: Kind }
impl Drop for Charge {
    fn drop(&mut self) {
        match self.kind { Kind::Heap => &self.budget.0.heap, Kind::Mapped => &self.budget.0.mapped, Kind::Buffer => &self.budget.0.buffers }.sub(self.bytes);
    }
}
struct FileCharge(CodeBudget);
impl FileCharge {
    fn new(budget: CodeBudget) -> Self { budget.0.files.fetch_add(1, Relaxed); Self(budget) }
}
impl Drop for FileCharge { fn drop(&mut self) { self.0.0.files.fetch_sub(1, Relaxed); } }

#[derive(Default)]
struct Heap { values: Vec<u32>, charge: Option<Charge> }
// Drop the map BEFORE closing its private delete-on-close file (important on Windows).
struct Mapped { map: Mmap, _file: File, charge: Charge, _file_charge: FileCharge }
enum Backing { Heap(Heap), Mapped(Mapped) }

/// Cheap immutable snapshots. Disk-backed storage never exposes a path or a writable handle.
#[derive(Clone)]
pub struct Codes(Arc<Backing>);
impl Default for Codes { fn default() -> Self { Vec::new().into() } }
impl From<Vec<u32>> for Codes {
    fn from(values: Vec<u32>) -> Self { Self(Arc::new(Backing::Heap(Heap { values, charge: None }))) }
}
impl Deref for Codes {
    type Target = [u32];
    fn deref(&self) -> &[u32] {
        match &*self.0 {
            Backing::Heap(h) => &h.values,
            // Only created from this module's private native-endian, aligned, whole-u32 file.
            Backing::Mapped(m) => unsafe { std::slice::from_raw_parts(m.map.as_ptr().cast::<u32>(), m.map.len() / 4) },
        }
    }
}
impl AsRef<[u32]> for Codes { fn as_ref(&self) -> &[u32] { self } }
impl PartialEq for Codes { fn eq(&self, other: &Self) -> bool { self.ptr_eq(other) || **self == **other } }
impl Eq for Codes {}
impl std::fmt::Debug for Codes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { f.debug_struct("Codes").field("len", &self.len()).field("mapped", &self.is_mapped()).finish() }
}
impl Codes {
    pub fn ptr_eq(&self, other: &Self) -> bool { Arc::ptr_eq(&self.0, &other.0) }
    pub fn is_mapped(&self) -> bool { matches!(&*self.0, Backing::Mapped(_)) }
    pub fn mapped_bytes(&self) -> usize { if self.is_mapped() { self.len() * 4 } else { 0 } }
    pub fn budget(&self) -> Option<CodeBudget> {
        match &*self.0 { Backing::Heap(h) => h.charge.as_ref().map(|c| c.budget.clone()), Backing::Mapped(m) => Some(m.charge.budget.clone()) }
    }
    /// A new snapshot, or an error with the original unchanged. Repeated indices use the last edit.
    pub fn with_edits(&self, edits: &[(usize, u32)], budget: CodeBudget) -> Result<Self> {
        if edits.iter().any(|&(r, _)| r >= self.len()) { return Err("row code edit is outside the column".into()); }
        if edits.is_empty() { return Ok(self.clone()); }
        let edits: BTreeMap<usize, u32> = edits.iter().copied().collect();
        let mut out = CodeBuilder::new(budget);
        let mut start = 0;
        for (row, code) in edits {
            out.extend_slice(&self[start..row])?;
            out.push(code)?;
            start = row + 1;
        }
        out.extend_slice(&self[start..])?;
        out.finish()
    }
}

struct Spilled {
    writer: BufWriter<File>,
    buffer_charge: Charge,
    file_charge: FileCharge,
}
enum Building { Heap(Heap), File(Spilled) }

/// Append-only codes: heap capacity is reserved against a shared budget before allocation.
/// File buffers are separately reported (at most 64 KiB per spilled builder).
pub struct CodeBuilder { budget: CodeBudget, state: Building, len: usize, failed: bool }
impl CodeBuilder {
    pub fn new(budget: CodeBudget) -> Self { Self { budget, state: Building::Heap(Heap::default()), len: 0, failed: false } }
    pub fn len(&self) -> usize { self.len }
    pub fn is_empty(&self) -> bool { self.len == 0 }
    pub fn push(&mut self, code: u32) -> Result<()> { self.extend_slice(&[code]) }
    pub fn extend_repeated(&mut self, code: u32, n: usize) -> Result<()> {
        let block = [code; GROW_CODES];
        for _ in 0..n / GROW_CODES { self.extend_slice(&block)?; }
        self.extend_slice(&block[..n % GROW_CODES])
    }
    pub fn extend_slice(&mut self, codes: &[u32]) -> Result<()> {
        let result = self.append(codes);
        if result.is_err() { self.failed = true; }
        result
    }
    fn append(&mut self, codes: &[u32]) -> Result<()> {
        if self.failed { return Err("row code builder has failed".into()); }
        let len = self.len.checked_add(codes.len()).filter(|n| n.checked_mul(4).is_some_and(|b| b <= isize::MAX as usize)).ok_or("row code column is too large")?;
        if self.len % GROW_CODES == 0 || codes.len() >= GROW_CODES { jobs::tick(0)?; }
        if codes.is_empty() { return Ok(()); }
        if let Building::Heap(heap) = &mut self.state {
            if len > heap.values.capacity() {
                // Reserve the entire new allocation while the old one is still live.
                let available = self.budget.0.limit.saturating_sub(self.budget.0.heap.current.load(Relaxed)) / 4;
                let capacity = heap.values.capacity().saturating_mul(2).max(64).max(len).min(available);
                let new_heap = if capacity >= len {
                    if let Some(mut charge) = self.budget.reserve(capacity * 4) {
                        let mut values = Vec::new();
                        values.try_reserve_exact(capacity).map_err(|e| format!("allocating row codes: {e}"))?;
                        let actual = values.capacity() * 4;
                        if actual > charge.bytes {
                            // Allocators may expose more capacity than requested; account for it too.
                            if let Some(mut extra) = self.budget.reserve(actual - charge.bytes) {
                                charge.bytes = actual;
                                extra.bytes = 0;
                            } else { return self.spill_then_append(codes, len); }
                        }
                        values.extend_from_slice(&heap.values);
                        Some(Heap { values, charge: Some(charge) })
                    } else { None }
                } else { None };
                if let Some(new_heap) = new_heap { *heap = new_heap; } else { return self.spill_then_append(codes, len); }
            }
            heap.values.extend_from_slice(codes);
        } else if let Building::File(file) = &mut self.state { write_codes(&mut file.writer, codes)?; }
        self.len = len;
        Ok(())
    }
    fn spill_then_append(&mut self, codes: &[u32], len: usize) -> Result<()> {
        let file = tempfile::tempfile_in(&self.budget.0.dir).map_err(|e| format!("creating spilled row codes: {e}"))?;
        let file_charge = FileCharge::new(self.budget.clone());
        let mut writer = BufWriter::with_capacity(BUFFER, file);
        let buffer_charge = self.budget.charge(writer.capacity(), Kind::Buffer);
        if let Building::Heap(heap) = &self.state { write_codes(&mut writer, &heap.values)?; }
        write_codes(&mut writer, codes)?;
        self.state = Building::File(Spilled { writer, buffer_charge, file_charge });
        self.len = len;
        Ok(())
    }
    pub fn finish(self) -> Result<Codes> {
        if self.failed { return Err("row code builder has failed".into()); }
        jobs::tick(0)?;
        match self.state {
            Building::Heap(heap) => Ok(Codes(Arc::new(Backing::Heap(heap)))),
            Building::File(Spilled { mut writer, buffer_charge, file_charge }) => {
                writer.flush().map_err(|e| format!("flushing spilled row codes: {e}"))?;
                let file = writer.into_inner().map_err(|e| format!("closing spilled row code writer: {e}"))?;
                drop(buffer_charge);
                let bytes = self.len * 4;
                if file.metadata().map_err(err)?.len() != bytes as u64 { return Err("spilled row code length mismatch".into()); }
                jobs::tick(0)?;
                // SAFETY: only this module owns the anonymous/delete-on-close file; its sole
                // writable handle is sealed here and is never exposed or modified after mapping.
                let map = unsafe { MmapOptions::new().len(bytes).map(&file) }.map_err(|e| format!("mapping spilled row codes: {e}"))?;
                if map.as_ptr().align_offset(std::mem::align_of::<u32>()) != 0 { return Err("spilled row codes are not aligned".into()); }
                let charge = self.budget.charge(bytes, Kind::Mapped);
                Ok(Codes(Arc::new(Backing::Mapped(Mapped { map, _file: file, charge, _file_charge: file_charge }))))
            }
        }
    }
}

fn write_codes(writer: &mut impl Write, codes: &[u32]) -> Result<()> {
    for (i, block) in codes.chunks(GROW_CODES).enumerate() {
        if i > 0 { jobs::tick(0)?; }
        // Native-endian bytes are private temporary storage, never the persistent OXT format.
        let bytes = unsafe { std::slice::from_raw_parts(block.as_ptr().cast::<u8>(), block.len() * 4) };
        writer.write_all(bytes).map_err(|e| format!("writing spilled row codes: {e}"))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn low_budget_streams_codes_and_snapshots_keep_the_private_file_alive() {
        let dir = tempfile::tempdir().unwrap();
        let budget = CodeBudget::new(1024, dir.path().into());
        let mut builder = CodeBuilder::new(budget.clone());
        for i in 0..250_001 { builder.push(i % 37).unwrap(); }
        let codes = builder.finish().unwrap();
        assert!(codes.is_mapped());
        assert_eq!(codes.mapped_bytes(), 1_000_004);
        assert!(codes.iter().enumerate().all(|(i, &c)| c == i as u32 % 37));
        let snapshot = codes.clone();
        assert!(Codes::ptr_eq(&codes, &snapshot));
        let stats = budget.stats();
        assert_eq!(stats.spill_files, 1);
        assert_eq!(stats.mapped_bytes, 1_000_004);
        assert_eq!(stats.heap_bytes, 0);
        assert_eq!(stats.buffer_bytes, 0);
        assert!(stats.heap_peak_bytes <= 1024);
        drop(codes);
        assert_eq!(snapshot[250_000], 250_000 % 37);
        assert_eq!(budget.stats().spill_files, 1);
        drop(snapshot);
        assert_eq!(budget.stats().mapped_bytes, 0);
        assert_eq!(budget.stats().spill_files, 0);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0, "Windows delete-on-close files must be gone after the last snapshot");
    }

    #[test]
    fn shared_budget_counts_capacity_and_transient_growth_across_workers() {
        let dir = tempfile::tempdir().unwrap();
        let budget = CodeBudget::new(32 * 1024, dir.path().into());
        let handles: Vec<_> = (0..4).map(|column| {
            let budget = budget.clone();
            std::thread::spawn(move || {
                let mut out = CodeBuilder::new(budget);
                for _ in 0..50_000 { out.push(column).unwrap(); }
                out.finish().unwrap()
            })
        }).collect();
        let codes: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        assert!(codes.iter().enumerate().all(|(c, v)| v.is_mapped() && v.iter().all(|&x| x == c as u32)));
        let stats = budget.stats();
        assert_eq!(stats.mapped_bytes, 800_000);
        assert_eq!(stats.spill_files, 4);
        assert!(stats.heap_peak_bytes <= 32 * 1024, "{stats:?}");
        assert_eq!(stats.heap_bytes, 0);
        drop(codes);
        assert_eq!(budget.stats().spill_files, 0);
        assert_eq!(budget.stats().mapped_bytes, 0);
    }

    #[test]
    fn copy_on_write_edits_preserve_history_and_io_failure_never_falls_back_to_heap() {
        let dir = tempfile::tempdir().unwrap();
        let budget = CodeBudget::new(0, dir.path().into());
        let mut out = CodeBuilder::new(budget.clone());
        out.extend_slice(&[4, 8, 15, 16, 23, 42]).unwrap();
        let old = out.finish().unwrap();
        let snapshot = old.clone();
        let edited = old.with_edits(&[(4, 99), (0, 1), (4, 100)], budget.clone()).unwrap();
        assert_eq!(&*edited, &[1, 8, 15, 16, 100, 42]);
        assert_eq!(&*old, &[4, 8, 15, 16, 23, 42]);
        assert!(old.ptr_eq(&snapshot));
        assert!(old.with_edits(&[(6, 5)], budget.clone()).unwrap_err().contains("outside"));
        let bad_dir = dir.path().join("not-a-directory");
        std::fs::write(&bad_dir, b"owned test file").unwrap();
        let failed_budget = CodeBudget::new(0, bad_dir);
        assert!(old.with_edits(&[(0, 2)], failed_budget.clone()).unwrap_err().contains("creating spilled"));
        let mut failed = CodeBuilder::new(failed_budget.clone());
        assert!(failed.push(9).is_err());
        assert!(failed.finish().unwrap_err().contains("failed"));
        assert_eq!(failed_budget.stats().spill_files, 0);
        assert_eq!(failed_budget.stats().heap_bytes, 0);
        assert_eq!(&*snapshot, &[4, 8, 15, 16, 23, 42]);
        drop((snapshot, old, edited));
        assert_eq!(budget.stats().spill_files, 0);
    }

    #[test]
    fn heap_reservations_are_shared_until_the_last_snapshot_and_empty_never_maps() {
        let dir = tempfile::tempdir().unwrap();
        let budget = CodeBudget::new(1024 * 1024, dir.path().into());
        let mut small = CodeBuilder::new(budget.clone());
        for i in 0..10 { small.push(i).unwrap(); }
        let small = small.finish().unwrap();
        assert!(budget.stats().heap_bytes <= 1024, "tiny wide columns must not each reserve a 64 KiB I/O chunk");
        drop(small);
        let mut out = CodeBuilder::new(budget.clone());
        for i in 0..40_000 { out.push(i).unwrap(); }
        let codes = out.finish().unwrap();
        assert!(!codes.is_mapped());
        let heap = budget.stats().heap_bytes;
        assert!(heap >= 160_000);
        assert!(budget.stats().heap_peak_bytes <= budget.heap_limit());
        let snapshot = codes.clone();
        assert_eq!(budget.stats().heap_bytes, heap);
        drop(codes);
        assert_eq!(budget.stats().heap_bytes, heap);
        drop(snapshot);
        assert_eq!(budget.stats().heap_bytes, 0);
        let empty = CodeBuilder::new(CodeBudget::new(0, dir.path().join("missing"))).finish().unwrap();
        assert!(empty.is_empty());
        assert!(!empty.is_mapped());
        let from_vec: Codes = vec![0, 1, u32::MAX].into();
        assert_eq!(&*from_vec, &[0, 1, u32::MAX]);
        assert!(from_vec.budget().is_none(), "From<Vec> must remain an infallible heap constructor");
    }

    #[test]
    fn cancellation_drops_partial_spill_and_does_not_publish_codes() {
        let dir = tempfile::tempdir().unwrap();
        let budget = CodeBudget::new(0, dir.path().into());
        let owner = format!("spill-cancel-{:?}", std::thread::current().id());
        let result: Result<Codes> = jobs::with_owner(Some(&owner), || jobs::run_result("Spill cancellation", || {
            let mut out = CodeBuilder::new(budget.clone());
            out.extend_repeated(7, GROW_CODES).unwrap();
            assert_eq!(budget.stats().spill_files, 1);
            let list = jobs::list_owned(&owner);
            let id = list.as_array().unwrap().iter().find(|j| j["state"] == "running").unwrap()["id"].as_u64().unwrap();
            assert!(jobs::cancel_owned(&owner, id));
            let error = out.push(8).unwrap_err();
            assert_eq!(error, jobs::CANCELLED);
            assert!(out.finish().is_err());
            Err(error)
        }));
        assert_eq!(result.unwrap_err(), jobs::CANCELLED);
        assert_eq!(budget.stats().spill_files, 0);
        assert_eq!(budget.stats().buffer_bytes, 0);
        assert_eq!(budget.stats().mapped_bytes, 0);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    #[test]
    fn partial_writes_are_reported_instead_of_silently_truncating_codes() {
        struct FailAfter { bytes: Vec<u8>, remaining: usize }
        impl Write for FailAfter {
            fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                if self.remaining == 0 { return Err(std::io::Error::other("synthetic disk full")); }
                let n = bytes.len().min(self.remaining);
                self.bytes.extend_from_slice(&bytes[..n]);
                self.remaining -= n;
                Ok(n)
            }
            fn flush(&mut self) -> std::io::Result<()> { Ok(()) }
        }
        let mut sink = FailAfter { bytes: vec![], remaining: 6 };
        assert!(write_codes(&mut sink, &[1, 2, 3]).unwrap_err().contains("synthetic disk full"));
        assert_eq!(sink.bytes.len(), 6);
    }

    #[test]
    #[ignore = "200 MB real spill scan; run explicitly under the benchmark process memory cap"]
    fn fifty_million_and_one_codes_are_written_without_a_whole_column_vec() {
        let dir = tempfile::tempdir().unwrap();
        let budget = CodeBudget::new(1024 * 1024, dir.path().into());
        let mut out = CodeBuilder::new(budget.clone());
        for _ in 0..50_000_001 / GROW_CODES { out.extend_repeated(17, GROW_CODES).unwrap(); }
        out.extend_repeated(17, 50_000_001 % GROW_CODES).unwrap();
        let codes = out.finish().unwrap();
        assert_eq!(codes.len(), 50_000_001);
        assert!(codes.is_mapped());
        assert_eq!(codes.mapped_bytes(), 200_000_004);
        assert_eq!(codes.iter().map(|&c| c as u64).sum::<u64>(), 50_000_001 * 17);
        assert!(budget.stats().heap_peak_bytes <= 1024 * 1024);
    }
}
