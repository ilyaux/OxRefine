use ox_core::{dispatch, ops::Op, orproject, store::Store, Project, Session};
use serde_json::{json, Value};
use std::{io::{Cursor, Read, Write}, path::{Path, PathBuf}};

fn root() -> PathBuf { Path::new(env!("CARGO_MANIFEST_DIR")).join("../..") }
fn fixture() -> PathBuf { root().join("tests/orproject/extensions/wikibase.openrefine.tar.gz") }
fn output(name: &str) -> PathBuf {
    let dir = root().join("target/extension-export");
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name)
}
fn op(v: Value) -> Op { serde_json::from_value(v).unwrap() }

#[test]
fn wikibase_schema_and_unsupported_history_are_preserved() {
    let bytes = std::fs::read(fixture()).unwrap();
    let expected: Value = serde_json::from_slice(&std::fs::read(root().join("tests/orproject/extensions/fixtures.json")).unwrap()).unwrap();
    let source = orproject::read(&fixture()).unwrap();
    assert_eq!(json!(source.history), expected["history"]);
    let (p, report) = source.into_project();
    assert!(report.merged > 0, "the unsupported Wikibase history must be reported");
    assert!(!report.warnings.is_empty());
    assert!(p.archive_warning().is_some());
    let path = output("wikibase.openrefine.tar.gz");
    let sibling = path.with_extension("tmp");
    std::fs::write(&sibling, b"keep sibling file").unwrap();
    orproject::write(&p, &path).unwrap();
    assert_eq!(std::fs::read(&sibling).unwrap(), b"keep sibling file");
    assert_eq!(std::fs::read(&path).unwrap(), bytes, "the whole archive, not only its table, must survive");
    assert_eq!(json!(orproject::read(&path).unwrap().history), expected["history"]);
}

// Unknown overlays and files are synthetic additions to a real OpenRefine archive: preserve them
// without claiming to understand or execute the extension that owns them.
fn additional_data(kind: &str) -> PathBuf {
    let source = std::fs::read(root().join("tests/metadata/project.openrefine.tar.gz")).unwrap();
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(Cursor::new(source)));
    let mut out = tar::Builder::new(flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default()));
    for entry in archive.entries().unwrap() {
        let mut entry = entry.unwrap();
        let name = entry.path().unwrap().into_owned();
        let mut bytes = Vec::new(); entry.read_to_end(&mut bytes).unwrap();
        if name == Path::new("data.zip") && matches!(kind, "overlay" | "column" | "zip") {
            let mut z = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
            let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
            for i in 0..z.len() {
                let mut file = z.by_index(i).unwrap();
                let name = file.name().unwrap().to_string();
                let mut contents = Vec::new(); file.read_to_end(&mut contents).unwrap();
                if name == "data.txt" && kind != "zip" {
                    let text = String::from_utf8(contents).unwrap();
                    let changed = if kind == "overlay" {
                        text.replacen("rowCount=", "overlayModel:example={\"column\":\"a\",\"rows\":[0]}\nrowCount=", 1)
                    } else { text.replacen("\"name\":\"a\"", "\"name\":\"a\",\"example:state\":{\"x\":1}", 1) };
                    assert_ne!(changed, text);
                    contents = changed.into_bytes();
                }
                writer.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
                writer.write_all(&contents).unwrap();
            }
            if kind == "zip" {
                writer.start_file("vendor/state.bin", zip::write::SimpleFileOptions::default()).unwrap();
                writer.write_all(&[0, 255, 42]).unwrap();
            }
            bytes = writer.finish().unwrap().into_inner();
        }
        if name == Path::new("metadata.json") && kind == "metadata" {
            let mut meta: Value = serde_json::from_slice(&bytes).unwrap();
            meta["example:state"] = json!({"column":"a"});
            bytes = serde_json::to_vec(&meta).unwrap();
        }
        let mut header = entry.header().clone(); header.set_size(bytes.len() as u64); header.set_cksum();
        out.append_data(&mut header, &name, Cursor::new(bytes)).unwrap();
    }
    if kind == "file" {
        let mut header = tar::Header::new_gnu(); header.set_size(3); header.set_mode(0o644); header.set_cksum();
        out.append_data(&mut header, "extensions/example/state.bin", Cursor::new([0, 255, 42])).unwrap();
    }
    let path = output(&format!("unknown-{kind}.openrefine.tar.gz"));
    std::fs::write(&path, out.into_inner().unwrap().finish().unwrap()).unwrap();
    path
}

#[test]
fn unknown_overlays_files_and_metadata_are_preserved() {
    assert!(Project::open(&root().join("tests/metadata/project.openrefine.tar.gz")).unwrap().archive_warning().is_none());
    for kind in ["overlay", "column", "zip", "metadata", "file"] {
        let source = additional_data(kind);
        let p = Project::open(&source).unwrap();
        assert!(p.archive_warning().is_some(), "not detected: {kind}");
        let path = output(&format!("unknown-{kind}-back.openrefine.tar.gz"));
        orproject::write(&p, &path).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), std::fs::read(&source).unwrap(), "lost {kind}");
    }
}

#[test]
fn edits_metadata_and_undo_cannot_export_stale_extension_data() {
    for kind in ["rename", "metadata", "name", "noop", "undo"] {
        let mut p = Project::open(&fixture()).unwrap();
        match kind {
            "rename" => p.apply(op(json!({"op":"core/column-rename","oldColumnName":"name","newColumnName":"renamed"}))).unwrap(),
            "metadata" => p.info.description = "changed".into(),
            "name" => p.name = "changed".into(),
            "noop" => p.apply(op(json!({"op":"core/text-transform","columnName":"name","expression":"value"}))).unwrap(),
            _ => p.goto(0).unwrap(),
        }
        let path = output(&format!("refused-{kind}.openrefine.tar.gz"));
        std::fs::write(&path, b"keep destination").unwrap();
        assert!(orproject::write(&p, &path).unwrap_err().contains("additional OpenRefine data"), "not refused: {kind}");
        assert_eq!(std::fs::read(&path).unwrap(), b"keep destination");
        if kind == "undo" { p.goto(1).unwrap(); orproject::write(&p, &path).unwrap(); }
        else if kind == "rename" {
            p.goto(1).unwrap();
            assert!(orproject::write(&p, &path).is_err(), "undone new steps must not be silently dropped");
        }
        orproject::write_original(&p, &output(&format!("original-{kind}.openrefine.tar.gz"))).unwrap();
        assert_eq!(std::fs::read(output(&format!("original-{kind}.openrefine.tar.gz"))).unwrap(), std::fs::read(fixture()).unwrap());
    }
}

#[test]
fn restart_keeps_original_and_rejects_missing_or_corrupt_payload() {
    let home = output(&format!("store-{}", std::process::id()));
    let store = Store::open(&home).unwrap();
    let input = home.join("input.openrefine.tar.gz");
    let original = std::fs::read(fixture()).unwrap();
    std::fs::write(&input, &original).unwrap();
    let mut s = Session::with_store(store.clone());
    let imported = dispatch(&mut s, "import_project", json!({"path":input})).unwrap();
    assert!(imported["data"]["archiveWarning"].is_string());
    let id = imported["data"]["id"].as_str().unwrap().to_string();
    std::fs::remove_file(&input).unwrap();
    drop(s);
    let mut s = Session::with_store(store.clone());
    let reopened = dispatch(&mut s, "open_project", json!({"id":id})).unwrap();
    assert!(reopened["data"]["archiveWarning"].is_string());
    let back = output("reopened.openrefine.tar.gz");
    dispatch(&mut s, "export_project", json!({"path":back})).unwrap();
    assert_eq!(std::fs::read(&back).unwrap(), original);
    dispatch(&mut s, "apply", json!({"op":{"op":"core/column-rename","oldColumnName":"name","newColumnName":"renamed"}})).unwrap();
    drop(s);
    let mut s = Session::with_store(store.clone());
    dispatch(&mut s, "open_project", json!({"id":id})).unwrap();
    assert!(dispatch(&mut s, "export_project", json!({"path":back})).is_err());
    dispatch(&mut s, "export_original_project", json!({"path":back})).unwrap();
    assert_eq!(std::fs::read(&back).unwrap(), original);
    let dir = store.root().join(&id);
    let manifest: Value = serde_json::from_slice(&std::fs::read(dir.join("history.json")).unwrap()).unwrap();
    assert!(manifest["original"]["bytes"].is_null(), "source bytes belong in the binary sidecar");
    let sidecar = dir.join("original.openrefine.tar.gz");
    std::fs::write(&sidecar, b"damaged").unwrap();
    assert!(dispatch(&mut Session::with_store(store.clone()), "open_project", json!({"id":id})).unwrap_err().contains("damaged"));
    std::fs::remove_file(&sidecar).unwrap();
    assert!(dispatch(&mut Session::with_store(store.clone()), "open_project", json!({"id":id})).unwrap_err().contains("original.openrefine.tar.gz"));
    std::fs::write(&sidecar, &original).unwrap();
    let mut missing_manifest = manifest; missing_manifest.as_object_mut().unwrap().remove("original");
    std::fs::write(dir.join("history.json"), serde_json::to_vec(&missing_manifest).unwrap()).unwrap();
    assert!(store.load(&id).err().unwrap().contains("no history manifest"));
}

#[test]
fn ordinary_projects_still_allow_editable_archive_export() {
    let mut p = Project::open(&root().join("tests/metadata/project.openrefine.tar.gz")).unwrap();
    assert!(p.archive_warning().is_none());
    p.apply(op(json!({"op":"core/column-rename","oldColumnName":"a","newColumnName":"renamed"}))).unwrap();
    let path = output("ordinary-edited.openrefine.tar.gz");
    orproject::write(&p, &path).unwrap();
    assert_eq!(Project::open(&path).unwrap().table().names[0], "renamed");
}

#[test]
fn regenerated_reconciliation_ids_survive_restart_without_false_export_refusal() {
    let fixtures: Value = serde_json::from_slice(&std::fs::read(root().join("tests/orproject/fixtures.json")).unwrap()).unwrap();
    let case = fixtures["cases"].as_array().unwrap().iter().find(|c| c["name"] == "recon: extend: entity and strings").unwrap();
    let path = root().join("tests/orproject/archives").join(case["archive"].as_str().unwrap());
    let store = Store::open(output(&format!("recon-store-{}", std::process::id()))).unwrap();
    let mut s = Session::with_store(store.clone());
    let imported = dispatch(&mut s, "import_project", json!({"path":path})).unwrap();
    let id = imported["data"]["id"].as_str().unwrap().to_string();
    assert!(imported["data"]["archiveWarning"].is_string());
    drop(s);
    let mut s = Session::with_store(store);
    dispatch(&mut s, "open_project", json!({"id":id})).unwrap();
    let back = output("recon-reopened.openrefine.tar.gz");
    dispatch(&mut s, "export_project", json!({"path":back})).unwrap();
    assert_eq!(std::fs::read(back).unwrap(), std::fs::read(path).unwrap());
}
