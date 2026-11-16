use anyhow::{Context, Result, anyhow, bail};
use rmcp::model::{CallToolRequestParams, ContentBlock};
use rmcp::service::RunningService;
use rmcp::transport::{ConfigureCommandExt, TokioChildProcess};
use rmcp::{RoleClient, ServiceExt};
use serde_json::{Value, json};

use crate::backend::{Backend, Invocation, PageTool, Tab};
use crate::mcp::Provider;

/// agent-browser, used as an MCP client of `agent-browser mcp --tools core,webmcp`.
pub struct AgentBrowser {
    client: RunningService<RoleClient, ()>,
    session: String,
}

impl AgentBrowser {
    /// Start agent-browser's MCP server. `AGENT_WEBMCP_AGENT_BROWSER` overrides the binary.
    pub async fn connect(session: &str) -> Result<Self> {
        let bin = std::env::var("AGENT_WEBMCP_AGENT_BROWSER").unwrap_or_else(|_| "agent-browser".into());
        let transport = TokioChildProcess::new(tokio::process::Command::new(&bin).configure(|c| {
            c.args(["mcp", "--tools", "core,webmcp"]);
        }))
        .with_context(|| format!("starting `{bin} mcp`; is agent-browser installed?"))?;
        let client = ()
            .serve(transport)
            .await
            .context("connecting to agent-browser's MCP server")?;
        Ok(Self { client, session: session.to_string() })
    }

    /// Close the browser this session owns, then stop the MCP server.
    pub async fn shutdown(mut self, close_browser: bool) -> Result<()> {
        if close_browser {
            let _ = self.tool("agent_browser_close", json!({})).await;
        }
        self.client.close().await.map_err(|e| anyhow!("closing agent-browser: {e}"))?;
        Ok(())
    }

    /// Call one agent-browser tool and return the `data` of its response.
    async fn tool(&self, name: &'static str, mut args: Value) -> Result<Value> {
        let obj = args.as_object_mut().expect("tool arguments are an object");
        obj.insert("session".into(), json!(self.session));
        let params = CallToolRequestParams::new(name).with_arguments(obj.clone());
        let result = self
            .client
            .call_tool(params)
            .await
            .map_err(|e| anyhow!("{name}: {e}"))?;
        if result.is_error == Some(true) {
            bail!("{name} failed: {}", error_text(&result.content, result.structured_content.as_ref()));
        }
        let response = result
            .structured_content
            .as_ref()
            .and_then(|v| v.get("response"))
            .cloned()
            .or_else(|| {
                result.content.iter().find_map(|c| match c {
                    ContentBlock::Text(t) => serde_json::from_str::<Value>(&t.text).ok(),
                    _ => None,
                })
            })
            .ok_or_else(|| anyhow!("{name}: no response in result"))?;
        Ok(response.get("data").cloned().unwrap_or(Value::Null))
    }
}

fn error_text(content: &[ContentBlock], structured: Option<&Value>) -> String {
    if let Some(e) = structured
        .and_then(|v| v.get("response"))
        .and_then(|r| r.get("error").or(Some(r)))
    {
        let msg = e.as_str().map(str::to_string).unwrap_or_else(|| e.to_string());
        if !msg.is_empty() && msg != "null" {
            return msg;
        }
    }
    content
        .iter()
        .filter_map(|c| match c {
            ContentBlock::Text(t) => Some(t.text.clone()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The tools array of a `webmcp list` response.
pub fn parse_tools(data: &Value) -> Vec<PageTool> {
    data.get("tools")
        .and_then(Value::as_array)
        .map(|tools| {
            tools
                .iter()
                .filter_map(|t| {
                    Some(PageTool {
                        name: t.get("name")?.as_str()?.to_string(),
                        description: t.get("description").and_then(Value::as_str).unwrap_or("").to_string(),
                        frame_id: t.get("frameId")?.as_str()?.to_string(),
                        origin: t.get("origin").and_then(Value::as_str).map(str::to_string),
                        read_only: t
                            .pointer("/annotations/readOnly")
                            .and_then(Value::as_bool)
                            .unwrap_or(false),
                        consequential: t
                            .pointer("/annotations/consequential")
                            .and_then(Value::as_bool)
                            .unwrap_or(false),
                        input_schema: t.get("inputSchema").cloned().unwrap_or(Value::Null),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The result of a `webmcp invoke` response.
pub fn parse_invocation(data: &Value) -> Result<Invocation> {
    let status = data.get("status").and_then(Value::as_str).unwrap_or("");
    if status != "completed" {
        bail!("the tool did not complete ({status}): {data}");
    }
    Ok(Invocation {
        output: data.get("output").cloned().unwrap_or(Value::Null),
        origin: data.get("origin").and_then(Value::as_str).map(str::to_string),
        duration_ms: data.get("durationMs").and_then(Value::as_u64),
    })
}

impl Backend for AgentBrowser {
    async fn open(&self, url: &str) -> Result<Tab> {
        self.tool("agent_browser_open", json!({ "url": url })).await?;
        Ok(Tab { session: self.session.clone(), url: url.to_string() })
    }

    async fn inject(&self, _tab: &Tab, script: &str) -> Result<()> {
        self.tool("agent_browser_eval", json!({ "script": script })).await?;
        Ok(())
    }

    async fn list_tools(&self, _tab: &Tab) -> Result<Vec<PageTool>> {
        let data = self.tool("agent_browser_webmcp_list", json!({})).await?;
        Ok(parse_tools(&data))
    }

    async fn invoke(&self, _tab: &Tab, tool: &PageTool, args: Value) -> Result<Invocation> {
        let data = self
            .tool(
                "agent_browser_webmcp_invoke",
                json!({ "tool": tool.name, "frameId": tool.frame_id, "params": args }),
            )
            .await?;
        parse_invocation(&data)
    }
}

/// Starts a fresh agent-browser session for each call and closes it afterwards.
#[derive(Default)]
pub struct AgentBrowserProvider {
    next: std::sync::atomic::AtomicU64,
}

impl Provider for AgentBrowserProvider {
    type B = AgentBrowser;

    async fn connect(&self) -> Result<AgentBrowser> {
        let n = self.next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        AgentBrowser::connect(&format!("agent-webmcp-{}-{n}", std::process::id())).await
    }

    async fn finish(&self, backend: AgentBrowser) {
        if let Err(e) = backend.shutdown(true).await {
            eprintln!("agent-webmcp: closing the browser failed: {e:#}");
        }
    }
}
