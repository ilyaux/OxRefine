//! Google Sheets and Drive, as OpenRefine's gdata extension did (it is no longer bundled with
//! OpenRefine 3.10.1): import a sheet by its link or from the signed-in Drive, export rows to a new
//! Google Sheet, upload the project archive to Drive.
//!
//! A sheet shared with "anyone with the link" imports without signing in. Everything else signs in
//! with OAuth 2.0 for installed apps (loopback redirect, PKCE). Google issues OAuth clients to the
//! people who register them, so OxRefine ships none: set `google.clientId` and `google.clientSecret`
//! in Preferences (a "Desktop app" client from Google Cloud Console), or OXREFINE_GOOGLE_CLIENT_ID /
//! OXREFINE_GOOGLE_CLIENT_SECRET. The refresh token is kept in google-token.json in the data folder.
use crate::Result;
use serde_json::{json, Value as J};
use std::{
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    sync::Mutex,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const SCOPES: &str = "https://www.googleapis.com/auth/drive.file https://www.googleapis.com/auth/drive.readonly";
pub const SHEET_MIME: &str = "application/vnd.google-apps.spreadsheet";
const XLSX_MIME: &str = "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet";

/// Google's endpoints; tests point them at a local server.
fn endpoint(name: &str, default: &str) -> String {
    std::env::var(format!("OXREFINE_GOOGLE_{name}")).unwrap_or_else(|_| default.to_string())
}
fn auth_url() -> String {
    endpoint("AUTH_URL", "https://accounts.google.com/o/oauth2/v2/auth")
}
fn token_url() -> String {
    endpoint("TOKEN_URL", "https://oauth2.googleapis.com/token")
}
fn api() -> String {
    endpoint("API_URL", "https://www.googleapis.com")
}
fn docs() -> String {
    endpoint("DOCS_URL", "https://docs.google.com")
}

/// The spreadsheet id (and the sheet's gid, when the link names one) in a Google Sheets link.
pub fn sheet_id(link: &str) -> Option<(String, Option<String>)> {
    let u = url::Url::parse(link.trim()).ok()?;
    if u.host_str() != Some("docs.google.com") {
        return None;
    }
    let mut seg = u.path_segments()?;
    if seg.next()? != "spreadsheets" || seg.next()? != "d" {
        return None;
    }
    let id = seg.next().filter(|s| !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'))?;
    let gid = u.fragment().and_then(|f| f.strip_prefix("gid=")).map(str::to_string).or_else(|| u.query_pairs().find(|(k, _)| k == "gid").map(|(_, v)| v.into_owned()));
    Some((id.to_string(), gid))
}

// ---------------------------------------------------------------- configuration and tokens

struct Client {
    id: String,
    secret: String,
}

fn client(root: &Path) -> Option<Client> {
    let prefs: serde_json::Map<String, J> = std::fs::read(root.join("preferences.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
    let get = |pref: &str, env: &str| std::env::var(env).ok().filter(|v| !v.is_empty()).or_else(|| prefs.get(pref).and_then(|v| v.as_str()).filter(|v| !v.is_empty()).map(str::to_string));
    Some(Client { id: get("google.clientId", "OXREFINE_GOOGLE_CLIENT_ID")?, secret: get("google.clientSecret", "OXREFINE_GOOGLE_CLIENT_SECRET").unwrap_or_default() })
}

fn token_path(root: &Path) -> PathBuf {
    root.join("google-token.json")
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

fn save_token(root: &Path, t: &J) -> Result<()> {
    let path = token_path(root);
    std::fs::write(&path, serde_json::to_vec_pretty(t).unwrap()).map_err(|e| format!("{}: {e}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

fn load_token(root: &Path) -> Option<J> {
    std::fs::read(token_path(root)).ok().and_then(|b| serde_json::from_slice(&b).ok())
}

fn read_json(mut res: ureq::http::Response<ureq::Body>) -> Result<J> {
    let status = res.status();
    let text = res.body_mut().read_to_string().map_err(|e| e.to_string())?;
    let j: J = serde_json::from_str(&text).unwrap_or(J::String(text.clone()));
    if !status.is_success() {
        let msg = j["error"]["message"].as_str().or(j["error_description"].as_str()).or(j["error"].as_str()).unwrap_or(&text);
        return Err(format!("Google answered {}: {msg}", status.as_u16()));
    }
    Ok(j)
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder().http_status_as_error(false).timeout_global(Some(Duration::from_secs(120))).build().into()
}

/// A valid access token, refreshed when it has expired.
fn access_token(root: &Path) -> Result<String> {
    let mut t = load_token(root).ok_or("Sign in to Google first (Google Sheets → Sign in).")?;
    if t["expires_at"].as_u64().unwrap_or(0) > now() + 60 {
        if let Some(a) = t["access_token"].as_str() {
            return Ok(a.to_string());
        }
    }
    let c = client(root).ok_or("Set google.clientId in Preferences to use Google Drive.")?;
    let refresh = t["refresh_token"].as_str().ok_or("Sign in to Google again: the saved sign-in has no refresh token.")?.to_string();
    let res = agent()
        .post(&token_url())
        .send_form([("client_id", c.id.as_str()), ("client_secret", c.secret.as_str()), ("refresh_token", refresh.as_str()), ("grant_type", "refresh_token")])
        .map_err(|e| format!("Google sign-in: {e}"))?;
    let fresh = read_json(res).map_err(|e| format!("{e} (sign in again)"))?;
    t["access_token"] = fresh["access_token"].clone();
    t["expires_at"] = json!(now() + fresh["expires_in"].as_u64().unwrap_or(3600));
    save_token(root, &t)?;
    t["access_token"].as_str().map(str::to_string).ok_or_else(|| "Google returned no access token".into())
}

// ---------------------------------------------------------------- sign-in (loopback + PKCE)

#[derive(Default)]
struct Pending {
    started: Option<Instant>,
    error: Option<String>,
}
static SIGN_IN: Mutex<Pending> = Mutex::new(Pending { started: None, error: None });

fn random_token(bytes: usize) -> String {
    use base64::Engine;
    let mut buf = vec![0u8; bytes];
    // ponytail: entropy from the clock, a fresh address and the process id hashed with SHA-256; use getrandom if a CSPRNG is ever required elsewhere
    let mut seed = format!("{:?}{:p}{}", SystemTime::now(), &buf, std::process::id()).into_bytes();
    for chunk in buf.chunks_mut(32) {
        use sha2::Digest;
        let d = sha2::Sha256::digest(&seed);
        chunk.copy_from_slice(&d[..chunk.len()]);
        seed = d.to_vec();
    }
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(buf)
}

/// Start signing in: returns the Google page to open. A loopback listener waits (up to 5 minutes) for
/// the browser to come back with the code, exchanges it and saves the token.
pub fn sign_in(root: &Path) -> Result<J> {
    use base64::Engine;
    use sha2::Digest;
    let c = client(root).ok_or("Google sign-in needs an OAuth client: set google.clientId and google.clientSecret in Preferences (a Desktop app client from Google Cloud Console).")?;
    let listener = std::net::TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
    let redirect = format!("http://127.0.0.1:{}", listener.local_addr().map_err(|e| e.to_string())?.port());
    let verifier = random_token(48);
    let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(sha2::Sha256::digest(verifier.as_bytes()));
    let state = random_token(16);
    let mut url = url::Url::parse(&auth_url()).map_err(|e| e.to_string())?;
    url.query_pairs_mut()
        .append_pair("client_id", &c.id)
        .append_pair("redirect_uri", &redirect)
        .append_pair("response_type", "code")
        .append_pair("scope", SCOPES)
        .append_pair("code_challenge", &challenge)
        .append_pair("code_challenge_method", "S256")
        .append_pair("state", &state)
        .append_pair("access_type", "offline")
        .append_pair("prompt", "consent");
    *SIGN_IN.lock().unwrap() = Pending { started: Some(Instant::now()), error: None };
    let root = root.to_path_buf();
    std::thread::spawn(move || {
        let result = wait_for_code(&listener, &state).and_then(|code| {
            let res = agent()
                .post(&token_url())
                .send_form([
                    ("client_id", c.id.as_str()),
                    ("client_secret", c.secret.as_str()),
                    ("code", code.as_str()),
                    ("code_verifier", verifier.as_str()),
                    ("grant_type", "authorization_code"),
                    ("redirect_uri", redirect.as_str()),
                ])
                .map_err(|e| format!("Google sign-in: {e}"))?;
            let mut t = read_json(res)?;
            t["expires_at"] = json!(now() + t["expires_in"].as_u64().unwrap_or(3600));
            save_token(&root, &t)?;
            if let Ok(email) = whoami(&root) {
                t["email"] = json!(email);
                save_token(&root, &t)?;
            }
            Ok(())
        });
        let mut p = SIGN_IN.lock().unwrap();
        p.started = None;
        p.error = result.err();
    });
    Ok(json!({ "url": url.to_string() }))
}

/// Accept connections until one carries `?code=...&state=<state>` (or the 5 minutes run out).
fn wait_for_code(listener: &std::net::TcpListener, state: &str) -> Result<String> {
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    let deadline = Instant::now() + Duration::from_secs(300);
    while Instant::now() < deadline {
        let (mut stream, _) = match listener.accept() {
            Ok(s) => s,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(100));
                continue;
            }
            Err(e) => return Err(e.to_string()),
        };
        let _ = stream.set_nonblocking(false);
        let mut line = String::new();
        BufReader::new(&stream).read_line(&mut line).map_err(|e| e.to_string())?;
        let target = line.split_whitespace().nth(1).unwrap_or("/");
        let u = url::Url::parse(&format!("http://127.0.0.1{target}")).map_err(|e| e.to_string())?;
        let q: std::collections::HashMap<String, String> = u.query_pairs().into_owned().collect();
        let (body, outcome) = match (q.get("code"), q.get("error"), q.get("state").map(String::as_str)) {
            (Some(code), _, Some(s)) if s == state => ("Signed in to Google. You can close this tab and go back to OxRefine.", Some(Ok(code.clone()))),
            (_, Some(e), _) => ("Google sign-in was not completed. You can close this tab.", Some(Err(format!("Google sign-in: {e}")))),
            _ => ("", None),
        };
        let page = format!("<!doctype html><meta charset=utf-8><title>OxRefine</title><p style=\"font:16px system-ui;margin:3em\">{body}</p>");
        let _ = write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{page}", page.len());
        if let Some(o) = outcome {
            return o;
        }
    }
    Err("Google sign-in timed out after 5 minutes".into())
}

fn whoami(root: &Path) -> Result<String> {
    let token = access_token(root)?;
    let res = agent().get(&format!("{}/drive/v3/about?fields=user(emailAddress)", api())).header("Authorization", &format!("Bearer {token}")).call().map_err(|e| e.to_string())?;
    Ok(read_json(res)?["user"]["emailAddress"].as_str().unwrap_or("").to_string())
}

/// Whether sign-in is configured, in progress, done (and as whom), or failed.
pub fn status(root: &Path) -> J {
    let p = SIGN_IN.lock().unwrap();
    let token = load_token(root);
    json!({
        "configured": client(root).is_some(),
        "signedIn": token.is_some(),
        "email": token.as_ref().and_then(|t| t["email"].as_str().map(str::to_string)),
        "pending": p.started.is_some_and(|s| s.elapsed() < Duration::from_secs(300)),
        "error": p.error,
    })
}

pub fn sign_out(root: &Path) -> Result<J> {
    // revoke when possible; the local token goes either way
    if let Some(t) = load_token(root) {
        if let Some(r) = t["refresh_token"].as_str().or(t["access_token"].as_str()) {
            let _ = agent().post(&endpoint("REVOKE_URL", "https://oauth2.googleapis.com/revoke")).send_form([("token", r)]);
        }
    }
    let _ = std::fs::remove_file(token_path(root));
    Ok(status(root))
}

// ---------------------------------------------------------------- Drive

/// Spreadsheets in the signed-in Drive, newest first; `query` filters by name.
pub fn list_sheets(root: &Path, query: &str) -> Result<J> {
    let token = access_token(root)?;
    let mut q = format!("mimeType='{SHEET_MIME}' and trashed=false");
    if !query.trim().is_empty() {
        q.push_str(&format!(" and name contains '{}'", query.trim().replace('\\', "\\\\").replace('\'', "\\'")));
    }
    let mut u = url::Url::parse(&format!("{}/drive/v3/files", api())).map_err(|e| e.to_string())?;
    u.query_pairs_mut().append_pair("q", &q).append_pair("orderBy", "modifiedTime desc").append_pair("pageSize", "100").append_pair("fields", "files(id,name,modifiedTime,owners(displayName),webViewLink)");
    let res = agent().get(u.as_str()).header("Authorization", &format!("Bearer {token}")).call().map_err(|e| e.to_string())?;
    Ok(read_json(res)?["files"].clone())
}

/// Download a spreadsheet as .xlsx (every sheet) into `dir`: through Drive when signed in, else
/// through the public export link, which works for sheets shared with anyone who has the link.
pub fn download(root: Option<&Path>, dir: &Path, id: &str) -> Result<(PathBuf, String)> {
    let signed_in = root.is_some_and(|r| load_token(r).is_some());
    crate::jobs::progress("Connecting to Google", 0);
    let mut res = if signed_in {
        let token = access_token(root.unwrap())?;
        agent()
            .get(&format!("{}/drive/v3/files/{id}/export?mimeType={XLSX_MIME}", api()))
            .header("Authorization", &format!("Bearer {token}"))
            .call()
            .map_err(|e| e.to_string())?
    } else {
        agent().get(&format!("{}/spreadsheets/d/{id}/export?format=xlsx", docs())).call().map_err(|e| e.to_string())?
    };
    let status = res.status();
    let html = res.headers().get("content-type").and_then(|v| v.to_str().ok()).is_some_and(|c| c.starts_with("text/html"));
    if !status.is_success() || html {
        return Err(if signed_in {
            format!("Google Drive answered {} for this sheet: check that your account can open it", status.as_u16())
        } else {
            "This sheet isn't shared with anyone who has the link. Share it that way, or sign in to Google to import it.".into()
        });
    }
    let name = res
        .headers()
        .get("content-disposition")
        .and_then(|v| v.to_str().ok())
        .and_then(disposition_name)
        .unwrap_or_else(|| format!("Google Sheet {id}.xlsx"));
    let name: String = name.chars().map(|c| if "\\/:*?\"<>|".contains(c) { '_' } else { c }).collect();
    let name = if name.to_ascii_lowercase().ends_with(".xlsx") { name } else { format!("{name}.xlsx") };
    let size = res.headers().get("content-length").and_then(|v| v.to_str().ok()?.parse().ok()).unwrap_or(0);
    crate::jobs::progress_units("Downloading the Google Sheet", size, "bytes");
    let folder = dir.join(format!("google-{}", random_token(6)));
    std::fs::create_dir_all(&folder).map_err(|e| format!("{}: {e}", folder.display()))?;
    let path = folder.join(&name);
    let mut file = std::fs::File::create(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    std::io::copy(&mut crate::jobs::Counted(res.body_mut().as_reader()), &mut file).map_err(|e| e.to_string())?;
    Ok((path, name.trim_end_matches(".xlsx").to_string()))
}

/// `filename*=UTF-8''...` or `filename="..."` from a Content-Disposition header.
fn disposition_name(h: &str) -> Option<String> {
    for part in h.split(';').map(str::trim) {
        if let Some(v) = part.strip_prefix("filename*=") {
            let v = v.split_once("''").map_or(v, |(_, rest)| rest);
            return url::form_urlencoded::parse(format!("x={}", v.replace('+', "%2B")).as_bytes()).next().map(|(_, v)| v.into_owned());
        }
    }
    h.split(';').map(str::trim).find_map(|p| p.strip_prefix("filename=")).map(|v| v.trim_matches('"').to_string())
}

/// Upload a file to Drive; `convert` turns spreadsheet data into a Google Sheet. Returns its id and link.
pub fn upload(root: &Path, name: &str, mime: &str, data: &[u8], convert: bool) -> Result<J> {
    let token = access_token(root)?;
    let boundary = format!("oxrefine-{}", random_token(12));
    let meta = json!({ "name": name, "mimeType": if convert { SHEET_MIME } else { mime } });
    let mut body = format!("--{boundary}\r\nContent-Type: application/json; charset=UTF-8\r\n\r\n{meta}\r\n--{boundary}\r\nContent-Type: {mime}\r\n\r\n").into_bytes();
    body.extend_from_slice(data);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    crate::jobs::progress("Uploading to Google Drive", 0);
    let res = agent()
        .post(&format!("{}/upload/drive/v3/files?uploadType=multipart&fields=id,name,webViewLink", api()))
        .header("Authorization", &format!("Bearer {token}"))
        .header("Content-Type", &format!("multipart/related; boundary={boundary}"))
        .send(&body[..])
        .map_err(|e| e.to_string())?;
    read_json(res)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sheet_links() {
        assert_eq!(sheet_id("https://docs.google.com/spreadsheets/d/1AbC-_9/edit#gid=42"), Some(("1AbC-_9".into(), Some("42".into()))));
        assert_eq!(sheet_id("https://docs.google.com/spreadsheets/d/1AbC/edit?usp=sharing"), Some(("1AbC".into(), None)));
        assert_eq!(sheet_id("https://example.com/spreadsheets/d/1AbC/edit"), None);
        assert_eq!(sheet_id("https://docs.google.com/document/d/1AbC/edit"), None);
        assert_eq!(disposition_name("attachment; filename=\"a.xlsx\"; filename*=UTF-8''Sales%20Q1.xlsx").as_deref(), Some("Sales Q1.xlsx"));
    }

    /// The whole sign-in, listing, download and upload against a local stand-in for Google.
    #[test]
    fn oauth_and_drive_against_a_mock() {
        let server = tiny_mock();
        let base = format!("http://{}", server.0);
        // SAFETY: tests in this module don't read these variables concurrently with other tests
        unsafe {
            std::env::set_var("OXREFINE_GOOGLE_TOKEN_URL", format!("{base}/token"));
            std::env::set_var("OXREFINE_GOOGLE_API_URL", &base);
            std::env::set_var("OXREFINE_GOOGLE_DOCS_URL", &base);
            std::env::set_var("OXREFINE_GOOGLE_AUTH_URL", format!("{base}/auth"));
            std::env::set_var("OXREFINE_GOOGLE_REVOKE_URL", format!("{base}/revoke"));
        }
        let root = tempfile::tempdir().unwrap();
        // a public sheet works without a client or a sign-in
        let (path, name) = download(Some(root.path()), root.path(), "PUBLIC").unwrap();
        assert_eq!(name, "Public sheet");
        assert_eq!(std::fs::read(&path).unwrap(), b"XLSX-PUBLIC");
        assert!(download(Some(root.path()), root.path(), "PRIVATE").unwrap_err().contains("isn't shared"));
        // sign in needs a client id
        assert!(sign_in(root.path()).unwrap_err().contains("google.clientId"));
        std::fs::write(root.path().join("preferences.json"), r#"{"google.clientId":"cid","google.clientSecret":"sec"}"#).unwrap();
        let url = sign_in(root.path()).unwrap()["url"].as_str().unwrap().to_string();
        let u = url::Url::parse(&url).unwrap();
        let q: std::collections::HashMap<_, _> = u.query_pairs().into_owned().collect();
        assert_eq!(q["code_challenge_method"], "S256");
        // the browser comes back to the loopback address with the code
        let back = format!("{}?code=CODE&state={}", q["redirect_uri"], q["state"]);
        let page = ureq::get(&back).call().unwrap().body_mut().read_to_string().unwrap();
        assert!(page.contains("Signed in"));
        for _ in 0..100 {
            if !status(root.path())["pending"].as_bool().unwrap() {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let s = status(root.path());
        assert_eq!(s["signedIn"], true, "{s}");
        assert_eq!(s["email"], "ann@example.com");
        assert_eq!(list_sheets(root.path(), "Sal'es").unwrap()[0]["name"], "Sales");
        let (path, _) = download(Some(root.path()), root.path(), "PRIVATE").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"XLSX-DRIVE");
        let up = upload(root.path(), "Rows", "text/csv", b"a,b\n1,2\n", true).unwrap();
        assert_eq!(up["webViewLink"], "https://docs.google.com/spreadsheets/d/NEW");
        // an expired token is refreshed
        let mut t = load_token(root.path()).unwrap();
        t["expires_at"] = json!(0);
        save_token(root.path(), &t).unwrap();
        assert_eq!(access_token(root.path()).unwrap(), "ACCESS2");
        assert_eq!(sign_out(root.path()).unwrap()["signedIn"], false);
        assert_eq!(server.1.lock().unwrap().iter().filter(|r| r.starts_with("POST /token")).count(), 2);
    }

    /// A tiny HTTP server answering like Google's OAuth and Drive endpoints; records request lines.
    fn tiny_mock() -> (std::net::SocketAddr, std::sync::Arc<Mutex<Vec<String>>>) {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap();
        let log = std::sync::Arc::new(Mutex::new(vec![]));
        let seen = log.clone();
        std::thread::spawn(move || {
            for s in l.incoming().flatten() {
                let mut r = BufReader::new(s.try_clone().unwrap());
                let mut line = String::new();
                r.read_line(&mut line).unwrap();
                let mut len = 0;
                let mut auth = String::new();
                loop {
                    let mut h = String::new();
                    r.read_line(&mut h).unwrap();
                    if h.trim().is_empty() {
                        break;
                    }
                    let lower = h.to_ascii_lowercase();
                    if let Some(v) = lower.strip_prefix("content-length:") {
                        len = v.trim().parse().unwrap();
                    }
                    if lower.starts_with("authorization:") {
                        auth = h.trim().to_string();
                    }
                }
                let mut body = vec![0; len];
                std::io::Read::read_exact(&mut r, &mut body).unwrap();
                let body = String::from_utf8_lossy(&body).to_string();
                seen.lock().unwrap().push(line.trim().to_string());
                let bearer = auth.ends_with("ACCESS") || auth.ends_with("ACCESS2");
                let (status, ctype, extra, out): (u16, &str, &str, Vec<u8>) = match line.split_whitespace().nth(1).unwrap_or("") {
                    p if p.starts_with("/token") && body.contains("grant_type=authorization_code") && body.contains("code_verifier=") => {
                        (200, "application/json", "", br#"{"access_token":"ACCESS","refresh_token":"REFRESH","expires_in":3600}"#.to_vec())
                    }
                    p if p.starts_with("/token") && body.contains("grant_type=refresh_token") => (200, "application/json", "", br#"{"access_token":"ACCESS2","expires_in":3600}"#.to_vec()),
                    p if p.starts_with("/drive/v3/about") && bearer => (200, "application/json", "", br#"{"user":{"emailAddress":"ann@example.com"}}"#.to_vec()),
                    p if p.starts_with("/drive/v3/files?") && bearer && p.contains("Sal%5C%27es") => (200, "application/json", "", br#"{"files":[{"id":"S1","name":"Sales"}]}"#.to_vec()),
                    p if p.starts_with("/drive/v3/files/PRIVATE/export") && bearer => (200, XLSX_MIME, "", b"XLSX-DRIVE".to_vec()),
                    "/spreadsheets/d/PUBLIC/export?format=xlsx" => (200, XLSX_MIME, "Content-Disposition: attachment; filename*=UTF-8''Public%20sheet.xlsx\r\n", b"XLSX-PUBLIC".to_vec()),
                    "/spreadsheets/d/PRIVATE/export?format=xlsx" => (200, "text/html", "", b"<html>sign in</html>".to_vec()),
                    p if p.starts_with("/upload/drive/v3/files") && bearer && body.contains(SHEET_MIME) && body.contains("a,b") => {
                        (200, "application/json", "", br#"{"id":"NEW","name":"Rows","webViewLink":"https://docs.google.com/spreadsheets/d/NEW"}"#.to_vec())
                    }
                    "/revoke" => (200, "application/json", "", b"{}".to_vec()),
                    _ => (401, "application/json", "", br#"{"error":{"message":"unexpected request"}}"#.to_vec()),
                };
                let mut s = s;
                let _ = write!(s, "HTTP/1.1 {status} X\r\nContent-Type: {ctype}\r\n{extra}Content-Length: {}\r\nConnection: close\r\n\r\n", out.len());
                let _ = s.write_all(&out);
            }
        });
        (addr, log)
    }
}
