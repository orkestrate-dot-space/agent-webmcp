use std::future::Future;

use anyhow::Result;
use serde_json::Value;

/// A browser tab a backend has opened.
#[derive(Debug, Clone)]
pub struct Tab {
    pub session: String,
    pub url: String,
}

/// A tool a page has registered through WebMCP.
#[derive(Debug, Clone)]
pub struct PageTool {
    pub name: String,
    pub description: String,
    pub frame_id: String,
    pub origin: Option<String>,
    pub read_only: bool,
    pub input_schema: Value,
}

#[derive(Debug, Clone)]
pub struct Invocation {
    pub output: Value,
    pub origin: Option<String>,
    pub duration_ms: Option<u64>,
}

/// What agent-webmcp needs from a browser driver. Every driver we care about can do
/// all four.
pub trait Backend {
    /// Open or reuse a tab and navigate it to `url`.
    fn open(&self, url: &str) -> impl Future<Output = Result<Tab>> + Send;

    /// Run a script in the page's own JavaScript world.
    fn inject(&self, tab: &Tab, script: &str) -> impl Future<Output = Result<()>> + Send;

    /// The WebMCP tools the page has registered right now.
    fn list_tools(&self, tab: &Tab) -> impl Future<Output = Result<Vec<PageTool>>> + Send;

    /// Call one of the page's tools. `args` is a JSON object.
    fn invoke(
        &self,
        tab: &Tab,
        tool: &PageTool,
        args: Value,
    ) -> impl Future<Output = Result<Invocation>> + Send;
}
