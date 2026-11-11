//! The files a path stands for when importing: the file itself, the files inside a zip, tar or
//! gzip archive, or the files in a folder (OpenRefine's archive handling and multi-file uploads).
use crate::{err, Result};
use std::{
    io::Read,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

pub struct Unpacked {
    /// (name for the File column, file to read), in archive or folder order
    pub files: Vec<(String, PathBuf)>,
    /// the archive's file name, for the Archive column (None for a plain file, gzip or folder)
    pub archive: Option<String>,
    tmp: Option<PathBuf>,
}

impl Unpacked {
    /// More than one file to choose from (an archive or a folder).
    pub fn several(&self) -> bool {
        self.files.len() > 1 || self.archive.is_some()
    }
}

impl Drop for Unpacked {
    fn drop(&mut self) {
        if let Some(t) = &self.tmp {
            let _ = std::fs::remove_dir_all(t);
        }
    }
}

fn name_of(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

/// Folders and files inside archives that are never data: macOS resource forks, dotfiles.
fn skipped(name: &str) -> bool {
    name.starts_with("__MACOSX/") || name.rsplit('/').next().is_some_and(|b| b.is_empty() || b.starts_with('.'))
}

fn temp_dir() -> Result<PathBuf> {
    static N: AtomicU64 = AtomicU64::new(0);
    let d = std::env::temp_dir().join(format!("oxrefine-unpack-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
    std::fs::create_dir_all(&d).map_err(|e| format!("{}: {e}", d.display()))?;
    Ok(d)
}

/// Each entry is written under its own numbered folder, by its base name only (no path escapes).
fn extract(tmp: &Path, i: usize, name: &str, mut data: impl Read) -> Result<PathBuf> {
    let dir = tmp.join(i.to_string());
    std::fs::create_dir_all(&dir).map_err(err)?;
    let path = dir.join(name.rsplit('/').next().unwrap_or(name));
    let mut f = std::fs::File::create(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    std::io::copy(&mut data, &mut f).map_err(|e| format!("{name}: {e}"))?;
    Ok(path)
}

fn is_tar(head: &[u8]) -> bool {
    head.get(257..262) == Some(b"ustar")
}

fn untar(tmp: &Path, data: impl Read) -> Result<Vec<(String, PathBuf)>> {
    let mut out = vec![];
    for (i, e) in tar::Archive::new(data).entries().map_err(err)?.enumerate() {
        let e = e.map_err(|e| format!("damaged archive: {e}"))?;
        let name = e.path().map_err(err)?.to_string_lossy().replace('\\', "/").trim_start_matches("./").to_string();
        if e.header().entry_type().is_file() && !skipped(&name) {
            let path = extract(tmp, i, &name, e)?;
            out.push((name, path));
        }
    }
    Ok(out)
}

pub fn unpack(path: &Path) -> Result<Unpacked> {
    if path.is_dir() {
        let mut files: Vec<(String, PathBuf)> = std::fs::read_dir(path)
            .map_err(|e| format!("{}: {e}", path.display()))?
            .flatten()
            .filter(|e| e.path().is_file())
            .map(|e| (e.file_name().to_string_lossy().into_owned(), e.path()))
            .filter(|(n, _)| !skipped(n))
            .collect();
        files.sort();
        return Ok(Unpacked { files, archive: None, tmp: None });
    }
    let mut head = vec![0u8; 512];
    let n = std::fs::File::open(path).and_then(|mut f| f.read(&mut head)).map_err(|e| format!("{}: {e}", path.display()))?;
    head.truncate(n);
    let name = name_of(path);
    let open = || std::fs::File::open(path).map(std::io::BufReader::new).map_err(|e| format!("{}: {e}", path.display()));
    // spreadsheets and other documents that are zip files inside are files, not archives
    let document = path.extension().is_some_and(|e| ["xlsx", "xlsm", "xlsb", "ods", "odt", "docx"].iter().any(|x| e.eq_ignore_ascii_case(x)));
    if head.starts_with(b"PK\x03\x04") && !document {
        let tmp = temp_dir()?;
        let mut z = zip::ZipArchive::new(open()?).map_err(|e| format!("{name}: {e}"))?;
        let mut files = vec![];
        for i in 0..z.len() {
            let e = z.by_index(i).map_err(|e| format!("{name}: {e}"))?;
            let inner = e.name().map_err(|e| format!("{name}: {e}"))?.to_string();
            if e.is_file() && !skipped(&inner) {
                files.push((inner.clone(), extract(&tmp, i, &inner, e)?));
            }
        }
        return Ok(Unpacked { files, archive: Some(name), tmp: Some(tmp) });
    }
    if head.starts_with(&[0x1f, 0x8b]) {
        let tmp = temp_dir()?;
        let mut inner_head = vec![0u8; 512];
        let k = flate2::read::GzDecoder::new(open()?).read(&mut inner_head).unwrap_or(0);
        inner_head.truncate(k);
        if is_tar(&inner_head) {
            let files = untar(&tmp, flate2::read::GzDecoder::new(open()?))?;
            return Ok(Unpacked { files, archive: Some(name), tmp: Some(tmp) });
        }
        // one compressed file: OpenRefine names it after the .gz file itself
        let inner = name.strip_suffix(".gz").or_else(|| name.strip_suffix(".GZ")).unwrap_or(&name).to_string();
        let file = extract(&tmp, 0, &inner, flate2::read::MultiGzDecoder::new(open()?))?;
        return Ok(Unpacked { files: vec![(name, file)], archive: None, tmp: Some(tmp) });
    }
    if is_tar(&head) {
        let tmp = temp_dir()?;
        let files = untar(&tmp, open()?)?;
        return Ok(Unpacked { files, archive: Some(name), tmp: Some(tmp) });
    }
    Ok(Unpacked { files: vec![(name, path.to_path_buf())], archive: None, tmp: None })
}
