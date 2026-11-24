mod common;

use agent_webmcp::setup::{SkillSync, driver_report, newer_available, skill_dirs, sync_skills};

#[test]
fn versions_compare_numerically() {
    assert!(newer_available("0.2.0", "0.2.1"));
    assert!(newer_available("0.2.0", "0.10.0"));
    assert!(!newer_available("0.2.1", "0.2.0"));
    assert!(!newer_available("0.2.0", "0.2.0"));
    assert!(!newer_available("0.2.0", "next"));
}

#[test]
fn versions_follow_semver_prerelease_order() {
    assert!(newer_available("0.3.0-rc.1", "0.3.0"));
    assert!(!newer_available("0.3.0", "0.3.0-rc.1"));
    assert!(newer_available("0.2.9", "0.3.0-rc.1"));
    assert!(newer_available("0.3.0-rc.1", "0.3.0-rc.2"));
    assert!(newer_available("0.3.0-rc.9", "0.3.0-rc.10"));
    assert!(newer_available("0.3.0-alpha", "0.3.0-beta"));
    assert!(!newer_available("0.2.0", "0.2"));
    assert!(!newer_available("0.2.0", "0.2.1.4"));
}

#[test]
fn skill_dirs_only_targets_detected_clients() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    std::fs::create_dir_all(&home).unwrap();

    // Fresh home: only ~/.agents/skills.
    assert_eq!(skill_dirs(&home, None), vec![home.join(".agents/skills")]);

    // Existing ~/.claude adds ~/.claude/skills.
    std::fs::create_dir_all(home.join(".claude")).unwrap();
    assert_eq!(
        skill_dirs(&home, None),
        vec![home.join(".agents/skills"), home.join(".claude/skills")]
    );

    // XDG_CONFIG_HOME overrides ~/.config for OpenCode detection.
    let xdg = dir.path().join("xdg");
    std::fs::create_dir_all(xdg.join("opencode")).unwrap();
    assert_eq!(
        skill_dirs(&home, Some(&xdg)),
        vec![
            home.join(".agents/skills"),
            home.join(".claude/skills"),
            xdg.join("opencode/skills"),
        ]
    );

    // Without XDG_CONFIG_HOME, checks ~/.config/opencode.
    std::fs::create_dir_all(home.join(".config/opencode")).unwrap();
    assert_eq!(
        skill_dirs(&home, None),
        vec![
            home.join(".agents/skills"),
            home.join(".claude/skills"),
            home.join(".config/opencode/skills"),
        ]
    );

    // Codex, OpenClaw, and Cursor join when their homes exist.
    std::fs::create_dir_all(home.join(".codex")).unwrap();
    std::fs::create_dir_all(home.join(".openclaw")).unwrap();
    std::fs::create_dir_all(home.join(".cursor")).unwrap();
    assert_eq!(
        skill_dirs(&home, None),
        vec![
            home.join(".agents/skills"),
            home.join(".claude/skills"),
            home.join(".config/opencode/skills"),
            home.join(".codex/skills"),
            home.join(".openclaw/skills"),
            home.join(".cursor/skills"),
        ]
    );
}

#[test]
fn sync_skills_installs_updates_and_skips_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let skills_root = dir.path().join(".agents/skills");
    let target = skills_root.join("agent-webmcp/SKILL.md");

    // Pass 1: fresh install.
    let first = sync_skills(std::slice::from_ref(&skills_root)).unwrap();
    assert_eq!(
        first,
        SkillSync {
            installed: vec![target.clone()],
            updated: vec![],
        }
    );
    let current = std::fs::read_to_string(&target).unwrap();
    assert!(current.contains("name: agent-webmcp"));

    // Pass 2: unchanged content is skipped.
    let second = sync_skills(std::slice::from_ref(&skills_root)).unwrap();
    assert_eq!(second, SkillSync::default());

    // Pass 3: stale content is updated.
    std::fs::write(&target, "# stale skill\n").unwrap();
    let third = sync_skills(std::slice::from_ref(&skills_root)).unwrap();
    assert_eq!(
        third,
        SkillSync {
            installed: vec![],
            updated: vec![target.clone()],
        }
    );
    assert_eq!(std::fs::read_to_string(&target).unwrap(), current);
}

#[cfg(unix)]
#[test]
fn driver_report_comes_from_detection() {
    let dir = tempfile::tempdir().unwrap();
    let script = common::Fake::new().install(&dir, "ab");
    let mut cfg = common::run_config(&script);
    cfg.drivers.browse.command = dir.path().join("no-such-browse").display().to_string();
    cfg.drivers.lightpanda.command = dir.path().join("no-such-lightpanda").display().to_string();
    let report = driver_report(&cfg).join("\n");
    assert!(report.contains("agent-browser (run): ready"), "{report}");
    assert!(
        report.contains("browse (run): missing; install:"),
        "{report}"
    );
    assert!(
        report.contains("lightpanda (run, experimental): missing; install:"),
        "{report}"
    );
    assert!(report.contains("call will use: agent-browser"), "{report}");
}

#[cfg(unix)]
#[test]
fn driver_report_without_run_driver() {
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = common::allow_all();
    cfg.drivers.agent_browser.command = dir
        .path()
        .join("no-such-agent-browser")
        .display()
        .to_string();
    cfg.drivers.browse.command = dir.path().join("no-such-browse").display().to_string();
    cfg.drivers.lightpanda.command = dir.path().join("no-such-lightpanda").display().to_string();
    let report = driver_report(&cfg).join("\n");
    assert!(
        report.contains("agent-browser (run): missing; install:"),
        "{report}"
    );
    assert!(
        report.contains("lightpanda (run, experimental): missing; install:"),
        "{report}"
    );
    assert!(
        report.contains("call will use: nothing yet (no run driver installed)"),
        "{report}"
    );
}

#[test]
fn home_dir_prefers_home_then_userprofile() {
    let home = std::path::PathBuf::from("/tmp/home-test");
    let env_both = |key: &str| match key {
        "HOME" => Some("/tmp/home-test".to_string()),
        "USERPROFILE" => Some("C:\\Users\\test".to_string()),
        _ => None,
    };
    assert_eq!(
        agent_webmcp::config::home_dir(&env_both),
        Some(home.clone())
    );
    let env_profile_only = |key: &str| match key {
        "HOME" => None,
        "USERPROFILE" => Some("/tmp/profile-home".to_string()),
        _ => None,
    };
    assert_eq!(
        agent_webmcp::config::home_dir(&env_profile_only),
        Some(std::path::PathBuf::from("/tmp/profile-home"))
    );
    // Empty values count as unset.
    let env_empty = |key: &str| match key {
        "HOME" => Some(String::new()),
        "USERPROFILE" => Some(String::new()),
        _ => None,
    };
    assert_eq!(agent_webmcp::config::home_dir(&env_empty), None);
    let _ = home;
}

#[test]
fn sync_skills_continues_past_failing_dir() {
    let dir = tempfile::tempdir().unwrap();
    // A regular file where a directory should be: mkdir/write under it fails.
    let blocker = dir.path().join("blocker");
    std::fs::write(&blocker, b"not a dir").unwrap();
    let failing = blocker.join("skills");
    let writable = dir.path().join("good-skills");

    let err = sync_skills(std::slice::from_ref(&failing)).unwrap_err();
    assert!(!format!("{err:#}").is_empty());

    // First dir fails, second still gets synced, error is still returned.
    let err = sync_skills(&[failing.clone(), writable.clone()]).unwrap_err();
    assert!(
        format!("{err:#}").contains(&failing.join("agent-webmcp").display().to_string())
            || format!("{err:#}").contains("creating")
            || format!("{err:#}").contains("writing"),
        "{err:#}"
    );
    let target = writable.join("agent-webmcp").join("SKILL.md");
    assert!(target.is_file(), "writable dir must still sync");
    assert!(
        std::fs::read_to_string(&target)
            .unwrap()
            .contains("name: agent-webmcp")
    );
}

#[cfg(unix)]
#[test]
fn sync_skills_rejects_symlink_stub_without_overwriting_referent() {
    let dir = tempfile::tempdir().unwrap();
    let skills_root = dir.path().join("skills");
    let target = skills_root.join("agent-webmcp").join("SKILL.md");
    std::fs::create_dir_all(target.parent().unwrap()).unwrap();

    let referent = dir.path().join("referent.md");
    std::fs::write(&referent, "# original referent\n").unwrap();
    std::os::unix::fs::symlink(&referent, &target).unwrap();

    let err = sync_skills(std::slice::from_ref(&skills_root)).unwrap_err();
    let msg = format!("{err:#}");
    assert!(msg.contains("symlink"), "{msg}");
    // The referent must be untouched (no write through the symlink).
    assert_eq!(
        std::fs::read_to_string(&referent).unwrap(),
        "# original referent\n"
    );
    // The symlink itself is preserved.
    assert!(
        std::fs::symlink_metadata(&target)
            .unwrap()
            .file_type()
            .is_symlink()
    );
}

#[cfg(unix)]
#[test]
fn sync_skills_rejects_fifo_without_blocking() {
    use std::io::Read as _;
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    let skills_root = home.join(".agents").join("skills");
    let target = skills_root.join("agent-webmcp").join("SKILL.md");
    std::fs::create_dir_all(target.parent().unwrap()).unwrap();
    assert!(
        std::process::Command::new("mkfifo")
            .arg(&target)
            .status()
            .unwrap()
            .success()
    );

    // Run the sync in a child process with a deadline: if `sync_skills`
    // regresses and opens the FIFO for reading, the child blocks without a
    // writer and the deadline kills and reaps it instead of stalling the
    // suite. A correct sync rejects the FIFO immediately with an error.
    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_agent-webmcp"))
        .args(["skills", "sync"])
        .env_clear()
        .env("HOME", &home)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    let status = loop {
        match child.try_wait().unwrap() {
            Some(status) => break status,
            None if std::time::Instant::now() >= deadline => {
                child.kill().unwrap();
                let _ = child.wait();
                panic!("skills sync blocked on a FIFO for 10s; killing it");
            }
            None => std::thread::sleep(std::time::Duration::from_millis(50)),
        }
    };
    assert!(!status.success(), "FIFO sync must fail");
    let mut err = String::new();
    child
        .stderr
        .take()
        .unwrap()
        .read_to_string(&mut err)
        .unwrap();
    assert!(err.contains("not a regular file"), "{err}");
}

#[test]
fn sync_skills_replaces_atomically_without_temp_leftovers() {
    let dir = tempfile::tempdir().unwrap();
    let skills_root = dir.path().join("skills");
    let target = skills_root.join("agent-webmcp").join("SKILL.md");

    let first = sync_skills(std::slice::from_ref(&skills_root)).unwrap();
    assert_eq!(first.installed, vec![target.clone()]);

    // Stale content is replaced; no temp files remain in the parent.
    std::fs::write(&target, "# stale skill\n").unwrap();
    let third = sync_skills(std::slice::from_ref(&skills_root)).unwrap();
    assert_eq!(third.updated, vec![target.clone()]);
    let content = std::fs::read_to_string(&target).unwrap();
    assert!(content.contains("name: agent-webmcp"), "{content}");
    let parent = target.parent().unwrap();
    let leftovers: Vec<_> = std::fs::read_dir(parent)
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().contains(".tmp."))
        .collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");
}

#[test]
fn sync_skills_bounded_compare_replaces_huge_stub() {
    let dir = tempfile::tempdir().unwrap();
    let skills_root = dir.path().join("skills");
    let target = skills_root.join("agent-webmcp").join("SKILL.md");
    std::fs::create_dir_all(target.parent().unwrap()).unwrap();

    // A huge stale stub (1 MiB) must be detected as stale without unbounded
    // allocation: the comparison reads only `SKILL.len() + 1` bytes.
    let huge = vec![b'x'; 1024 * 1024];
    std::fs::write(&target, &huge).unwrap();
    let synced = sync_skills(std::slice::from_ref(&skills_root)).unwrap();
    assert_eq!(synced.updated, vec![target.clone()]);
    let content = std::fs::read_to_string(&target).unwrap();
    assert!(content.contains("name: agent-webmcp"));

    // A huge file sharing the stub prefix but longer must still differ.
    let prefix = std::fs::read(&target).unwrap();
    let mut longer = prefix.clone();
    longer.push(b'\n');
    longer.extend_from_slice(&vec![b'y'; 64 * 1024]);
    std::fs::write(&target, &longer).unwrap();
    let synced = sync_skills(std::slice::from_ref(&skills_root)).unwrap();
    assert_eq!(synced.updated, vec![target.clone()]);
}

#[test]
fn sync_error_still_allows_driver_report() {
    // Mirrors `setup::run()`: a `sync_skills` error must not prevent the
    // driver report, and the error is still returned for scripts.
    let dir = tempfile::tempdir().unwrap();
    let blocker = dir.path().join("blocker");
    std::fs::write(&blocker, b"not a dir").unwrap();
    let failing = blocker.join("skills");
    let writable = dir.path().join("good-skills");

    let sync_result = sync_skills(&[failing.clone(), writable.clone()]);
    assert!(sync_result.is_err(), "first dir must fail");

    // Drivers are still reported after a sync failure.
    let cfg = agent_webmcp::config::Config::default();
    let report = driver_report(&cfg).join("\n");
    assert!(report.contains("drivers:"), "{report}");
    assert!(report.contains("call will use:"), "{report}");

    // The sync error is preserved for the caller.
    let err = sync_result.unwrap_err();
    assert!(
        format!("{err:#}").contains(&failing.display().to_string())
            || format!("{err:#}").contains("creating")
            || format!("{err:#}").contains("writing")
    );
    // The writable dir still synced despite the earlier failure.
    assert!(writable.join("agent-webmcp/SKILL.md").is_file());
}

#[cfg(unix)]
#[test]
fn sync_skills_failure_names_target_without_unsafe_rename() {
    // Initial installs publish via `hard_link` (no-replace) and never fall
    // back to an overwriting `rename`: a concurrently created symlink must
    // be rejected, never clobbered. Simulate the post-`hard_link`
    // `AlreadyExists` path by pre-creating the target as a symlink — the
    // update path (shared with the concurrent-create fallback via
    // `classify_existing`) must refuse without touching the referent.
    let dir = tempfile::tempdir().unwrap();
    let skills_root = dir.path().join("skills");
    let target = skills_root.join("agent-webmcp").join("SKILL.md");
    std::fs::create_dir_all(target.parent().unwrap()).unwrap();
    let referent = dir.path().join("referent.md");
    std::fs::write(&referent, "# original referent\n").unwrap();
    std::os::unix::fs::symlink(&referent, &target).unwrap();

    let err = sync_skills(std::slice::from_ref(&skills_root)).unwrap_err();
    let msg = format!("{err:#}");
    assert!(msg.contains("symlink"), "{msg}");
    assert!(msg.contains(&target.display().to_string()), "{msg}");
    // The referent is untouched and the symlink is preserved (no rename).
    assert_eq!(
        std::fs::read_to_string(&referent).unwrap(),
        "# original referent\n"
    );
    assert!(
        std::fs::symlink_metadata(&target)
            .unwrap()
            .file_type()
            .is_symlink()
    );
}
