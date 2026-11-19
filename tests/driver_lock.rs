//! Session lock: contention, stale takeover, names, release. No browser,
//! no env: every test points `acquire` at its own temp dir. Pipeline tests
//! isolate the data dir with the shared [`common::HomeGuard`].

mod common;

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

#[test]
fn unreadable_lock_is_live_until_stale() {
    let dir = sessions();
    let sessions = dir.path().join("sessions");
    std::fs::create_dir_all(&sessions).unwrap();
    let path = sessions.join("test-s.lock");
    // Empty file with a fresh mtime: a racing writer, not a dead holder.
    // A contender must wait, not steal the lock and split the session.
    std::fs::write(&path, "").unwrap();
    let e = lock::acquire(&sessions, "test-s").unwrap_err();
    assert_eq!(e.code, "concurrent_call");
    // Corrupt JSON is the same: unreadable, fresh, still live.
    std::fs::write(&path, "{oops").unwrap();
    let e = lock::acquire(&sessions, "test-s").unwrap_err();
    assert_eq!(e.code, "concurrent_call");
    // Same file with an old mtime: abandoned, take it over.
    let old = std::time::SystemTime::now() - std::time::Duration::from_secs(60);
    std::fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_modified(old)
        .unwrap();
    let _held = lock::acquire(&sessions, "test-s").unwrap();
}

fn lock_now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or(0)
}

// Run-path wiring: AGENT_WEBMCP_HOME is process-wide, so pipeline tests
// isolate it with the shared HomeGuard from `common`.

#[cfg(unix)]
use agent_webmcp::call::{self, Outcome, resolve};
#[cfg(unix)]
use serde_json::json;

#[cfg(unix)]
#[test]
fn contended_session_is_concurrent_call() {
    let home = common::HomeGuard::isolate();
    let scriptdir = tempfile::tempdir().unwrap();
    let script = common::Fake::new()
        .open(common::OPEN_ANNOUNCED)
        .invoke(&[common::INVOKE_OK])
        .install(&scriptdir, "ab");
    let cfg = common::run_config(&script);
    let session = cfg.drivers.agent_browser.session.clone();
    std::fs::create_dir_all(home.path().join("sessions")).unwrap();
    std::fs::write(
        home.path().join("sessions").join(format!("{session}.lock")),
        payload(std::process::id(), lock_now_ms()),
    )
    .unwrap();
    let resolved = resolve("cal.com/ask_cal_com", &common::sample_search()).unwrap();
    let e = call::execute(
        &resolved,
        json!({"question": "hi"}),
        &common::run_opts(),
        &cfg,
    )
    .unwrap_err();
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
    let home = common::HomeGuard::isolate();
    let scriptdir = tempfile::tempdir().unwrap();
    let script = common::Fake::new()
        .open(common::OPEN_ANNOUNCED)
        .invoke(&[common::INVOKE_OK])
        .install(&scriptdir, "ab");
    let cfg = common::run_config(&script);
    let resolved = resolve("cal.com/ask_cal_com", &common::sample_search()).unwrap();
    // A file where the sessions dir belongs: a named session cannot even
    // create its lock dir here.
    std::fs::write(home.path().join("sessions"), "not a dir").unwrap();
    let e = call::execute(
        &resolved,
        json!({"question": "hi"}),
        &common::run_opts(),
        &cfg,
    )
    .unwrap_err();
    assert_eq!(e.code, "driver_error");
    // A fresh session never touches the lock, so it runs fine.
    let mut opts = common::run_opts();
    opts.fresh = true;
    match call::execute(&resolved, json!({"question": "hi"}), &opts, &cfg) {
        Ok(Outcome::Ran { .. }) => {}
        other => panic!("fresh should skip the lock, got {other:?}"),
    }
}
