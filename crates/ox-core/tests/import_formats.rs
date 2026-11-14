use ox_core::{dispatch, import, store::Store, table::Table, Project, Session};
use serde_json::json;
use std::{path::PathBuf, time::{SystemTime, UNIX_EPOCH}};

struct Work(PathBuf);
impl Drop for Work { fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.0); } }

fn same_cells(expected: &Table, actual: &Table) {
    assert_eq!(actual.names, expected.names);
    assert_eq!(actual.rows, expected.rows);
    for (a, e) in actual.columns.iter().zip(&expected.columns) {
        for row in 0..expected.rows {
            assert_eq!(a.get(row), e.get(row));
            assert_eq!(a.recon(row).map(|r| r.to_json(true)), e.recon(row).map(|r| r.to_json(true)));
        }
    }
}

#[test]
fn preview_caps_respect_user_limits_and_format_specific_zero_semantics() {
    let work = Work(std::env::temp_dir().join(format!("ox-preview-limits-{}-{}", std::process::id(), SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos())));
    std::fs::create_dir(&work.0).unwrap();
    for (file, format, text) in [
        ("data.csv", "csv", "name\nA\nB\nC\n"),
        ("data.px", "pcaxis", "STUB=\"Name\";\nVALUES(\"Name\")=\"A\",\"B\",\"C\";\nDATA=1 2 3;\n"),
        ("data.wiki", "wikitext", "{|\n! Name\n|-\n| A\n|-\n| B\n|-\n| C\n|}\n"),
    ] {
        let path = work.0.join(file);
        std::fs::write(&path, text).unwrap();
        let mut session = Session::default();
        for (requested, cap, expected) in [(1, 50, 1), (2, 1, 1), (-1, 2, 2), (0, 2, if format == "csv" { 0 } else { 2 })] {
            let preview = dispatch(&mut session, "preview_import", json!({ "path": path, "format": format, "options": { "limit": requested }, "limit": cap })).unwrap();
            assert_eq!(preview["data"]["rows"].as_array().unwrap().len(), expected, "{format}: requested {requested}, preview cap {cap}");
        }
        let preview = dispatch(&mut session, "preview_import", json!({ "path": path, "options": { "limit": 0 }, "limit": 2 })).unwrap();
        assert_eq!(preview["data"]["rows"].as_array().unwrap().len(), if format == "csv" { 0 } else { 2 });
    }
}

#[test]
fn new_import_formats_preview_without_saving_and_reopen_with_source_options() {
    let work = Work(std::env::temp_dir().join(format!("ox-new-formats-{}-{}", std::process::id(), SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos())));
    std::fs::create_dir(&work.0).unwrap();
    let wiki = work.0.join("tables.wiki");
    std::fs::write(&wiki, "{|\n! Name !! Value\n|-\n| Alpha || 42\n|}\n\n{|\n! Other\n|-\n| Second\n|}\n").unwrap();
    let px = work.0.join("cube.PX");
    std::fs::write(&px, "STUB=\"Area\";\nVALUES(\"Area\")=\"A\",\"B\";\nDATA=001 ..;\n").unwrap();
    let marc = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/marc/ascii.mrc");
    let store = Store::open(&work.0.join("store")).unwrap();
    for (path, format) in [(marc, "marc"), (wiki, "wikitext"), (px, "pcaxis")] {
        let inspected = import::inspect(&path).unwrap();
        assert_eq!(inspected["format"], format);
        let mut options = inspected["options"].clone();
        if format == "wikitext" { options["tableIndexes"] = json!([1]); }
        let args = json!({ "path": path, "format": format, "options": options, "name": format });
        let mut session = Session::with_store(store.clone());
        let before = store.list().len();
        dispatch(&mut session, "preview_import", args.clone()).unwrap();
        assert!(session.project.is_none());
        assert_eq!(store.list().len(), before, "a preview must not save a project");
        dispatch(&mut session, "open", args).unwrap();
        let original = session.project.as_ref().unwrap();
        assert!(original.table().rows > 0);
        assert_eq!(original.recipe().len(), 0);
        let id = original.meta.as_ref().unwrap().id.clone();
        let (meta, table, history) = Store::open(&work.0.join("store")).unwrap().load(&id).unwrap();
        let source = meta.source.as_ref().unwrap();
        assert_eq!(source.format.as_deref(), Some(format));
        if format == "wikitext" { assert_eq!(source.options.table_indexes, [1]); }
        let reopened = Project::restore(meta, table, history).unwrap();
        same_cells(original.table(), reopened.table());
        let archive = work.0.join(format!("{format}.openrefine.tar.gz"));
        ox_core::orproject::write(original, &archive).unwrap();
        let imported = ox_core::orproject::read(&archive).unwrap().into_project().0;
        same_cells(original.table(), imported.table());
    }
}
