//! A bridge to a real OpenRefine installation, for what OxRefine can't run itself: OpenRefine's Java
//! extensions. The project goes there as an OpenRefine project archive (with its history), the steps
//! run in a headless OpenRefine with a scratch workspace, and the result comes back as an archive
//! with OpenRefine's history. Nothing is installed or downloaded: it uses the folder the user names.
use crate::{err, Project, Result};
use serde_json::Value as J;
use std::{
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

/// The OpenRefine folder (with server/ and webapp/), also inside a macOS OpenRefine.app.
pub fn home(folder: &Path) -> Result<PathBuf> {
    let candidates = [folder.to_path_buf(), folder.join("Contents/Resources")];
    candidates.into_iter().find(|f| f.join("server").is_dir() && f.join("webapp").is_dir())
        .ok_or_else(|| format!("{} is not an OpenRefine folder (it has no server and webapp folders)", folder.display()))
}

/// Its bundled Java (Windows and macOS builds), else JAVA_HOME's, else `java` on the PATH.
fn java(home: &Path) -> PathBuf {
    let exe = if cfg!(windows) { "java.exe" } else { "java" };
    [home.join("server/target/jre/bin").join(exe), home.join("jre/bin").join(exe)].into_iter().find(|p| p.is_file())
        .or_else(|| std::env::var_os("JAVA_HOME").map(|j| PathBuf::from(j).join("bin").join(exe)).filter(|p| p.is_file()))
        .unwrap_or_else(|| PathBuf::from(exe))
}

/// A headless OpenRefine on a free local port with its own scratch workspace, stopped on drop.
pub struct OpenRefine { child: Child, url: String, agent: ureq::Agent, _workspace: tempfile::TempDir }

impl Drop for OpenRefine {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl OpenRefine {
    pub fn start(folder: &Path) -> Result<OpenRefine> {
        let home = home(folder)?;
        let workspace = tempfile::tempdir().map_err(err)?;
        let port = std::net::TcpListener::bind("127.0.0.1:0").and_then(|l| l.local_addr()).map_err(err)?.port();
        let separator = if cfg!(windows) { ";" } else { ":" };
        let classpath = format!("{}{separator}{}", home.join("server/classes").display(), home.join("server/target/lib/*").display());
        let child = Command::new(java(&home))
            .args(["-cp", &classpath, "-Xms256M", "-Xmx1400M", "-Drefine.memory=1400M", "-Drefine.max_form_content_size=1048576000"])
            .arg(format!("-Drefine.port={port}"))
            .args(["-Drefine.interface=127.0.0.1", "-Drefine.host=127.0.0.1", "-Drefine.headless=true", "-Drefine.autosave=1440"])
            .arg(format!("-Drefine.webapp={}", home.join("webapp").display()))
            .arg(format!("-Drefine.data_dir={}", workspace.path().display()))
            .arg("com.google.refine.Refine")
            .current_dir(&home).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null())
            .spawn().map_err(|e| format!("could not start OpenRefine's Java ({}): {e}", java(&home).display()))?;
        let agent = ureq::Agent::new_with_config(ureq::Agent::config_builder().http_status_as_error(false).max_redirects(0).timeout_global(Some(Duration::from_secs(3600))).build());
        let mut or = OpenRefine { child, url: format!("http://127.0.0.1:{port}"), agent, _workspace: workspace };
        let until = Instant::now() + Duration::from_secs(180);
        loop {
            crate::jobs::tick(0)?;
            if let Ok(Some(status)) = or.child.try_wait() { return Err(format!("OpenRefine stopped while starting ({status})")); }
            if or.token().is_ok() { return Ok(or); }
            if Instant::now() > until { return Err("OpenRefine did not start within 3 minutes".into()); }
            std::thread::sleep(Duration::from_millis(500));
        }
    }

    fn token(&self) -> Result<String> {
        let mut r = self.agent.get(format!("{}/command/core/get-csrf-token", self.url)).call().map_err(err)?;
        let j: J = serde_json::from_reader(r.body_mut().as_reader()).map_err(err)?;
        j["token"].as_str().map(String::from).ok_or_else(|| "no CSRF token".to_string())
    }

    /// Import an OpenRefine project archive; its project id.
    pub fn import(&self, archive: &[u8]) -> Result<String> {
        let boundary = format!("oxrefine{:x}", crate::recon::new_id());
        let mut body = format!("--{boundary}\r\nContent-Disposition: form-data; name=\"project-name\"\r\n\r\nOxRefine\r\n--{boundary}\r\nContent-Disposition: form-data; name=\"project-file\"; filename=\"project.openrefine.tar.gz\"\r\nContent-Type: application/gzip\r\n\r\n").into_bytes();
        body.extend_from_slice(archive);
        body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
        let r = self.agent.post(format!("{}/command/core/import-project?csrf_token={}", self.url, self.token()?))
            .header("Content-Type", format!("multipart/form-data; boundary={boundary}")).send(&body[..]).map_err(err)?;
        let location = r.headers().get("location").and_then(|l| l.to_str().ok()).unwrap_or("");
        location.split(['?', '&']).find_map(|kv| kv.strip_prefix("project=")).map(String::from)
            .ok_or_else(|| format!("OpenRefine did not import the project (HTTP {})", r.status()))
    }

    /// Run OpenRefine operations (a recipe) on a project and wait until they are done.
    pub fn apply(&self, project: &str, ops: &J) -> Result<()> {
        let mut r = self.agent.post(format!("{}/command/core/apply-operations?csrf_token={}", self.url, self.token()?))
            .send_form([("project", project), ("operations", &ops.to_string())]).map_err(err)?;
        let reply: J = serde_json::from_reader(r.body_mut().as_reader()).map_err(|e| format!("OpenRefine's reply: {e}"))?;
        if reply["code"] == "error" { return Err(format!("OpenRefine: {}", reply["message"].as_str().unwrap_or("the steps failed"))); }
        loop {
            crate::jobs::tick(0)?;
            let mut r = self.agent.get(format!("{}/command/core/get-processes?project={project}", self.url)).call().map_err(err)?;
            let j: J = serde_json::from_reader(r.body_mut().as_reader()).map_err(err)?;
            if j["processes"].as_array().is_none_or(|p| p.is_empty()) { return Ok(()); }
            std::thread::sleep(Duration::from_millis(200));
        }
    }

    /// The project as an OpenRefine archive.
    pub fn export(&self, project: &str) -> Result<Vec<u8>> {
        let mut r = self.agent.post(format!("{}/command/core/export-project?project={project}&csrf_token={}", self.url, self.token()?)).send_empty().map_err(err)?;
        if r.status() != 200 { return Err(format!("OpenRefine did not export the project (HTTP {})", r.status())); }
        r.body_mut().with_config().limit(u64::MAX).read_to_vec().map_err(err)
    }
}

/// Run `ops` on the project in the OpenRefine at `folder`; the result, as an OpenRefine archive at
/// `out` (OpenRefine's history included), for opening as a new project.
pub fn run(folder: &Path, p: &Project, ops: &J, out: &Path) -> Result<()> {
    if !ops.is_array() { return Err("The steps must be a JSON array of OpenRefine operations".into()); }
    let dir = tempfile::tempdir().map_err(err)?;
    let archive = dir.path().join("project.openrefine.tar.gz");
    crate::jobs::progress("Writing the project for OpenRefine", 0);
    crate::orproject::write(p, &archive)?;
    crate::jobs::progress("Starting OpenRefine", 0);
    let or = OpenRefine::start(folder)?;
    let project = or.import(&std::fs::read(&archive).map_err(err)?)?;
    crate::jobs::progress("Running the steps in OpenRefine", 0);
    or.apply(&project, ops)?;
    let bytes = or.export(&project)?;
    std::fs::write(out, bytes).map_err(|e| format!("{}: {e}", out.display()))
}
