//! Session lock: contention, stale takeover, names, release. No browser,
//! no env: every test points `acquire` at its own temp dir.

use agent_webmcp::driver::lock;

fn sessions() -> tempfile::TempDir {
    tempfile::tempdir().unwrap()
}

fn payload(pid: u32, ts: u64) -> String {
    format!("{{\"pid\":{pid},\"ts\":{ts}}}")
}

#[test]
fn acquire_and_release_allows_reacquire() {
    let dir = sessions();
    let sessions = dir.path().join("sessions");
    let held = lock::acquire(&sessions, "test-s").unwrap();
    drop(held);
    let _again = lock::acquire(&sessions, "test-s").unwrap();
    drop(_again);
    assert!(!sessions.join("test-s.lock").exists());
}

#[test]
fn live_holder_contends_with_pid_and_age() {
    let dir = sessions();
    let sessions = dir.path().join("sessions");
    let _held = lock::acquire(&sessions, "test-s").unwrap();
    let e = lock::acquire(&sessions, "test-s").unwrap_err();
    assert_eq!(e.code, "concurrent_call");
    assert!(
        e.message.contains(&std::process::id().to_string()),
        "{}",
        e.message
    );
    assert!(e.message.contains("test-s"), "{}", e.message);
    assert!(e.data.as_ref().unwrap().get("ageMs").is_some());
}

#[test]
fn live_holder_contends_whatever_the_age() {
    let dir = sessions();
    let sessions = dir.path().join("sessions");
    std::fs::create_dir_all(&sessions).unwrap();
    // Our own pid is live and the stamp is 60 s old: still contention.
    let old = lock_now_ms().saturating_sub(60_000);
    std::fs::write(
        sessions.join("test-s.lock"),
        payload(std::process::id(), old),
    )
    .unwrap();
    let held = lock::acquire(&sessions, "test-s");
    let e = held.unwrap_err();
    assert_eq!(e.code, "concurrent_call");
    assert!(
        e.message.contains(&std::process::id().to_string()),
        "{}",
        e.message
    );
}

#[test]
fn dead_holder_is_taken_over() {
    let dir = sessions();
    let sessions = dir.path().join("sessions");
    std::fs::create_dir_all(&sessions).unwrap();
    // Pid 2^31 - 1 cannot exist here, so the lock is taken over.
    std::fs::write(
        sessions.join("test-s.lock"),
        payload(i32::MAX as u32, lock_now_ms()),
    )
    .unwrap();
    let _held = lock::acquire(&sessions, "test-s").unwrap();
}

#[test]
fn hostile_names_are_usage() {
    let dir = sessions();
    let sessions = dir.path().join("sessions");
    for name in ["", "../escape", "a/b", "with space", "semi;colon"] {
        let e = lock::acquire(&sessions, name).unwrap_err();
        assert_eq!(e.code, "usage", "{name}");
    }
    assert!(!sessions.exists(), "no dir for rejected names");
}

fn lock_now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or(0)
}

// Run-path wiring: env is process-wide, so these serialize on a mutex
// (this binary never runs other env-dependent tests concurrently).

#[cfg(unix)]
static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(unix)]
struct EnvGuard {
    saved: Vec<(String, Option<std::ffi::OsString>)>,
}

#[cfg(unix)]
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
}

#[cfg(unix)]
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

#[cfg(unix)]
use agent_webmcp::call::{self, CallOptions, Outcome, resolve};
#[cfg(unix)]
use agent_webmcp::model::{Annotations, Index, Site, Status, Tool};
#[cfg(unix)]
use serde_json::json;

#[cfg(unix)]
fn ask_tool() -> Tool {
    Tool {
        name: "ask_cal_com".into(),
        description: "Ask about Cal.".into(),
        input_schema: json!({"type": "object", "required": ["question"], "properties": {"question": {"type": "string"}}}),
        page: "https://cal.com/".into(),
        origin: "https://cal.com".into(),
        annotations: Annotations {
            consequential: true,
            ..Default::default()
        },
        flags: vec![],
    }
}

#[cfg(unix)]
fn search() -> agent_webmcp::index::Search {
    agent_webmcp::index::Search::build(Index {
        schema_version: 1,
        generated_at: "2026-11-19T00:00:00Z".into(),
        sites: vec![Site {
            host: "cal.com".into(),
            status: Status::Verified,
            checked_at: "2026-11-19T00:00:00Z".into(),
            pages_checked: vec!["https://cal.com/".into()],
            platform: None,
            tools: vec![ask_tool()],
            note: None,
            missed: 0,
        }],
    })
    .unwrap()
}

#[cfg(unix)]
fn allow_all() -> agent_webmcp::config::Config {
    let mut cfg = agent_webmcp::config::Config::default();
    cfg.permissions.allow.push("*".into());
    cfg
}

#[cfg(unix)]
fn run_opts() -> CallOptions {
    CallOptions {
        confirm: true,
        plan_only: false,
        fresh: false,
        retry_hint: call::RETRY_CLI,
    }
}

#[cfg(unix)]
const OPEN_OK: &str =
    r#"{"success":true,"data":{"url":"https://cal.com/","lifecycle":{"reused":true}}}"#;
#[cfg(unix)]
const INVOKE_OK: &str = r#"{"success":true,"data":{"output":{"answer":1},"durationMs":5}}"#;

#[cfg(unix)]
fn run_config(script: &std::path::Path) -> agent_webmcp::config::Config {
    let mut cfg = allow_all();
    cfg.drivers.agent_browser.command = script.display().to_string();
    static SESSION_N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = SESSION_N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    cfg.drivers.agent_browser.session = format!("lock-s-{n}");
    cfg.call.timeout = 1234;
    cfg.call.settle_ms = 0;
    cfg
}

#[cfg(unix)]
fn fake_script(
    dir: &tempfile::TempDir,
    name: &str,
    invoke: &str,
    open: &str,
) -> std::path::PathBuf {
    let log = dir.path().join(format!("{name}.argv"));
    let path = dir.path().join(name);
    let script = format!(
        "#!/bin/sh\necho \"$@\" >> \"{log}\"\ncase \" $* \" in\n  *\" --version \"*) echo \"agent-browser 0.43.1\";;\n  *\" close \"*) echo '{{\"success\":true,\"data\":{{}}}}';;\n  *\" invoke \"*) echo '{invoke}';;\n  *\" open \"*) echo '{open}';;\nesac\n",
        log = log.display(),
    );
    std::fs::write(&path, script).unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

#[cfg(unix)]
fn with_home(
    dir: &tempfile::TempDir,
    file: bool,
) -> (tempfile::TempDir, EnvGuard, std::path::PathBuf) {
    let home = dir.path().join("home");
    let mut env = EnvGuard::new();
    if file {
        std::fs::write(&home, "not a dir").unwrap();
    } else {
        std::fs::create_dir_all(home.join("sessions")).unwrap();
    }
    env.set("AGENT_WEBMCP_HOME", home.to_str().unwrap());
    let scriptdir = tempfile::tempdir().unwrap();
    let home_path = home.clone();
    (scriptdir, env, home_path)
}

#[cfg(unix)]
#[test]
fn contended_session_is_concurrent_call() {
    let _guard = ENV_LOCK.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let (scriptdir, _env, home) = with_home(&dir, false);
    let script = fake_script(&scriptdir, "ab", INVOKE_OK, OPEN_OK);
    let cfg = run_config(&script);
    let session = cfg.drivers.agent_browser.session.clone();
    std::fs::write(
        home.join("sessions").join(format!("{session}.lock")),
        payload(std::process::id(), lock_now_ms()),
    )
    .unwrap();
    let resolved = resolve("cal.com/ask_cal_com", &search()).unwrap();
    let e = call::execute(&resolved, json!({"question": "hi"}), &run_opts(), &cfg).unwrap_err();
    assert_eq!(e.code, "concurrent_call");
    assert!(
        e.message.contains(&std::process::id().to_string()),
        "{}",
        e.message
    );
    assert_eq!(e.data.as_ref().unwrap()["session"], json!(session));
}

#[cfg(unix)]
#[test]
fn fresh_skips_the_session_lock() {
    let _guard = ENV_LOCK.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let (scriptdir, _env, _home) = with_home(&dir, true);
    let script = fake_script(&scriptdir, "ab", INVOKE_OK, OPEN_OK);
    let cfg = run_config(&script);
    let resolved = resolve("cal.com/ask_cal_com", &search()).unwrap();
    // A named session cannot even create its lock dir here.
    let e = call::execute(&resolved, json!({"question": "hi"}), &run_opts(), &cfg).unwrap_err();
    assert_eq!(e.code, "driver_error");
    // A fresh session never touches the lock, so it runs fine.
    let mut opts = run_opts();
    opts.fresh = true;
    match call::execute(&resolved, json!({"question": "hi"}), &opts, &cfg) {
        Ok(Outcome::Ran { .. }) => {}
        other => panic!("fresh should skip the lock, got {other:?}"),
    }
}
