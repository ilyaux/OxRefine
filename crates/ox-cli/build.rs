// Builds the web UI (ui/dist, from `npm run build` in ui/) into the `ox` binary, so `ox serve` needs
// nothing next to it. Without a built UI the server still runs and says how to build it.
use std::{fs, path::Path};

fn walk(dir: &Path, root: &Path, out: &mut Vec<(String, String)>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            walk(&p, root, out);
        } else {
            let url = format!("/{}", p.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/"));
            out.push((url, p.canonicalize().unwrap().to_string_lossy().into_owned()));
        }
    }
}

fn main() {
    let dist = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../ui/dist");
    println!("cargo:rerun-if-changed={}", dist.display());
    let mut files = vec![];
    walk(&dist, &dist, &mut files);
    files.sort();
    let mut code = String::from("pub static ASSETS: &[(&str, &[u8])] = &[\n");
    for (url, path) in &files {
        code += &format!("    ({url:?}, include_bytes!({path:?})),\n");
    }
    code += "];\n";
    fs::write(Path::new(&std::env::var("OUT_DIR").unwrap()).join("assets.rs"), code).unwrap();
}
