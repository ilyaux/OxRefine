use std::{path::{Path, PathBuf}, process::Command};

fn files(label: &str) -> (PathBuf, PathBuf, PathBuf) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let dir = root.join(format!("target/cli-extension-{label}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    (dir.join("recipe.json"), root.join("tests/orproject/extensions/wikibase.openrefine.tar.gz"), dir.join("new.openrefine.tar.gz"))
}

#[test]
fn new_archive_output_is_an_archive_and_keeps_extension_data() {
    let (recipe, input, output) = files("copy");
    std::fs::write(&recipe, "[]").unwrap();
    let run = Command::new(env!("CARGO_BIN_EXE_ox")).arg("apply").arg(recipe).arg(&input).arg("-o").arg(&output).output().unwrap();
    assert!(run.status.success(), "{}", String::from_utf8_lossy(&run.stderr));
    assert_eq!(std::fs::read(output).unwrap(), std::fs::read(input).unwrap());
}

#[test]
fn incompatible_edits_leave_existing_output_untouched() {
    let (recipe, input, output) = files("refused");
    std::fs::write(&recipe, r#"[{"op":"core/column-rename","oldColumnName":"name","newColumnName":"renamed"}]"#).unwrap();
    std::fs::write(&output, b"keep output").unwrap();
    let run = Command::new(env!("CARGO_BIN_EXE_ox")).arg("apply").arg(recipe).arg(input).arg("-o").arg(&output).output().unwrap();
    assert!(!run.status.success());
    assert!(String::from_utf8_lossy(&run.stderr).contains("additional OpenRefine data"));
    assert_eq!(std::fs::read(output).unwrap(), b"keep output");
}
