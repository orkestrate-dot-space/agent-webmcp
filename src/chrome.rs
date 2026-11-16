//! Launch a headless Chrome with WebMCP switched on, or attach to one.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};

/// Features that turn on WebMCP and its DevTools domain.
pub const WEBMCP_FEATURES: &str = "WebMCP,WebMCPTesting,DevToolsWebMCPSupport";

pub struct Chrome {
    pub ws_url: String,
    child: Option<Child>,
    profile: Option<PathBuf>,
}

impl Chrome {
    /// Start a private headless Chrome. `AGENT_WEBMCP_CHROME` overrides the binary.
    pub async fn launch() -> Result<Self> {
        let bin = find_chrome().context(
            "no Chrome found. Install Chrome for Testing (`npx @puppeteer/browsers install chrome@stable`) \
             or set AGENT_WEBMCP_CHROME to a Chrome or Chromium binary",
        )?;
        let profile = std::env::temp_dir().join(format!("agent-webmcp-chrome-{}", std::process::id()));
        let mut child = Command::new(&bin)
            .args([
                "--headless=new",
                "--no-first-run",
                "--no-default-browser-check",
                "--disable-gpu",
                "--mute-audio",
                "--remote-debugging-port=0",
                &format!("--enable-features={WEBMCP_FEATURES}"),
                &format!("--user-data-dir={}", profile.display()),
                "about:blank",
            ])
            .args(sandbox_args())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .with_context(|| format!("starting {}", bin.display()))?;

        // Chrome prints the WebSocket URL on stderr once it is listening.
        let stderr = child.stderr.take().context("no stderr from Chrome")?;
        let mut lines = BufReader::new(stderr).lines();
        let ws_url = tokio::time::timeout(Duration::from_secs(20), async {
            while let Some(line) = lines.next_line().await? {
                if let Some(url) = line.strip_prefix("DevTools listening on ") {
                    return Ok::<_, anyhow::Error>(url.trim().to_string());
                }
            }
            bail!("Chrome exited before it was ready")
        })
        .await
        .context("Chrome did not start within 20s")??;
        // Keep draining stderr so Chrome never blocks on a full pipe.
        tokio::spawn(async move { while let Ok(Some(_)) = lines.next_line().await {} });

        Ok(Self { ws_url, child: Some(child), profile: Some(profile) })
    }

    /// Use a browser someone else started. We never close it.
    pub fn attach(ws_url: impl Into<String>) -> Self {
        Self { ws_url: ws_url.into(), child: None, profile: None }
    }

    pub async fn close(mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill().await;
        }
        if let Some(dir) = self.profile.take() {
            let _ = std::fs::remove_dir_all(dir);
        }
    }
}

fn sandbox_args() -> Vec<&'static str> {
    // Root inside containers cannot use Chrome's sandbox.
    if cfg!(target_os = "linux") && std::env::var("AGENT_WEBMCP_NO_SANDBOX").is_ok() {
        vec!["--no-sandbox"]
    } else {
        vec![]
    }
}

/// Look for a Chrome that can run WebMCP: an explicit override, then Chrome for
/// Testing caches (agent-browser's, then Puppeteer's), then system browsers.
pub fn find_chrome() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("AGENT_WEBMCP_CHROME") {
        return Some(PathBuf::from(p));
    }
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    for dir in [home.join(".agent-browser/browsers"), home.join(".cache/puppeteer/chrome")] {
        if let Some(p) = newest_chrome_in(&dir) {
            return Some(p);
        }
    }
    for name in ["google-chrome-stable", "google-chrome", "chromium", "chromium-browser", "chrome"] {
        if let Ok(p) = which::which(name) {
            return Some(p);
        }
    }
    let mac = Path::new("/Applications/Google Chrome.app/Contents/MacOS/Google Chrome");
    mac.exists().then(|| mac.to_path_buf())
}

fn newest_chrome_in(dir: &Path) -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .flat_map(|p| {
            [
                p.join("chrome"),
                p.join("chrome-linux64/chrome"),
                p.join("chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing"),
                p.join("chrome-mac-x64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing"),
                p.join("chrome-win64/chrome.exe"),
            ]
        })
        .filter(|p| p.is_file())
        .collect();
    candidates.sort();
    candidates.pop()
}
