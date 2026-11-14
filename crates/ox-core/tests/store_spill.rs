use ox_core::{
    grel::Value,
    jobs,
    recon::{Recon, ReconCol},
    spill::CodeBudget,
    storage,
    store::{read_table, read_table_reader, write_table, History, Store},
    table::{Column, Table},
};
use serde_json::{json, Value as Json};
use std::{io::{self, Cursor, Read}, path::Path, sync::{atomic::{AtomicUsize, Ordering}, Arc}};

const CHUNK: usize = 64 * 1024;

fn wire(header: Json, payload: &[u8]) -> Vec<u8> {
    let header = serde_json::to_vec(&header).unwrap();
    let mut bytes = b"OXT1".to_vec();
    bytes.extend_from_slice(&(header.len() as u64).to_le_bytes());
    bytes.extend(header);
    bytes.extend_from_slice(payload);
    bytes
}

// An independently authored OXT1 fixture, including codes >255 to check little-endian decoding.
fn legacy(rows: usize) -> Vec<u8> {
    let header = json!({ "names": ["number", "label"], "rows": rows, "key": "number",
        "dicts": [(0..300).map(|i| json!(i)).collect::<Vec<_>>(), vec![json!("café"), Json::Null]] });
    let mut payload = Vec::new();
    for row in 0..rows { payload.extend_from_slice(&((row % 300) as u32).to_le_bytes()); }
    for row in 0..rows { payload.extend_from_slice(&((row % 2) as u32).to_le_bytes()); }
    payload.extend((0..rows).map(|r| u8::from(r % 65_536 == 65_535)));
    payload.extend((0..rows).map(|r| u8::from(r == 0 || r + 1 == rows)));
    wire(header, &payload)
}

fn budget(dir: &Path) -> CodeBudget { CodeBudget::new(0, dir.into()) }

fn clean(budget: &CodeBudget, dir: &Path) {
    let stats = budget.stats();
    assert_eq!(stats.heap_bytes, 0);
    assert_eq!(stats.mapped_bytes, 0);
    assert_eq!(stats.buffer_bytes, 0);
    assert_eq!(stats.spill_files, 0);
    assert_eq!(std::fs::read_dir(dir).unwrap().count(), 0);
}

fn same(left: &Table, right: &Table) {
    assert_eq!(left.names, right.names);
    assert_eq!(left.rows, right.rows);
    assert_eq!(left.key, right.key);
    assert_eq!(left.starred, right.starred);
    assert_eq!(left.flagged, right.flagged);
    for (a, b) in left.columns.iter().zip(&right.columns) {
        assert_eq!(a.dict, b.dict);
        assert_eq!(&*a.codes, &*b.codes);
        assert_eq!(a.recon, b.recon);
    }
}

struct BoundedReader { bytes: Cursor<Vec<u8>>, largest: Arc<AtomicUsize> }
impl Read for BoundedReader {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        assert!(out.len() <= CHUNK, "the reader must not request the whole code column/file");
        self.largest.fetch_max(out.len(), Ordering::Relaxed);
        // Short reads also have to work across the header, code and flag boundaries.
        let n = out.len().min(4093);
        self.bytes.read(&mut out[..n])
    }
}

#[test]
fn legacy_oxt_streams_little_endian_codes_and_chunked_flags_with_forced_spilling() {
    let temp = tempfile::tempdir().unwrap();
    let budget = budget(temp.path());
    let empty = storage::with_budget(budget.clone(), || read_table(&wire(
        json!({ "names": ["empty"], "rows": 0, "dicts": [[]] }), &[],
    ))).unwrap();
    assert_eq!(empty.rows, 0);
    assert!(empty.columns[0].codes.is_empty());
    assert!(!empty.columns[0].codes.is_mapped());
    drop(empty);
    let input = legacy(65_557);
    let largest = Arc::new(AtomicUsize::new(0));
    let table = storage::with_budget(budget.clone(), || read_table_reader(BoundedReader {
        bytes: Cursor::new(input), largest: largest.clone(),
    })).unwrap();
    assert_eq!(largest.load(Ordering::Relaxed), CHUNK);
    assert!(table.columns.iter().all(|c| c.codes.is_mapped()));
    assert_eq!(table.columns[0].get(299), &Value::Int(299));
    assert_eq!(table.columns[0].get(300), &Value::Int(0));
    assert_eq!(table.columns[1].get(0), &Value::Str("café".into()));
    assert_eq!(table.columns[1].get(1), &Value::Null);
    assert!(table.starred[65_535]);
    assert!(!table.starred[65_536]);
    assert!(table.flagged[0] && table.flagged[table.rows - 1]);
    assert_eq!(budget.stats().heap_peak_bytes, 0);
    assert_eq!(budget.stats().mapped_bytes, table.rows * 2 * 4);

    let output = temp.path().join("roundtrip.oxt");
    write_table(&output, &table).unwrap();
    let restored = storage::with_budget(budget.clone(), || read_table(&std::fs::read(&output).unwrap())).unwrap();
    same(&table, &restored);
    drop((table, restored));
    std::fs::remove_file(output).unwrap();
    clean(&budget, temp.path());
}

#[test]
fn store_clones_share_the_budget_and_mapped_tables_own_private_files() {
    let home = tempfile::tempdir().unwrap();
    let spill = tempfile::tempdir().unwrap();
    let budget = budget(spill.path());
    let table = read_table(&legacy(40_003)).unwrap();
    let mut table = table;
    let mut recon = Recon::bare(7);
    recon.service = "https://example.test/reconcile".into();
    let mut cells = vec![None; table.rows];
    cells[299] = Some(Arc::new(recon));
    Arc::get_mut(&mut table.columns[0]).unwrap().recon = Some(Arc::new(ReconCol {
        cells, config: Some(json!({ "mode": "standard-service", "service": "https://example.test/reconcile" })), stats: None,
    }));
    let store = Store::open_with_budget(home.path(), budget.clone()).unwrap();
    let meta = store.create("Spilled project", &table, None).unwrap();
    store.save(&meta, &History::default()).unwrap();
    let clone = store.clone();
    drop(store);
    let (_, restored, history) = clone.load(&meta.id).unwrap();
    assert_eq!(history.cursor, 0);
    assert!(restored.columns.iter().all(|c| c.codes.is_mapped()));
    same(&table, &restored);
    assert_eq!(clone.code_budget().stats().mapped_bytes, budget.stats().mapped_bytes);
    let snapshot = restored.clone();
    drop(restored);

    // Truncating/deleting the public project must never invalidate a loaded private mapping.
    let public = clone.root().join(&meta.id).join("data.oxt");
    std::fs::write(public, b"external truncation of the owned test fixture").unwrap();
    same(&table, &snapshot);
    clone.delete(&meta.id).unwrap();
    same(&table, &snapshot);
    drop(snapshot);
    clean(&budget, spill.path());
}

#[test]
fn corrupt_truncated_and_overflowing_oxt_never_leave_partial_spills() {
    let spill = tempfile::tempdir().unwrap();
    let budget = budget(spill.path());
    let input = legacy(40_003);
    let header_len = u64::from_le_bytes(input[4..12].try_into().unwrap()) as usize;
    let data = 12 + header_len;
    let mut bad_code = input.clone();
    let late_code = data + 40_003 * 4 + 39_999 * 4;
    bad_code[late_code..late_code + 4].copy_from_slice(&u32::MAX.to_le_bytes());
    let mut bad_flag = input.clone();
    bad_flag[data + 40_003 * 8 + 12] = 2;
    let mut trailing = input.clone();
    trailing.push(1);
    let mut bad_length = b"OXT1".to_vec();
    bad_length.extend_from_slice(&u64::MAX.to_le_bytes());
    let mut cases = vec![bad_code, bad_flag, trailing, bad_length,
        wire(json!({ "names": ["x"], "rows": 0, "key": null, "dicts": [] }), &[]),
        wire(json!({ "names": [], "rows": usize::MAX, "key": null, "dicts": [] }), &[])];
    cases.extend([0, 3, 11, data - 1, data + CHUNK + 7, input.len() - 1].map(|n| input[..n].to_vec()));
    for (i, bytes) in cases.into_iter().enumerate() {
        let result = storage::with_budget(budget.clone(), || read_table_reader(Cursor::new(bytes)));
        assert!(result.is_err(), "corrupt case {i} must fail");
        clean(&budget, spill.path());
    }
    struct FailedRead(Cursor<Vec<u8>>);
    impl Read for FailedRead {
        fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
            if self.0.position() >= (CHUNK * 2) as u64 { return Err(io::Error::other("owned fixture read failure")); }
            self.0.read(out)
        }
    }
    let result = storage::with_budget(budget.clone(), || read_table_reader(FailedRead(Cursor::new(input))));
    assert!(result.err().unwrap().contains("owned fixture read failure"));
    clean(&budget, spill.path());
    assert!(budget.stats().mapped_peak_bytes > 0, "late failures happened after a column had already mapped");
}

struct CancelReader<'a> { bytes: Cursor<Vec<u8>>, owner: &'a str, budget: CodeBudget, cancelled: bool }
impl Read for CancelReader<'_> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if !self.cancelled && self.bytes.position() >= (CHUNK * 2) as u64 {
            assert!(self.budget.stats().spill_files > 0, "cancel after a real partial spill");
            assert!(jobs::cancel_owned(self.owner, 0));
            self.cancelled = true;
        }
        self.bytes.read(out)
    }
}

#[test]
fn cancellation_mid_stream_cleans_spills_and_keeps_the_saved_source_unchanged() {
    let home = tempfile::tempdir().unwrap();
    let spill = tempfile::tempdir().unwrap();
    let budget = budget(spill.path());
    let input = legacy(70_001);
    let path = home.path().join("saved.oxt");
    std::fs::write(&path, &input).unwrap();
    let owner = format!("store-read-cancel-{:?}", std::thread::current().id());
    let result = jobs::with_owner(Some(&owner), || jobs::run_result("Reading stored table", || {
        storage::with_budget(budget.clone(), || read_table_reader(CancelReader {
            bytes: Cursor::new(input.clone()), owner: &owner, budget: budget.clone(), cancelled: false,
        }))
    }));
    assert_eq!(result.err().unwrap(), jobs::CANCELLED);
    assert_eq!(jobs::list_owned(&owner)[0]["state"], "cancelled");
    assert_eq!(std::fs::read(path).unwrap(), input);
    clean(&budget, spill.path());
}

#[test]
fn failed_spill_does_not_fall_back_to_heap_or_change_a_saved_project() {
    let home = tempfile::tempdir().unwrap();
    let unusable = home.path().join("not-a-directory");
    std::fs::write(&unusable, b"keep this owned fixture").unwrap();
    let budget = budget(&unusable);
    let store = Store::open_with_budget(home.path().join("library"), budget.clone()).unwrap();
    let table = read_table(&legacy(20_001)).unwrap();
    let meta = store.create("Saved", &table, None).unwrap();
    let dir = store.root().join(&meta.id);
    let original: Vec<_> = ["data.oxt", "history.json", "meta.json"].iter().map(|name|
        (dir.join(name), std::fs::read(dir.join(name)).unwrap())).collect();
    assert!(store.load(&meta.id).err().unwrap().contains("creating spilled"));
    for (path, bytes) in original { assert_eq!(std::fs::read(path).unwrap(), bytes); }
    assert_eq!(std::fs::read(unusable).unwrap(), b"keep this owned fixture");
    assert_eq!(budget.stats().heap_peak_bytes, 0);
    assert_eq!(budget.stats().spill_files, 0);
}

#[test]
fn cancelled_or_invalid_writes_preserve_the_existing_destination() {
    let home = tempfile::tempdir().unwrap();
    let path = home.path().join("saved.oxt");
    let mut table = Table::new(vec!["x".into()], vec![Column::from_values([Value::Int(7)])], 1);
    write_table(&path, &table).unwrap();
    let original = std::fs::read(&path).unwrap();
    let owner = format!("store-write-cancel-{:?}", std::thread::current().id());
    let result = jobs::with_owner(Some(&owner), || jobs::run_result("Saving stored table", || {
        jobs::cancel_owned(&owner, 0);
        write_table(&path, &table)
    }));
    assert_eq!(result.err().unwrap(), jobs::CANCELLED);
    table.flagged = Arc::new(vec![]);
    assert!(write_table(&path, &table).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), original);
    assert_eq!(std::fs::read_dir(home.path()).unwrap().count(), 1);

    table.flagged = Arc::new(vec![false]);
    let blocked = home.path().join("blocked.oxt");
    std::fs::create_dir(&blocked).unwrap();
    std::fs::write(blocked.join("keep"), b"owned fixture").unwrap();
    assert!(write_table(&blocked, &table).is_err(), "a rename failure must remove its temporary file");
    assert_eq!(std::fs::read(blocked.join("keep")).unwrap(), b"owned fixture");
    assert_eq!(std::fs::read(&path).unwrap(), original);
    assert_eq!(std::fs::read_dir(home.path()).unwrap().count(), 2);
}
