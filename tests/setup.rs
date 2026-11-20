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
