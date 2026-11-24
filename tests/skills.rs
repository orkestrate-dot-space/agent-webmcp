use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_agent-webmcp"))
}

fn base_cmd(home: &Path) -> Command {
    let mut cmd = Command::new(bin());
    cmd.env_clear()
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", "")
        .env("PATH", std::env::var("PATH").unwrap_or_default());
    cmd
}

fn cmd_with_skills_dir(home: &Path, skills_dir: &Path) -> Command {
    let mut cmd = base_cmd(home);
    cmd.env("AGENT_WEBMCP_SKILLS_DIR", skills_dir);
    cmd
}

fn stdout_json(out: &std::process::Output) -> Value {
    serde_json::from_slice(&out.stdout).unwrap_or_else(|e| {
        panic!(
            "not JSON: {e}; stdout={:?}",
            String::from_utf8_lossy(&out.stdout)
        )
    })
}

/// Speak JSON-RPC over stdio to a child `agent-webmcp mcp` server and
/// return its `resources/list` URIs. Used to prove the production server
/// honors `AGENT_WEBMCP_SKILLS_DIR` exactly like the CLI (same env, same
/// listing) without any in-process env mutation.
#[cfg(unix)]
fn mcp_resources_via_stdio(home: &Path, skills_dir: &Path, index: &Path) -> Vec<String> {
    use std::io::{BufRead, BufReader, Write};
    use std::process::Stdio;
    use std::time::Duration;
    let mut child = Command::new(bin());
    child
        .args(["mcp"])
        .env_clear()
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", "")
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("AGENT_WEBMCP_SKILLS_DIR", skills_dir)
        .env("AGENT_WEBMCP_INDEX", index)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = child.spawn().expect("spawn mcp server");
    let mut stdin = child.stdin.take().expect("child stdin");
    let stdout = child.stdout.take().expect("child stdout");
    let stderr = child.stderr.take().expect("child stderr");
    let (tx, rx) = std::sync::mpsc::channel::<String>();
    let (etx, erx) = std::sync::mpsc::channel::<String>();
    std::thread::spawn(move || {
        let reader = BufReader::new(stdout);
        for line in reader.lines() {
            match line {
                Ok(l) if !l.trim().is_empty() => {
                    if tx.send(l).is_err() {
                        break;
                    }
                }
                Ok(_) => {}
                Err(_) => break,
            }
        }
    });
    std::thread::spawn(move || {
        let reader = BufReader::new(stderr);
        let mut logged = String::new();
        for line in reader.lines().map_while(Result::ok) {
            logged.push_str(&line);
            logged.push('\n');
        }
        let _ = etx.send(logged);
    });
    let mut send = |v: Value| {
        let mut s = serde_json::to_string(&v).unwrap();
        s.push('\n');
        stdin.write_all(s.as_bytes()).expect("write to mcp stdin");
        stdin.flush().expect("flush mcp stdin");
    };
    let mut recv = |what: &str| -> Value {
        let line = rx
            .recv_timeout(Duration::from_secs(30))
            .unwrap_or_else(|_| {
                let logged = erx.try_recv().unwrap_or_default();
                let _ = child.kill();
                let _ = child.wait();
                panic!("timed out waiting for mcp {what}; child stderr: {logged}")
            });
        serde_json::from_str(&line).unwrap_or_else(|e| panic!("mcp {what} not JSON: {e}: {line}"))
    };
    send(serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2026-07-28",
            "capabilities": {},
            "clientInfo": {"name": "skills-env-test", "version": "0.0.0"},
        },
    }));
    let init = recv("initialize response");
    assert_eq!(init["id"], serde_json::json!(1), "{init}");
    send(serde_json::json!({"jsonrpc": "2.0", "method": "notifications/initialized"}));
    send(serde_json::json!({"jsonrpc": "2.0", "id": 2, "method": "resources/list", "params": {}}));
    let listed = recv("resources/list response");
    assert_eq!(listed["id"], serde_json::json!(2), "{listed}");
    let uris: Vec<String> = listed["result"]["resources"]
        .as_array()
        .unwrap_or_else(|| panic!("no resources array: {listed}"))
        .iter()
        .filter_map(|r| r["uri"].as_str().map(str::to_string))
        .collect();
    let _ = child.kill();
    let _ = child.wait();
    uris
}

/// Run `cmd` with a timeout, retaining the `Child` handle so a hung child is
/// `kill()`ed + `wait()`ed on timeout instead of leaking as a detached
/// process. Captures piped stdout/stderr like `Command::output`.
fn output_with_timeout(mut cmd: Command, timeout: std::time::Duration) -> std::process::Output {
    use std::process::Stdio;
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd.spawn().expect("spawn test child");
    let start = std::time::Instant::now();
    loop {
        match child.try_wait().expect("try_wait") {
            Some(_) => return child.wait_with_output().expect("wait output"),
            None => {
                if start.elapsed() > timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    panic!("test child hung and was killed after {timeout:?}");
                }
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
        }
    }
}

#[test]
fn skills_lists_bundled_core() {
    let home = tempfile::tempdir().unwrap();
    let out = base_cmd(home.path()).args(["skills"]).output().unwrap();
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("core (bundled)"), "{text}");
}

#[test]
fn skills_list_json_envelope() {
    let home = tempfile::tempdir().unwrap();
    let out = base_cmd(home.path())
        .args(["skills", "list", "--json"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    let value = stdout_json(&out);
    let names: Vec<&str> = value["skills"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["name"].as_str().unwrap())
        .collect();
    assert!(names.contains(&"core"), "{value}");
    let core = value["skills"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["name"] == "core")
        .unwrap();
    assert_eq!(core["source"], Value::String("bundled".into()));
}

#[test]
fn skills_get_core_contains_workflow() {
    let home = tempfile::tempdir().unwrap();
    let out = base_cmd(home.path())
        .args(["skills", "get", "core"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("agent-webmcp find"), "{text}");
    assert!(text.contains("agent-webmcp inspect"), "{text}");
    assert!(text.contains("agent-webmcp call"), "{text}");
    assert!(text.contains("--ref drivers"), "{text}");
}

#[test]
fn skills_get_ref_by_stem_filename_and_path() {
    let home = tempfile::tempdir().unwrap();
    for query in ["drivers", "drivers.md", "references/drivers.md"] {
        let out = base_cmd(home.path())
            .args(["skills", "get", "core", "--ref", query])
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(0), "{query} {out:?}");
        let text = String::from_utf8_lossy(&out.stdout);
        assert!(text.contains("Driver matrix"), "{query} {text}");
    }
    for query in ["permissions", "config", "authoring"] {
        let out = base_cmd(home.path())
            .args(["skills", "get", "core", "--ref", query])
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(0), "{out:?}");
    }
}

#[test]
fn skills_get_ref_authoring() {
    let home = tempfile::tempdir().unwrap();
    for query in ["authoring", "authoring.md", "references/authoring.md"] {
        let out = base_cmd(home.path())
            .args(["skills", "get", "core", "--ref", query])
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(0), "{query} {out:?}");
        let text = String::from_utf8_lossy(&out.stdout);
        assert!(text.contains("__webmcpCustom"), "{query} {text}");
        assert!(text.contains("tools validate"), "{query} {text}");
    }
}

#[test]
fn skills_get_unknown_ref_exits_two() {
    let home = tempfile::tempdir().unwrap();
    let out = base_cmd(home.path())
        .args(["skills", "get", "core", "--ref", "nope"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2), "{out:?}");
}

#[test]
fn skills_get_unknown_skill_exits_two() {
    let home = tempfile::tempdir().unwrap();
    let out = base_cmd(home.path())
        .args(["skills", "get", "does-not-exist"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2), "{out:?}");
}

#[test]
fn skills_get_requires_names_or_all() {
    let home = tempfile::tempdir().unwrap();
    let out = base_cmd(home.path())
        .args(["skills", "get"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2), "{out:?}");
    let out = base_cmd(home.path())
        .args(["skills", "get", "core", "--all"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2), "{out:?}");
}

#[test]
fn skills_get_full_inlines_refs() {
    let home = tempfile::tempdir().unwrap();
    let out = base_cmd(home.path())
        .args(["skills", "get", "core", "--full"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("agent-webmcp find"), "{text}");
    assert!(text.contains("Driver matrix"), "{text}");
    assert!(text.contains("Allow, ask, deny"), "{text}");
    assert!(text.contains("Files and precedence"), "{text}");
    assert!(text.contains("__webmcpCustom"), "{text}");
}

#[test]
fn skills_get_all_lists_core() {
    let home = tempfile::tempdir().unwrap();
    let out = base_cmd(home.path())
        .args(["skills", "get", "--all"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    let out_json = base_cmd(home.path())
        .args(["skills", "get", "--all", "--json"])
        .output()
        .unwrap();
    assert_eq!(out_json.status.code(), Some(0), "{out_json:?}");
    let value = stdout_json(&out_json);
    assert!(
        value["skills"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s["name"] == "core")
    );
}

#[test]
fn skills_path_prints_dirs() {
    let home = tempfile::tempdir().unwrap();
    let skills_dir = home.path().join("custom-skills");
    std::fs::create_dir_all(&skills_dir).unwrap();

    // No arg: local skills dir.
    let out = cmd_with_skills_dir(home.path(), &skills_dir)
        .args(["skills", "path"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.trim().ends_with("custom-skills"), "{text}");

    let out_json = cmd_with_skills_dir(home.path(), &skills_dir)
        .args(["skills", "path", "--json"])
        .output()
        .unwrap();
    let value = stdout_json(&out_json);
    assert!(value["path"].as_str().unwrap().ends_with("custom-skills"));

    // Bundled: prints bundled + stubs.
    let out = base_cmd(home.path())
        .args(["skills", "path", "core"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.lines().next().unwrap().contains("bundled"), "{text}");
}

#[test]
fn skills_sync_installs_stub() {
    let home = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(home.path().join(".agents")).unwrap();
    std::fs::create_dir_all(home.path().join(".claude")).unwrap();

    let out = base_cmd(home.path())
        .args(["skills", "sync", "--json"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    let value = stdout_json(&out);
    assert!(value["installed"].is_array(), "{value}");
    assert!(value["updated"].is_array(), "{value}");
    assert!(value["unchanged"].is_array(), "{value}");

    let stub = home.path().join(".agents/skills/agent-webmcp/SKILL.md");
    let content = std::fs::read_to_string(&stub).unwrap();
    assert!(content.contains("skills get core"), "{content}");

    // Second run is a no-op.
    let out = base_cmd(home.path())
        .args(["skills", "sync"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("already up to date"), "{text}");
}

#[test]
fn local_overlay_lists_and_gets() {
    let home = tempfile::tempdir().unwrap();
    let skills_dir = home.path().join("skills");
    let skill_root = skills_dir.join("mydemo");
    std::fs::create_dir_all(skill_root.join("references")).unwrap();
    std::fs::write(
        skill_root.join("SKILL.md"),
        "---\nname: mydemo\ndescription: Demo skill for tests.\n---\n\n# mydemo\n",
    )
    .unwrap();
    std::fs::write(skill_root.join("references/foo.md"), "# foo reference\n").unwrap();

    let out = cmd_with_skills_dir(home.path(), &skills_dir)
        .args(["skills", "list"])
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("mydemo (local)"), "{text}");
    assert!(text.contains("core (bundled)"), "{text}");

    let out = cmd_with_skills_dir(home.path(), &skills_dir)
        .args(["skills", "get", "mydemo"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    assert!(String::from_utf8_lossy(&out.stdout).contains("# mydemo"));

    let out = cmd_with_skills_dir(home.path(), &skills_dir)
        .args(["skills", "get", "mydemo", "--ref", "foo"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    assert!(String::from_utf8_lossy(&out.stdout).contains("foo reference"));
}

#[test]
fn default_dir_cannot_shadow_core_but_explicit_can() {
    let home = tempfile::tempdir().unwrap();
    // Default location: <HOME>/.config/agent-webmcp/skills/core
    let default_skills = home.path().join(".config/agent-webmcp/skills");
    let default_core = default_skills.join("core");
    std::fs::create_dir_all(&default_core).unwrap();
    std::fs::write(
        default_core.join("SKILL.md"),
        "---\nname: core\ndescription: Local shadow.\n---\n\n# shadow\n",
    )
    .unwrap();

    // Without override: bundled wins.
    let out = base_cmd(home.path())
        .args(["skills", "get", "core"])
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(!text.contains("# shadow"), "{text}");
    assert!(text.contains("agent-webmcp core guide"), "{text}");

    // With explicit override: local wins.
    let out = cmd_with_skills_dir(home.path(), &default_skills)
        .args(["skills", "get", "core"])
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("# shadow"), "{text}");
}

#[test]
fn traversal_and_symlink_escape_rejected() {
    let home = tempfile::tempdir().unwrap();
    let skills_dir = home.path().join("skills");
    std::fs::create_dir_all(&skills_dir).unwrap();

    for bad in ["../evil", "..", "a/b", ""] {
        if bad.is_empty() {
            continue;
        }
        let out = cmd_with_skills_dir(home.path(), &skills_dir)
            .args(["skills", "get", bad])
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(2), "{bad} {out:?}");
    }
    let out = cmd_with_skills_dir(home.path(), &skills_dir)
        .args(["skills", "get", "core", "--ref", "../../etc/passwd"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2), "{out:?}");

    // Symlinked skill dir escaping the root is not listed and not readable.
    #[cfg(unix)]
    {
        let outside = home.path().join("outside");
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(
            outside.join("SKILL.md"),
            "---\nname: evil\ndescription: Evil.\n---\n\n# evil\n",
        )
        .unwrap();
        std::os::unix::fs::symlink(&outside, skills_dir.join("evil")).unwrap();
        let out = cmd_with_skills_dir(home.path(), &skills_dir)
            .args(["skills", "list"])
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&out.stdout);
        assert!(!text.contains("evil ("), "{text}");
        let out = cmd_with_skills_dir(home.path(), &skills_dir)
            .args(["skills", "get", "evil"])
            .output()
            .unwrap();
        // Escaping the root is a real I/O error (exit 1), not "unknown skill".
        assert_eq!(out.status.code(), Some(1), "{out:?}");
    }
}

#[test]
fn version_and_help_flags() {
    let home = tempfile::tempdir().unwrap();
    let version = env!("CARGO_PKG_VERSION");
    for args in [["-v"], ["-V"], ["--version"]] {
        let out = base_cmd(home.path()).args(args).output().unwrap();
        assert_eq!(out.status.code(), Some(0), "{args:?} {out:?}");
        let text = String::from_utf8_lossy(&out.stdout);
        assert!(text.contains(version), "{args:?} {text}");
    }
    for args in [["-h"], ["--help"]] {
        let out = base_cmd(home.path()).args(args).output().unwrap();
        assert_eq!(out.status.code(), Some(0), "{args:?} {out:?}");
        let text = String::from_utf8_lossy(&out.stdout);
        assert!(text.contains("skills get core"), "{text}");
        assert!(text.contains("AGENT_WEBMCP_SKILLS_DIR"), "{text}");
    }
    let out = base_cmd(home.path()).args(["-h"]).output().unwrap();
    let lines = String::from_utf8_lossy(&out.stdout).lines().count();
    assert!((35..=45).contains(&lines), "help is {lines} lines");
}

// --- MCP resource mirror ---

fn ask_index() -> agent_webmcp::model::Index {
    agent_webmcp::model::Index {
        schema_version: 1,
        generated_at: "2026-11-19T00:00:00Z".into(),
        sites: vec![],
    }
}

fn test_server() -> agent_webmcp::mcp::Server {
    let search = agent_webmcp::index::Search::build(ask_index()).unwrap();
    agent_webmcp::mcp::Server::new(search)
}

#[tokio::test(flavor = "current_thread")]
async fn mcp_lists_three_tools_and_skill_resources() -> anyhow::Result<()> {
    use rmcp::model::{Implementation, InitializeRequestParams, ProtocolVersion};
    let server = test_server();
    let client_info = InitializeRequestParams::new(
        rmcp::model::ClientCapabilities::builder().build(),
        Implementation::new("skills-test", "0.0.0"),
    )
    .with_protocol_version(ProtocolVersion::V_2026_07_28);
    let mut server_info = rmcp::model::InitializeResult::new(
        rmcp::model::ServerCapabilities::builder()
            .enable_tools()
            .build(),
    );
    server_info.protocol_version = ProtocolVersion::V_2026_07_28;

    tokio::task::LocalSet::new()
        .run_until(async move {
            let (server_transport, client_transport) = tokio::io::duplex(8192);
            let server_task = tokio::task::spawn_local(async move {
                let running = rmcp::service::serve_directly::<
                    rmcp::service::RoleServer,
                    _,
                    _,
                    _,
                    _,
                >(server, server_transport, Some(client_info));
                running.waiting().await?;
                anyhow::Ok(())
            });
            let client = rmcp::service::serve_directly::<
                rmcp::service::RoleClient,
                _,
                _,
                _,
                _,
            >(
                (),
                client_transport,
                Some(server_info.into()),
            );
            let tools = client.list_tools(None).await?;
            assert_eq!(tools.tools.len(), 3, "{tools:?}");
            let names: Vec<&str> = tools.tools.iter().map(|t| t.name.as_ref()).collect();
            assert!(names.contains(&"find"));
            assert!(names.contains(&"inspect"));
            assert!(names.contains(&"call"));

            let resources = client.list_resources(None).await?;
            let uris: Vec<&str> = resources.resources.iter().map(|r| r.uri.as_str()).collect();
            assert!(
                uris.contains(&"skill://agent-webmcp/core"),
                "{uris:?}"
            );
            assert!(
                uris.contains(&"skill://agent-webmcp/core/references/drivers.md"),
                "{uris:?}"
            );

            let skill = client
                .read_resource(rmcp::model::ReadResourceRequestParams::new(
                    "skill://agent-webmcp/core",
                ))
                .await?;
            let text = match &skill.contents[0] {
                rmcp::model::ResourceContents::TextResourceContents { text, .. } => text.clone(),
                other => panic!("expected text, got {other:?}"),
            };
            assert!(text.contains("agent-webmcp find"), "{text}");

            let drivers = client
                .read_resource(rmcp::model::ReadResourceRequestParams::new(
                    "skill://agent-webmcp/core/drivers",
                ))
                .await?;
            let text = match &drivers.contents[0] {
                rmcp::model::ResourceContents::TextResourceContents { text, .. } => text.clone(),
                other => panic!("expected text, got {other:?}"),
            };
            assert!(text.contains("Driver matrix"), "{text}");

            server_task.abort();
            Ok(())
        })
        .await
}

#[tokio::test(flavor = "current_thread")]
async fn mcp_unknown_skill_resource_is_error() -> anyhow::Result<()> {
    use rmcp::model::{Implementation, InitializeRequestParams, ProtocolVersion};
    let server = test_server();
    let client_info = InitializeRequestParams::new(
        rmcp::model::ClientCapabilities::builder().build(),
        Implementation::new("skills-test", "0.0.0"),
    )
    .with_protocol_version(ProtocolVersion::V_2026_07_28);
    let mut server_info = rmcp::model::InitializeResult::new(
        rmcp::model::ServerCapabilities::builder()
            .enable_tools()
            .build(),
    );
    server_info.protocol_version = ProtocolVersion::V_2026_07_28;

    tokio::task::LocalSet::new()
        .run_until(async move {
            let (server_transport, client_transport) = tokio::io::duplex(8192);
            let server_task = tokio::task::spawn_local(async move {
                let running = rmcp::service::serve_directly::<
                    rmcp::service::RoleServer,
                    _,
                    _,
                    _,
                    _,
                >(server, server_transport, Some(client_info));
                running.waiting().await?;
                anyhow::Ok(())
            });
            let client = rmcp::service::serve_directly::<
                rmcp::service::RoleClient,
                _,
                _,
                _,
                _,
            >(
                (),
                client_transport,
                Some(server_info.into()),
            );
            let err = client
                .read_resource(rmcp::model::ReadResourceRequestParams::new(
                    "skill://agent-webmcp/does-not-exist",
                ))
                .await
                .unwrap_err();
            let msg = format!("{err:?}");
            assert!(msg.contains("unknown") || msg.contains("Invalid"), "{msg}");
            server_task.abort();
            Ok(())
        })
        .await
}

#[test]
fn invalid_utf8_skill_reports_io_exit_one() {
    let home = tempfile::tempdir().unwrap();
    let skills_dir = home.path().join("skills");
    let skill_root = skills_dir.join("badutf");
    std::fs::create_dir_all(&skill_root).unwrap();
    std::fs::write(skill_root.join("SKILL.md"), [0xff, 0xfe, 0x00, b'a']).unwrap();

    // `skills get` must surface the real I/O error (exit 1), not "unknown skill" (exit 2).
    let out = cmd_with_skills_dir(home.path(), &skills_dir)
        .args(["skills", "get", "badutf"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1), "{out:?}");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!stderr.contains("unknown skill"), "{stderr}");

    // `skills path` must do the same instead of falling through to exit 2.
    let out = cmd_with_skills_dir(home.path(), &skills_dir)
        .args(["skills", "path", "badutf"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1), "{out:?}");
}

#[test]
fn template_stem_matches_non_md_extension() {
    let home = tempfile::tempdir().unwrap();
    let skills_dir = home.path().join("skills");
    let skill_root = skills_dir.join("templated");
    std::fs::create_dir_all(skill_root.join("templates")).unwrap();
    std::fs::write(
        skill_root.join("SKILL.md"),
        "---\nname: templated\ndescription: Demo.\n---\n\n# templated\n",
    )
    .unwrap();
    std::fs::write(
        skill_root.join("templates/scaffold.json"),
        "{\"hello\": 1}\n",
    )
    .unwrap();
    std::fs::write(skill_root.join("templates/run.sh"), "#!/bin/sh\n").unwrap();

    for query in ["scaffold", "scaffold.json", "templates/scaffold.json"] {
        let out = cmd_with_skills_dir(home.path(), &skills_dir)
            .args(["skills", "get", "templated", "--ref", query])
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(0), "{query} {out:?}");
        assert!(
            String::from_utf8_lossy(&out.stdout).contains("hello"),
            "{query}"
        );
    }
    let out = cmd_with_skills_dir(home.path(), &skills_dir)
        .args(["skills", "get", "templated", "--ref", "run"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    assert!(String::from_utf8_lossy(&out.stdout).contains("#!/bin/sh"));
}

#[cfg(unix)]
#[test]
fn fifo_ref_is_skipped_not_hung() {
    use std::time::Duration;
    let home = tempfile::tempdir().unwrap();
    let skills_dir = home.path().join("skills");
    let skill_root = skills_dir.join("fifodemo");
    std::fs::create_dir_all(skill_root.join("references")).unwrap();
    std::fs::write(
        skill_root.join("SKILL.md"),
        "---\nname: fifodemo\ndescription: Demo.\n---\n\n# fifodemo\n",
    )
    .unwrap();
    std::fs::write(skill_root.join("references/ok.md"), "# ok\n").unwrap();
    let fifo = skill_root.join("references/fifo.md");
    assert!(
        std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap()
            .success()
    );
    // Listing the skill must not block on the FIFO; run with a timeout that
    // retains the `Child` and kills it on timeout so no process leaks.
    let out = output_with_timeout(
        {
            let mut c = cmd_with_skills_dir(home.path(), &skills_dir);
            c.args(["skills", "get", "fifodemo", "--ref", "ok"]);
            c
        },
        Duration::from_secs(20),
    );
    assert_eq!(out.status.code(), Some(0), "{out:?}");
}

#[test]
fn userprofile_fallback_resolves_home() {
    let home = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(home.path().join(".agents")).unwrap();

    // HOME empty + USERPROFILE set: skills sync must use USERPROFILE.
    let mut cmd = Command::new(bin());
    cmd.env_clear()
        .env("HOME", "")
        .env("USERPROFILE", home.path())
        .env("XDG_CONFIG_HOME", "")
        .env("PATH", std::env::var("PATH").unwrap_or_default());
    let out = cmd.args(["skills", "sync"]).output().unwrap();
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    assert!(
        home.path()
            .join(".agents/skills/agent-webmcp/SKILL.md")
            .is_file(),
        "{out:?}"
    );

    // Same fallback for `skills path core` (bundled + stubs, no crash).
    let mut cmd = Command::new(bin());
    cmd.env_clear()
        .env("HOME", "")
        .env("USERPROFILE", home.path())
        .env("XDG_CONFIG_HOME", "")
        .env("PATH", std::env::var("PATH").unwrap_or_default());
    let out = cmd.args(["skills", "path", "core"]).output().unwrap();
    assert_eq!(out.status.code(), Some(0), "{out:?}");
}

#[test]
fn skills_full_ref_combination_is_rejected() {
    let home = tempfile::tempdir().unwrap();
    let out = base_cmd(home.path())
        .args(["skills", "get", "core", "--full", "--ref", "drivers"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2), "{out:?}");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("--full") && stderr.contains("--ref"),
        "{stderr}"
    );
}

#[test]
fn skills_parent_json_get_core_outputs_json() {
    let home = tempfile::tempdir().unwrap();
    let out = base_cmd(home.path())
        .args(["skills", "--json", "get", "core"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    let value = stdout_json(&out);
    let skills = value["skills"].as_array().unwrap();
    assert_eq!(skills.len(), 1);
    assert_eq!(skills[0]["name"], Value::String("core".into()));
    assert!(
        skills[0]["content"]
            .as_str()
            .unwrap()
            .contains("agent-webmcp find")
    );
}

#[test]
fn skills_temp_dir_refusal_when_home_and_xdg_unset() {
    // No HOME/USERPROFILE/XDG and no explicit dir: `skills path` (no arg)
    // must refuse (exit 1) instead of returning a shared temp dir.
    let mut cmd = Command::new(bin());
    cmd.env_clear()
        .env("HOME", "")
        .env("USERPROFILE", "")
        .env("XDG_CONFIG_HOME", "")
        .env("AGENT_WEBMCP_SKILLS_DIR", "")
        .env("PATH", std::env::var("PATH").unwrap_or_default());
    let out = cmd.args(["skills", "path"]).output().unwrap();
    assert_eq!(out.status.code(), Some(1), "{out:?}");

    // `skills list` still works but only advertises bundled (never /tmp).
    let mut cmd = Command::new(bin());
    cmd.env_clear()
        .env("HOME", "")
        .env("USERPROFILE", "")
        .env("XDG_CONFIG_HOME", "")
        .env("AGENT_WEBMCP_SKILLS_DIR", "")
        .env("PATH", std::env::var("PATH").unwrap_or_default());
    let out = cmd.args(["skills", "list"]).output().unwrap();
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("core (bundled)"), "{text}");

    // Direct lib check: no temp fallback.
    let err = agent_webmcp::skills::skills_dir().map(|_| ());
    // In the test process HOME is set, so this succeeds; the refusal is
    // covered by `skills_dir_from` unit tests plus the CLI subprocess above.
    let _ = err;
}

#[test]
fn skills_lazy_loading_skips_broken_refs_for_plain_get() {
    let home = tempfile::tempdir().unwrap();
    let skills_dir = home.path().join("skills");
    let skill_root = skills_dir.join("lazy");
    std::fs::create_dir_all(skill_root.join("references")).unwrap();
    std::fs::write(
        skill_root.join("SKILL.md"),
        "---\nname: lazy\ndescription: Lazy.\n---\n\n# lazy\n",
    )
    .unwrap();
    std::fs::write(skill_root.join("references/ok.md"), "# ok\n").unwrap();
    std::fs::write(skill_root.join("references/bad.md"), [0xff, 0xfe]).unwrap();

    // Plain get reads only SKILL.md (no hang, no failure).
    let out = cmd_with_skills_dir(home.path(), &skills_dir)
        .args(["skills", "get", "lazy"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    assert!(String::from_utf8_lossy(&out.stdout).contains("# lazy"));

    // Selected good ref reads only that file.
    let out = cmd_with_skills_dir(home.path(), &skills_dir)
        .args(["skills", "get", "lazy", "--ref", "ok"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0), "{out:?}");

    // Selected bad ref surfaces I/O (exit 1), not unknown (exit 2).
    let out = cmd_with_skills_dir(home.path(), &skills_dir)
        .args(["skills", "get", "lazy", "--ref", "bad"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1), "{out:?}");

    // Full propagates the bad file instead of silently dropping it.
    let out = cmd_with_skills_dir(home.path(), &skills_dir)
        .args(["skills", "get", "lazy", "--full"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1), "{out:?}");
}

#[test]
fn skills_case_insensitive_core_policy() {
    let home = tempfile::tempdir().unwrap();
    let skills_dir = home.path().join("skills");
    let core_root = skills_dir.join("Core");
    std::fs::create_dir_all(core_root.join("references")).unwrap();
    std::fs::write(
        core_root.join("SKILL.md"),
        "---\nname: core\ndescription: Local shadow.\n---\n\n# shadow\n",
    )
    .unwrap();

    // Explicit `Core` is the `core` override: listed as `core (local)`, never
    // as a separate `Core`, never as bundled.
    let out = cmd_with_skills_dir(home.path(), &skills_dir)
        .args(["skills", "list"])
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("core (local)"), "{text}");
    assert!(!text.contains("Core ("), "{text}");
    assert!(!text.contains("core (bundled)"), "{text}");

    for name in ["core", "Core"] {
        let out = cmd_with_skills_dir(home.path(), &skills_dir)
            .args(["skills", "get", name])
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(0), "{name} {out:?}");
        assert!(String::from_utf8_lossy(&out.stdout).contains("# shadow"));
    }

    // Dangling explicit override: list advertises local, get reports I/O.
    #[cfg(unix)]
    {
        let home2 = tempfile::tempdir().unwrap();
        let skills2 = home2.path().join("skills");
        let dangling = skills2.join("core");
        std::fs::create_dir_all(&dangling).unwrap();
        std::os::unix::fs::symlink("/nonexistent-xyz", dangling.join("SKILL.md")).unwrap();
        let out = cmd_with_skills_dir(home2.path(), &skills2)
            .args(["skills", "list"])
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&out.stdout);
        assert!(text.contains("core (local)"), "{text}");
        assert!(!text.contains("core (bundled)"), "{text}");

        let out = cmd_with_skills_dir(home2.path(), &skills2)
            .args(["skills", "get", "core"])
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(1), "{out:?}");
    }
}

#[cfg(unix)]
#[test]
fn skills_directory_cycle_does_not_hang() {
    use std::time::Duration;
    let home = tempfile::tempdir().unwrap();
    let skills_dir = home.path().join("skills");
    let skill_root = skills_dir.join("cyc");
    std::fs::create_dir_all(skill_root.join("references/a")).unwrap();
    std::fs::write(
        skill_root.join("SKILL.md"),
        "---\nname: cyc\ndescription: Cycle.\n---\n\n# cyc\n",
    )
    .unwrap();
    std::fs::write(skill_root.join("references/ok.md"), "# ok\n").unwrap();
    std::os::unix::fs::symlink("..", skill_root.join("references/a/loop")).unwrap();

    for args in [
        vec!["skills", "list"],
        vec!["skills", "get", "cyc", "--full"],
    ] {
        let args_owned: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let mut cmd = Command::new(bin());
        cmd.env_clear()
            .env("HOME", home.path())
            .env("XDG_CONFIG_HOME", "")
            .env("AGENT_WEBMCP_SKILLS_DIR", &skills_dir)
            .env("PATH", std::env::var("PATH").unwrap_or_default());
        cmd.args(&args_owned);
        // Retain the `Child` so a cycle hang is killed, not leaked.
        let out = output_with_timeout(cmd, Duration::from_secs(20));
        assert_eq!(out.status.code(), Some(0), "{args:?} {out:?}");
    }
}

#[test]
fn skills_multiline_description_parsing() {
    let home = tempfile::tempdir().unwrap();
    let skills_dir = home.path().join("skills");
    let skill_root = skills_dir.join("mydesc");
    std::fs::create_dir_all(&skill_root).unwrap();
    std::fs::write(
        skill_root.join("SKILL.md"),
        "---\nname: mydesc\ndescription: >-\n  Line one\n  Line two\n---\n\n# mydesc\n",
    )
    .unwrap();

    let out = cmd_with_skills_dir(home.path(), &skills_dir)
        .args(["skills", "list"])
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("Line one Line two"), "{text}");
    assert!(!text.contains(">-"), "{text}");

    let out = cmd_with_skills_dir(home.path(), &skills_dir)
        .args(["skills", "list", "--json"])
        .output()
        .unwrap();
    let value = stdout_json(&out);
    let desc = value["skills"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["name"] == "mydesc")
        .unwrap()["description"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(desc, "Line one Line two");
}

/// Connect an in-process MCP client to `server` and hand both ends to `f`.
/// Each server carries its own explicit skills dir, so no test mutates the
/// process-wide `AGENT_WEBMCP_SKILLS_DIR`.
async fn with_mcp_client<F, Fut>(server: agent_webmcp::mcp::Server, f: F) -> anyhow::Result<()>
where
    F: FnOnce(
        rmcp::service::RunningService<rmcp::service::RoleClient, ()>,
        tokio::task::JoinHandle<anyhow::Result<()>>,
    ) -> Fut,
    Fut: std::future::Future<Output = anyhow::Result<()>>,
{
    use rmcp::model::{Implementation, InitializeRequestParams, ProtocolVersion};
    let client_info = InitializeRequestParams::new(
        rmcp::model::ClientCapabilities::builder().build(),
        Implementation::new("skills-test", "0.0.0"),
    )
    .with_protocol_version(ProtocolVersion::V_2026_07_28);
    let mut server_info = rmcp::model::InitializeResult::new(
        rmcp::model::ServerCapabilities::builder()
            .enable_tools()
            .build(),
    );
    server_info.protocol_version = ProtocolVersion::V_2026_07_28;

    tokio::task::LocalSet::new()
        .run_until(async move {
            let (server_transport, client_transport) = tokio::io::duplex(8192);
            let server_task = tokio::task::spawn_local(async move {
                let running =
                    rmcp::service::serve_directly::<rmcp::service::RoleServer, _, _, _, _>(
                        server,
                        server_transport,
                        Some(client_info),
                    );
                running.waiting().await?;
                anyhow::Ok(())
            });
            let client = rmcp::service::serve_directly::<rmcp::service::RoleClient, _, _, _, _>(
                (),
                client_transport,
                Some(server_info.into()),
            );
            f(client, server_task).await
        })
        .await
}

#[tokio::test(flavor = "current_thread")]
async fn mcp_json_mime_and_io_preservation() -> anyhow::Result<()> {
    // Explicit dir with a JSON template to check MIME preservation.
    let home = tempfile::tempdir().unwrap();
    let skills_dir = home.path().join("skills");
    let tmpl_root = skills_dir.join("tmpl");
    std::fs::create_dir_all(tmpl_root.join("templates")).unwrap();
    std::fs::write(
        tmpl_root.join("SKILL.md"),
        "---\nname: tmpl\ndescription: Tmpl.\n---\n\n# tmpl\n",
    )
    .unwrap();
    std::fs::write(
        tmpl_root.join("templates/scaffold.json"),
        "{\"hello\": 1}\n",
    )
    .unwrap();

    with_mcp_client(
        test_server().with_skills_dir(skills_dir.clone()),
        |client, server_task| async move {
            let resources = client.list_resources(None).await?;
            let json_res = resources
                .resources
                .iter()
                .find(|r| r.uri == "skill://agent-webmcp/tmpl/templates/scaffold.json")
                .expect("json template resource missing");
            assert_eq!(json_res.mime_type.as_deref(), Some("application/json"));
            server_task.abort();
            anyhow::Ok(())
        },
    )
    .await?;

    // Unreadable local skill: I/O must surface, not "unknown".
    let bad_dir = home.path().join("badskills");
    let bad_root = bad_dir.join("badutf");
    std::fs::create_dir_all(&bad_root)?;
    std::fs::write(bad_root.join("SKILL.md"), [0xff, 0xfe])?;
    with_mcp_client(
        test_server().with_skills_dir(bad_dir.clone()),
        |client, server_task| async move {
            let err = client
                .read_resource(rmcp::model::ReadResourceRequestParams::new(
                    "skill://agent-webmcp/badutf",
                ))
                .await
                .unwrap_err();
            let msg = format!("{err:?}");
            // Internal I/O, not "unknown skill resource".
            assert!(
                msg.contains("internal") || msg.contains("could not read"),
                "{msg}"
            );
            assert!(!msg.to_lowercase().contains("unknown skill"), "{msg}");
            server_task.abort();
            anyhow::Ok(())
        },
    )
    .await
}

/// The production MCP server honors `AGENT_WEBMCP_SKILLS_DIR` exactly like
/// the CLI: same env, same `resources/list` URIs as `skills list --json`.
#[cfg(unix)]
#[test]
fn mcp_production_honors_skills_dir_env_like_cli() {
    let home = tempfile::tempdir().unwrap();
    let skills_dir = home.path().join("skills");
    let prod = skills_dir.join("prodskill");
    std::fs::create_dir_all(prod.join("references")).unwrap();
    std::fs::write(
        prod.join("SKILL.md"),
        "---\nname: prodskill\ndescription: Prod.\n---\n\n# prod\n",
    )
    .unwrap();
    std::fs::write(prod.join("references/guide.md"), "# guide\n").unwrap();

    // CLI ground truth with the env var set in the child process only.
    let out = cmd_with_skills_dir(home.path(), &skills_dir)
        .args(["skills", "list", "--json"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    let value = stdout_json(&out);
    let cli_names: Vec<String> = value["skills"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|s| s["name"].as_str().map(str::to_string))
        .collect();
    assert!(
        cli_names.contains(&"prodskill".to_string()),
        "{cli_names:?}"
    );

    // Empty index so the MCP child starts without network access.
    let index = agent_webmcp::model::Index {
        schema_version: 1,
        generated_at: "2026-11-19T00:00:00Z".into(),
        sites: vec![],
    };
    let index_path = home.path().join("index.json");
    std::fs::write(&index_path, serde_json::to_string(&index).unwrap()).unwrap();

    let uris = mcp_resources_via_stdio(home.path(), &skills_dir, &index_path);
    for name in &cli_names {
        let exact = format!("skill://agent-webmcp/{name}");
        assert!(
            uris.iter()
                .any(|u| u == &exact || u.starts_with(&format!("{exact}/"))),
            "CLI skill {name} missing from MCP resources: {uris:?}"
        );
    }
    assert!(
        uris.contains(&"skill://agent-webmcp/prodskill/references/guide.md".to_string()),
        "{uris:?}"
    );
}

#[test]
fn skills_parent_json_path_outputs_json() {
    let home = tempfile::tempdir().unwrap();
    let skills_dir = home.path().join("custom-skills");
    std::fs::create_dir_all(&skills_dir).unwrap();

    // Parent `--json` must be honored just like `path --json`.
    let out = cmd_with_skills_dir(home.path(), &skills_dir)
        .args(["skills", "--json", "path"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    let value = stdout_json(&out);
    assert!(
        value["path"].as_str().unwrap().ends_with("custom-skills"),
        "{value}"
    );
}

#[test]
fn skills_parent_json_sync_outputs_json() {
    let home = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(home.path().join(".agents")).unwrap();

    let out = base_cmd(home.path())
        .args(["skills", "--json", "sync"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    let value = stdout_json(&out);
    assert!(value["installed"].is_array(), "{value}");
    assert!(value["updated"].is_array(), "{value}");
    assert!(value["unchanged"].is_array(), "{value}");
}

#[test]
fn skills_full_propagates_traversal_errors() {
    let home = tempfile::tempdir().unwrap();
    let skills_dir = home.path().join("skills");
    // `references` as a plain file (not a directory): listing is best-effort
    // (no refs), but `--full` must report I/O (exit 1), not silent success.
    let skill_root = skills_dir.join("badbase");
    std::fs::create_dir_all(&skill_root).unwrap();
    std::fs::write(
        skill_root.join("SKILL.md"),
        "---\nname: badbase\ndescription: Bad.\n---\n\n# bad\n",
    )
    .unwrap();
    std::fs::write(skill_root.join("references"), "not a dir\n").unwrap();

    let out = cmd_with_skills_dir(home.path(), &skills_dir)
        .args(["skills", "get", "badbase", "--full"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1), "{out:?}");

    // Plain get still works (lazy, no bodies).
    let out = cmd_with_skills_dir(home.path(), &skills_dir)
        .args(["skills", "get", "badbase"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0), "{out:?}");
}

#[cfg(unix)]
#[test]
fn skills_full_propagates_escaping_dir_symlink_cli() {
    let home = tempfile::tempdir().unwrap();
    let skills_dir = home.path().join("skills");
    let outside = home.path().join("outside");
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(outside.join("evil.md"), "# evil\n").unwrap();
    let skill_root = skills_dir.join("evcli");
    std::fs::create_dir_all(skill_root.join("references")).unwrap();
    std::fs::write(
        skill_root.join("SKILL.md"),
        "---\nname: evcli\ndescription: Ev.\n---\n\n# ev\n",
    )
    .unwrap();
    std::fs::write(skill_root.join("references/ok.md"), "# ok\n").unwrap();
    std::os::unix::fs::symlink(&outside, skill_root.join("references/evil")).unwrap();

    let out = cmd_with_skills_dir(home.path(), &skills_dir)
        .args(["skills", "get", "evcli", "--full"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1), "{out:?}");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("escapes"), "{stderr}");
}

#[test]
fn skills_list_json_excludes_oversized_refs_but_ref_errors() {
    // resources/list and skills list share filtered discovery
    let home = tempfile::tempdir().unwrap();
    let skills_dir = home.path().join("skills");
    let skill_root = skills_dir.join("bigres");
    std::fs::create_dir_all(skill_root.join("references")).unwrap();
    std::fs::write(
        skill_root.join("SKILL.md"),
        "---
name: bigres
description: Big.
---

# big
",
    )
    .unwrap();
    std::fs::write(
        skill_root.join("references/ok.md"),
        "# ok
",
    )
    .unwrap();
    let big = vec![b'a'; (512 * 1024 + 10) as usize];
    std::fs::write(skill_root.join("references/big.md"), &big).unwrap();

    let out = cmd_with_skills_dir(home.path(), &skills_dir)
        .args(["skills", "list", "--json"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    let value = stdout_json(&out);
    let skill = value["skills"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["name"] == "bigres")
        .expect("bigres must be listed");
    let refs = skill["refs"].as_array().unwrap();
    let ref_strs: Vec<&str> = refs.iter().map(|r| r.as_str().unwrap()).collect();
    assert!(
        ref_strs.contains(&"references/ok.md"),
        "ok must be listed, got {ref_strs:?}"
    );
    assert!(
        !ref_strs.contains(&"references/big.md"),
        "oversized must be excluded from listing, got {ref_strs:?}"
    );

    let out = cmd_with_skills_dir(home.path(), &skills_dir)
        .args(["skills", "get", "bigres", "--ref", "big"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1), "{out:?}");
}

#[test]
fn permissions_ref_documents_ask_precedence() {
    // `call::decide` is deny > ask > allow: `allow` must note both.
    let home = tempfile::tempdir().unwrap();
    let out = base_cmd(home.path())
        .args(["skills", "get", "core", "--ref", "permissions"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        text.contains("unless also matched by `ask` or `deny`"),
        "allow precedence must mention ask+deny, got: {text}"
    );
}

#[test]
fn frontmatter_indented_fence_preserved_in_list() {
    // An indented `---` inside a `|` scalar is content (column-0 rule).
    let home = tempfile::tempdir().unwrap();
    let skills_dir = home.path().join("skills");
    let skill_root = skills_dir.join("fenced");
    std::fs::create_dir_all(&skill_root).unwrap();
    std::fs::write(
        skill_root.join("SKILL.md"),
        "---\nname: fenced\ndescription: |\n  Line A\n  ---\n  Line B\n---\n\n# fenced\n",
    )
    .unwrap();

    let out = cmd_with_skills_dir(home.path(), &skills_dir)
        .args(["skills", "list", "--json"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    let value = stdout_json(&out);
    let desc = value["skills"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["name"] == "fenced")
        .expect("fenced listed")["description"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(desc, "Line A\n---\nLine B");
}

#[test]
fn unreadable_skills_dir_does_not_fall_back_to_bundled() {
    // A file where the skills dir should be fails `read_dir` with
    // `NotADirectory` (not `NotFound`): `list` must advertise `local` and
    // `get` must report I/O (exit 1), never silently use bundled `core`.
    let home = tempfile::tempdir().unwrap();
    let blocker = home.path().join("not-a-dir");
    std::fs::write(&blocker, b"not a dir").unwrap();

    let out = cmd_with_skills_dir(home.path(), &blocker)
        .args(["skills", "list"])
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("core (local)"), "{text}");
    assert!(!text.contains("core (bundled)"), "{text}");

    let out = cmd_with_skills_dir(home.path(), &blocker)
        .args(["skills", "get", "core"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1), "{out:?}");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("could not read"), "{stderr}");
}
