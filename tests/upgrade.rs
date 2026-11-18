use agent_webmcp::upgrade::{Install, detect, normalize_target};
use std::path::{Path, PathBuf};

#[test]
fn detects_unix_npm_global() {
    let exe = Path::new(
        "/usr/local/lib/node_modules/@orkestrate/agent-webmcp/node_modules/@orkestrate/agent-webmcp-linux-x64/bin/agent-webmcp",
    );
    assert_eq!(detect(exe), Install::NpmGlobal(PathBuf::from("/usr/local")));
}

#[test]
fn detects_custom_prefix() {
    let exe = Path::new(
        "/tmp/clean/lib/node_modules/@orkestrate/agent-webmcp-linux-x64/bin/agent-webmcp",
    );
    assert_eq!(detect(exe), Install::NpmGlobal(PathBuf::from("/tmp/clean")));
}

#[test]
fn cargo_build_is_other() {
    let exe = Path::new("/home/me/agent-webmcp/target/release/agent-webmcp");
    assert_eq!(detect(exe), Install::Other);
}

#[test]
fn project_local_install_is_other() {
    let exe =
        Path::new("/home/me/app/node_modules/@orkestrate/agent-webmcp-linux-x64/bin/agent-webmcp");
    assert_eq!(detect(exe), Install::Other);
}

#[test]
fn targets_normalize() {
    assert_eq!(normalize_target(None).unwrap(), "latest");
    assert_eq!(normalize_target(Some("next")).unwrap(), "next");
    assert_eq!(normalize_target(Some("v0.2.1")).unwrap(), "0.2.1");
    assert_eq!(normalize_target(Some("0.3.0-rc.1")).unwrap(), "0.3.0-rc.1");
}

#[test]
fn ranges_refused() {
    assert!(normalize_target(Some("0.2")).is_err());
    assert!(normalize_target(Some("^0.2.0")).is_err());
    assert!(normalize_target(Some("0.2.x")).is_err());
}

#[test]
fn bad_targets_refused() {
    assert!(normalize_target(Some("")).is_err());
    assert!(normalize_target(Some("0.2.1; rm -rf /")).is_err());
    assert!(normalize_target(Some("--registry=evil")).is_err());
    assert!(normalize_target(Some("github:x/y")).is_err());
}
