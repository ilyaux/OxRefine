use ox_core::{dispatch, store::Store, Session};
use serde_json::{json, Value};
use std::{path::{Path, PathBuf}, sync::atomic::{AtomicUsize, Ordering}};

fn home() -> PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let path = std::env::temp_dir().join(format!("oxrefine-stats-{}-{}-{}", std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos(), NEXT.fetch_add(1, Ordering::Relaxed)));
    std::fs::create_dir_all(&path).unwrap();
    path
}

fn call(s: &mut Session, cmd: &str, args: Value) -> Value { dispatch(s, cmd, args).unwrap()["data"].clone() }
fn listed(store: &Store) -> Value { call(&mut Session::with_store(store.clone()), "projects", json!({}))[0].clone() }
fn project(home: &Path) -> (Store, Session, PathBuf) {
    let csv = home.join("people.csv");
    std::fs::write(&csv, "name,n\n alice ,1\nbob,2\n").unwrap();
    let store = Store::open(home).unwrap();
    let mut session = Session::with_store(store.clone());
    let id = call(&mut session, "open", json!({"path": csv, "name":"People"}))["id"].as_str().unwrap().to_owned();
    let dir = store.root().join(id);
    (store, session, dir)
}

#[test]
fn counts_redo_and_regular_file_bytes_without_changing_metadata() {
    let home = home();
    let (store, mut session, dir) = project(&home);
    for expression in ["value.trim()", "value.toUppercase()"] {
        call(&mut session, "apply", json!({"op":{"op":"core/text-transform","columnName":"name","expression":expression}}));
    }
    call(&mut session, "goto", json!({"index":1}));
    let meta = std::fs::read(dir.join("meta.json")).unwrap();
    let history = std::fs::read(dir.join("history.json")).unwrap();
    assert_eq!(serde_json::from_slice::<Value>(&history).unwrap()["cursor"], 1);
    std::fs::create_dir(dir.join("extra")).unwrap();
    std::fs::write(dir.join("extra/state.bin"), [0; 123]).unwrap();
    let bytes: u64 = ["data.oxt", "meta.json", "history.json", "extra/state.bin"].iter()
        .map(|p| std::fs::metadata(dir.join(p)).unwrap().len()).sum();
    for store in [store, Store::open(&home).unwrap()] {
        let item = listed(&store);
        assert_eq!(item["stats"], json!({"historySteps":2,"sizeBytes":bytes}), "cold and saved counts include redo");
        assert_eq!(item["name"], "People");
        assert_eq!(store.list().len(), 1, "the ordinary metadata listing remains compatible");
    }
    assert_eq!(std::fs::read(dir.join("meta.json")).unwrap(), meta);
    assert_eq!(std::fs::read(dir.join("history.json")).unwrap(), history);
    assert!(serde_json::from_slice::<Value>(&meta).unwrap().get("stats").is_none());
}

#[test]
fn optional_project_pagination_preserves_legacy_listing_and_saved_metadata() {
    let home = home();
    let mut originals = vec![];
    for index in 0..3 {
        let (_, _, dir) = project(&home);
        let path = dir.join("meta.json");
        let mut meta: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        meta["modified"] = json!(format!("2026-11-{:02}T10:00:00Z", index + 1));
        let bytes = serde_json::to_vec(&meta).unwrap();
        std::fs::write(&path, &bytes).unwrap();
        originals.push((path, bytes));
    }
    let store = Store::open(&home).unwrap();
    let mut session = Session::with_store(store);
    let all = call(&mut session, "projects", json!({}));
    assert_eq!(all.as_array().unwrap().len(), 3);
    let page = |session: &mut Session, start, limit| call(session, "projects", json!({"start":start,"limit":limit,"params":{"paginate":true}}));
    assert_eq!(page(&mut session, 0, 2), json!({"total":3,"projects":[all[0],all[1]]}));
    assert_eq!(page(&mut session, 2, 2), json!({"total":3,"projects":[all[2]]}));
    assert_eq!(page(&mut session, 3, 2), json!({"total":3,"projects":[]}));
    assert_eq!(page(&mut session, 0, 0)["projects"], all, "default page size still returns this small library");
    assert_eq!(page(&mut session, 0, 1000)["projects"], all, "large page requests are safely clamped");
    assert_eq!(call(&mut session, "projects", json!({"start":1,"limit":1})), all, "legacy unflagged listing stays an array");
    for (path, bytes) in originals { assert_eq!(std::fs::read(path).unwrap(), bytes); }
    assert_eq!(call(&mut Session::default(), "projects", json!({"params":{"paginate":true}})), json!({"total":0,"projects":[]}));
}

#[test]
fn recipe_file_reply_is_small_and_legacy_array_reply_stays_compatible() {
    let home = home();
    let (_, mut session, _) = project(&home);
    call(&mut session, "apply", json!({"op":{"op":"core/text-transform","columnName":"name","expression":"value.trim()"}}));
    let legacy_path = home.join("legacy.json");
    let recipe = call(&mut session, "export_recipe", json!({"path":legacy_path}));
    assert_eq!(recipe.as_array().unwrap().len(), 1);
    assert_eq!(serde_json::from_slice::<Value>(&std::fs::read(legacy_path).unwrap()).unwrap(), recipe);
    let file_path = home.join("file.json");
    assert_eq!(call(&mut session, "export_recipe", json!({"path":file_path,"params":{"fileReply":true}})), json!(file_path));
    assert_eq!(serde_json::from_slice::<Value>(&std::fs::read(file_path).unwrap()).unwrap(), recipe);
    assert_eq!(call(&mut session, "export_recipe", json!({"params":{"fileReply":true}})), recipe, "without a file, retain the array reply");
}

#[test]
fn preserved_original_archive_is_included_in_size() {
    let home = home();
    let store = Store::open(&home).unwrap();
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/orproject/extensions/wikibase.openrefine.tar.gz");
    let mut session = Session::with_store(store.clone());
    let imported = call(&mut session, "import_project", json!({"path":fixture}));
    let dir = store.root().join(imported["id"].as_str().unwrap());
    let original = std::fs::read(dir.join("original.openrefine.tar.gz")).unwrap();
    assert_eq!(original, std::fs::read(fixture).unwrap());
    let bytes: u64 = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().metadata().unwrap().len()).sum();
    assert_eq!(listed(&Store::open(&home).unwrap())["stats"]["sizeBytes"], bytes);
    assert_eq!(std::fs::read(dir.join("original.openrefine.tar.gz")).unwrap(), original);
}

#[test]
fn old_missing_corrupt_and_oversized_history_have_honest_counts() {
    let home = home();
    let (store, _, dir) = project(&home);
    assert_eq!(listed(&store)["stats"]["historySteps"], 0, "an empty saved history is known zero");
    let history = dir.join("history.json");
    std::fs::remove_file(&history).unwrap();
    assert!(listed(&store)["stats"]["historySteps"].is_null(), "missing is not zero, even after a cached save");
    for bytes in [b"{".as_slice(), br#"{"ops":[{}],"cursor":0}"#, br#"{"ops":[],"cursor":1}"#] {
        std::fs::write(&history, bytes).unwrap();
        let stats = listed(&store)["stats"].clone();
        assert!(stats["historySteps"].is_null(), "corrupt or inconsistent history is not zero");
        assert!(stats["sizeBytes"].as_u64().unwrap() > 0);
    }
    // An unknown legacy operation still has one saved step; a large payload need not become an Op.
    let legacy = json!({"ops":[{"op":"vendor/legacy","payload":"x".repeat(1024*1024)}],"cursor":0});
    std::fs::write(&history, serde_json::to_vec(&legacy).unwrap()).unwrap();
    assert_eq!(listed(&store)["stats"]["historySteps"], 1, "file replacement invalidates the cached count");
    std::fs::File::options().write(true).open(&history).unwrap().set_len(64*1024*1024+1).unwrap();
    assert!(listed(&store)["stats"]["historySteps"].is_null(), "cold scans are bounded");
    assert!(listed(&store)["stats"]["sizeBytes"].as_u64().unwrap() > 64*1024*1024);
}

#[test]
fn linked_directories_are_never_counted_or_listed() {
    let home = home();
    let (store, _, dir) = project(&home);
    let outside = home.join("outside");
    std::fs::create_dir(&outside).unwrap();
    std::fs::write(outside.join("keep.bin"), [42; 100]).unwrap();
    let link = dir.join("external");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&outside, &link).unwrap();
    #[cfg(windows)]
    {
        // Junctions require no developer mode or symlink privilege on Windows.
        let result = std::process::Command::new("cmd").args(["/C", "mklink", "/J"])
            .arg(&link).arg(&outside).output().unwrap();
        assert!(result.status.success(), "could not create test junction: {}", String::from_utf8_lossy(&result.stderr));
    }
    #[cfg(any(unix, windows))]
    {
        assert!(listed(&store)["stats"]["sizeBytes"].is_null(), "a partial sum must not masquerade as total size");
        assert_eq!(listed(&store)["stats"]["historySteps"], 0);
        let alias = store.root().join("999-999");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&dir, &alias).unwrap();
        #[cfg(windows)]
        assert!(std::process::Command::new("cmd").args(["/C", "mklink", "/J"]).arg(&alias).arg(&dir).output().unwrap().status.success());
        assert_eq!(store.list_with_stats().len(), 1, "project directory links are skipped");
        assert_eq!(std::fs::read(outside.join("keep.bin")).unwrap(), [42; 100]);
    }
}
