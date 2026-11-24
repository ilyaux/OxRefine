//! `ox serve`'s access rule as reverse proxies and browsers meet it: the token, Host names (this
//! machine's, --allow-host), refusals that say what to do (page, API, stderr once), HEAD, /healthz,
//! Referrer-Policy and --log.
use std::{
    io::{BufRead, BufReader, Read, Write},
    net::TcpStream,
    process::{Child, Command, Stdio},
    path::PathBuf,
    sync::{atomic::{AtomicU64, Ordering}, Arc, Mutex},
    time::{Duration, Instant},
};

struct Server {
    child: Child,
    port: u16,
    stderr: Arc<Mutex<Vec<String>>>,
    data: PathBuf,
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.data);
    }
}

impl Server {
    fn start(args: &[&str], env: &[(&str, &str)]) -> Server {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let data = std::env::temp_dir().join(format!("ox-access-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        let mut command = Command::new(env!("CARGO_BIN_EXE_ox"));
        command.args(["serve", "--port", "0", "--data"]).arg(&data).args(args).env_remove("OXREFINE_TOKEN");
        for (k, v) in env {
            command.env(k, v);
        }
        let mut child = command.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped()).spawn().unwrap();
        let stderr = Arc::new(Mutex::new(vec![]));
        let lines = stderr.clone();
        let pipe = child.stderr.take().unwrap();
        std::thread::spawn(move || {
            for line in BufReader::new(pipe).lines().map_while(Result::ok) {
                lines.lock().unwrap().push(line);
            }
        });
        let until = Instant::now() + Duration::from_secs(20);
        let port = loop {
            let found = stderr.lock().unwrap().iter().find_map(|l| l.strip_prefix("OxRefine on http://").and_then(|r| r.split([':', '/']).nth(1)).and_then(|p| p.parse().ok()));
            if let Some(port) = found {
                break port;
            }
            assert!(Instant::now() < until, "ox serve didn't start: {:?}", stderr.lock().unwrap());
            std::thread::sleep(Duration::from_millis(20));
        };
        Server { child, port, stderr, data }
    }

    /// A raw request: status, headers (lowercased names) and body.
    fn request(&self, method: &str, path: &str, host: &str, headers: &[&str], body: &str) -> (u16, String, String) {
        let mut stream = TcpStream::connect(("127.0.0.1", self.port)).unwrap();
        stream.set_read_timeout(Some(Duration::from_secs(20))).unwrap();
        let extra: String = headers.iter().map(|h| format!("{h}\r\n")).collect();
        write!(stream, "{method} {path} HTTP/1.1\r\nHost: {host}\r\n{extra}Content-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
        let mut reply = vec![];
        stream.read_to_end(&mut reply).unwrap();
        let reply = String::from_utf8_lossy(&reply).into_owned();
        let (head, body) = reply.split_once("\r\n\r\n").unwrap_or((&reply, ""));
        (head.split_whitespace().nth(1).unwrap().parse().unwrap(), head.to_ascii_lowercase(), body.to_string())
    }

    fn api(&self, host: &str, headers: &[&str]) -> (u16, String) {
        let headers: Vec<&str> = [&["Content-Type: application/json"], headers].concat();
        let (status, _, body) = self.request("POST", "/api/projects", host, &headers, "{}");
        (status, body)
    }

    fn stderr(&self) -> Vec<String> {
        // what the server printed so far, after a moment for the pipe
        std::thread::sleep(Duration::from_millis(150));
        self.stderr.lock().unwrap().clone()
    }
}

#[test]
fn without_a_token_only_this_machine_and_allowed_names_are_answered() {
    let s = Server::start(&[], &[]);
    let local = format!("127.0.0.1:{}", s.port);
    // this machine by any of its names, with the port or without, IPv6 included
    for host in [local.as_str(), "localhost", "localhost:80", &format!("[::1]:{}", s.port)] {
        assert_eq!(s.api(host, &[]).0, 200, "{host}");
    }
    assert_eq!(s.request("GET", "/", &local, &[], "").0, 200);
    // a proxy that passes on the visitor's host name: the page and the API say what to do
    let (status, _, page) = s.request("GET", "/", "intranet.example.com", &[], "");
    assert_eq!(status, 403);
    assert!(page.contains("--allow-host intranet.example.com") && page.contains("--token"), "{page}");
    assert!(!page.contains("<div id=\"root\">"), "no app for a refused request");
    let (status, error) = s.api("intranet.example.com:443", &[]);
    assert_eq!(status, 403);
    assert!(error.contains("--allow-host intranet.example.com"), "{error}");
    // a host name is written into the page as text, never as markup
    let (_, _, page) = s.request("GET", "/", "<script>x</script>", &[], "");
    assert!(!page.contains("<script>x"), "{page}");
    // whoever runs it learns it once on stderr, however many requests are refused
    s.request("GET", "/", "intranet.example.com", &[], "");
    let told: Vec<_> = s.stderr().into_iter().filter(|l| l.contains("refused requests addressed to intranet.example.com")).collect();
    assert_eq!(told.len(), 1, "{told:?}");
    assert!(told[0].contains("--allow-host intranet.example.com"));
}

#[test]
fn allow_host_answers_a_proxy_that_keeps_the_visitors_host_name() {
    let s = Server::start(&["--allow-host", "Intranet.Example.com"], &[]);
    assert!(s.stderr().iter().any(|l| l.contains("also answering requests addressed to intranet.example.com")));
    for host in ["intranet.example.com", "intranet.example.com:443", "INTRANET.example.COM"] {
        assert_eq!(s.api(host, &[]).0, 200, "{host}");
        assert_eq!(s.request("GET", "/", host, &[], "").0, 200, "{host}");
    }
    assert_eq!(s.api("other.example.com", &[]).0, 403);
    // MCP from a web page: only from an address this server answers
    let init = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"t","version":"1"}}}"#;
    let mcp = |origin: &str| s.request("POST", "/mcp", "intranet.example.com", &["Content-Type: application/json", "Accept: application/json, text/event-stream", &format!("Origin: {origin}")], init).0;
    assert_eq!(mcp("https://intranet.example.com"), 200);
    assert_eq!(mcp("https://evil.example.net"), 403);
}

#[test]
fn a_token_from_the_environment_guards_everything_but_the_health_check() {
    let s = Server::start(&[], &[("OXREFINE_TOKEN", "env-secret")]);
    // a token makes the Host header irrelevant: the token decides
    let (status, error) = s.api("intranet.example.com", &[]);
    assert_eq!(status, 403);
    assert!(error.contains("?token="), "{error}");
    assert_eq!(s.api("intranet.example.com", &["Authorization: Bearer env-secret"]).0, 200);
    assert_eq!(s.api("intranet.example.com", &["Authorization: Bearer wrong"]).0, 403);
    let (status, _, page) = s.request("GET", "/", "intranet.example.com", &[], "");
    assert_eq!(status, 403);
    assert!(page.contains("?token="), "{page}");
    assert_eq!(s.request("GET", "/?token=env-secret", "intranet.example.com", &[], "").0, 200);
    // the token isn't on the command line, so ps doesn't show it; the link still is printed for the person who started it
    assert!(s.stderr().iter().any(|l| l.contains("?token=env-secret")));
    // load balancers and container health checks need no token and learn nothing else
    let (status, _, body) = s.request("GET", "/healthz", "anything", &[], "");
    assert_eq!((status, body.as_str()), (200, "ok"));
}

#[test]
fn head_requests_referrer_policy_and_the_request_log() {
    let s = Server::start(&["--token", "log-secret", "--log"], &[]);
    // HEAD answers like GET, without the body
    let (status, head, body) = s.request("HEAD", "/?token=log-secret", "localhost", &[], "");
    assert_eq!(status, 200);
    assert!(body.is_empty(), "{body}");
    // no address with the token goes on to other sites, or into a proxy's log as the referrer
    assert!(head.contains("referrer-policy: no-referrer"), "{head}");
    s.api("localhost", &["Authorization: Bearer log-secret"]);
    let log = s.stderr();
    assert!(log.iter().any(|l| l.starts_with("HEAD / 200 ")), "{log:?}");
    assert!(log.iter().any(|l| l.starts_with("POST /api/projects 200 ")), "{log:?}");
    // the log never holds the token, only the printed link does
    assert_eq!(log.iter().filter(|l| l.contains("log-secret")).count(), 1, "{log:?}");
}
