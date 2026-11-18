//! The stuck-state case from the launch review: the local index file is
//! corrupt while the state file already names the current sha, so every
//! refresh takes the "already current" path and fails parsing forever.
//! `refresh` must treat the corrupt copy as missing, re-download it, and only
//! then report "up to date" on the next run.

use std::ffi::OsString;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::Mutex;
use std::thread::JoinHandle;

const INDEX_BODY: &str =
    r#"{"schema_version": 1, "generated_at": "2026-11-18T00:00:00Z", "sites": []}"#;

/// Tests in this file mutate process env; serialise them.
static ENV_LOCK: Mutex<()> = Mutex::new(());

/// Restore process env on drop so parallel tests sharing the process are safe.
struct EnvGuard {
    saved: Vec<(String, Option<OsString>)>,
}

impl EnvGuard {
    fn new() -> Self {
        Self { saved: Vec::new() }
    }

    fn set(&mut self, key: &str, value: &str) {
        if !self.saved.iter().any(|(k, _)| k == key) {
            self.saved.push((key.into(), std::env::var_os(key)));
        }
        unsafe { std::env::set_var(key, value) };
    }

    fn remove(&mut self, key: &str) {
        if !self.saved.iter().any(|(k, _)| k == key) {
            self.saved.push((key.into(), std::env::var_os(key)));
        }
        unsafe { std::env::remove_var(key) };
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        for (key, value) in self.saved.drain(..) {
            match value {
                Some(v) => unsafe { std::env::set_var(&key, v) },
                None => unsafe { std::env::remove_var(&key) },
            }
        }
    }
}

fn respond(mut stream: std::net::TcpStream, body: &str) {
    let mut buf = vec![0u8; 8192];
    let mut req = Vec::new();
    loop {
        let n = stream.read(&mut buf).unwrap();
        req.extend_from_slice(&buf[..n]);
        if req.windows(4).any(|w| w == b"\r\n\r\n") || n == 0 {
            break;
        }
    }
    let line = String::from_utf8_lossy(&req);
    let path = line.split_whitespace().nth(1).unwrap_or("/").to_string();
    // The pointer lives at /index.json; the index file at /files/index.json.
    let body = if path == "/index.json" {
        let sha = agent_webmcp::store::sha256_hex(INDEX_BODY.as_bytes());
        serde_json::json!({ "sha256": sha, "url": "files/index.json" }).to_string()
    } else {
        assert_eq!(path, "/files/index.json", "unexpected request {path}");
        body.to_string()
    };
    let resp = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(resp.as_bytes()).unwrap();
}

/// Serve the pointer at /index.json and the index at /files/index.json.
/// Handles exactly `requests` connections, then exits.
fn serve(requests: usize) -> (String, JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!(
        "http://127.0.0.1:{}/index.json",
        listener.local_addr().unwrap().port()
    );
    let handle = std::thread::spawn(move || {
        for stream in listener.incoming().take(requests) {
            respond(stream.unwrap(), INDEX_BODY);
        }
    });
    (base, handle)
}

#[test]
fn corrupt_current_copy_is_redownloaded() {
    let _lock = ENV_LOCK.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let sha = agent_webmcp::store::sha256_hex(INDEX_BODY.as_bytes());

    // First refresh: pointer + file (2 requests). Second: pointer only, the
    // sha matches and the copy now parses (1 request).
    let (pointer_url, server) = serve(3);

    let mut env = EnvGuard::new();
    env.set("AGENT_WEBMCP_HOME", dir.path().to_str().unwrap());
    env.set("AGENT_WEBMCP_INDEX_URL", &pointer_url);
    env.remove("AGENT_WEBMCP_INDEX");
    for proxy in [
        "HTTP_PROXY",
        "HTTPS_PROXY",
        "ALL_PROXY",
        "http_proxy",
        "https_proxy",
        "all_proxy",
    ] {
        env.remove(proxy);
    }

    // Corrupt file, state already claiming it is current: the stuck state.
    std::fs::write(dir.path().join("index.json"), "garbage-not-json").unwrap();
    std::fs::write(
        dir.path().join("index.state"),
        format!("\n{sha}\n{pointer_url}\n"),
    )
    .unwrap();

    let index = agent_webmcp::store::refresh().unwrap();
    assert_eq!(index.generated_at, "2026-11-18T00:00:00Z");
    let text = std::fs::read_to_string(dir.path().join("index.json")).unwrap();
    agent_webmcp::store::parse(&text).unwrap();

    let (index, changed) = agent_webmcp::store::refresh_changed().unwrap();
    assert!(!changed);
    assert_eq!(index.generated_at, "2026-11-18T00:00:00Z");

    server.join().unwrap();
}

/// A corrupt copy that the state does not cover fails with a recovery hint.
#[test]
fn corrupt_copy_error_names_update() {
    let _lock = ENV_LOCK.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("index.json"), "garbage-not-json").unwrap();

    let mut env = EnvGuard::new();
    env.set("AGENT_WEBMCP_HOME", dir.path().to_str().unwrap());
    env.remove("AGENT_WEBMCP_INDEX");

    let err = agent_webmcp::store::load().unwrap_err().to_string();
    assert!(
        err.ends_with("run `agent-webmcp update` to re-download"),
        "{err}"
    );
}
