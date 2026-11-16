use std::sync::Mutex;
use std::time::Duration;

use agent_webmcp::backend::{Backend, Invocation, PageTool, Tab};
use agent_webmcp::catalog::Catalog;
use agent_webmcp::engine::{Options, call};
use anyhow::Result;
use serde_json::{Value, json};

/// A browser stand-in. `registered` is what the page registers on its own; `injected` is
/// what appears after a script is injected.
struct Fake {
    registered: Vec<PageTool>,
    injected: Vec<PageTool>,
    /// The list is empty for this many `list_tools` calls, like a page still loading.
    late_by: Mutex<u32>,
    log: Mutex<Vec<String>>,
    did_inject: Mutex<bool>,
}

fn tool(name: &str, origin: &str) -> PageTool {
    PageTool {
        name: name.into(),
        description: String::new(),
        frame_id: "frame-1".into(),
        origin: Some(origin.into()),
        read_only: true,
        input_schema: Value::Null,
    }
}

impl Fake {
    fn new(registered: Vec<PageTool>, injected: Vec<PageTool>) -> Self {
        Self {
            registered,
            injected,
            late_by: Mutex::new(0),
            log: Mutex::default(),
            did_inject: Mutex::new(false),
        }
    }
    fn log(&self) -> Vec<String> {
        self.log.lock().unwrap().clone()
    }
}

impl Backend for Fake {
    async fn open(&self, url: &str) -> Result<Tab> {
        self.log.lock().unwrap().push(format!("open {url}"));
        Ok(Tab { session: "s".into(), url: url.into() })
    }
    async fn inject(&self, _: &Tab, script: &str) -> Result<()> {
        self.log.lock().unwrap().push(format!("inject {} bytes", script.len()));
        *self.did_inject.lock().unwrap() = true;
        Ok(())
    }
    async fn list_tools(&self, _: &Tab) -> Result<Vec<PageTool>> {
        let mut late = self.late_by.lock().unwrap();
        if *late > 0 {
            *late -= 1;
            return Ok(vec![]);
        }
        let mut all = self.registered.clone();
        if *self.did_inject.lock().unwrap() {
            all.extend(self.injected.clone());
        }
        Ok(all)
    }
    async fn invoke(&self, _: &Tab, t: &PageTool, args: Value) -> Result<Invocation> {
        self.log.lock().unwrap().push(format!("invoke {} {args}", t.name));
        Ok(Invocation {
            output: json!({ "echo": args }),
            origin: t.origin.clone(),
            duration_ms: Some(1),
        })
    }
}

fn fast() -> Options {
    Options { approved: false, poll_attempts: 4, poll_delay: Duration::from_millis(1) }
}

fn catalog() -> Catalog {
    Catalog::embedded().unwrap()
}

#[tokio::test]
async fn native_tool_opens_the_page_and_calls_without_injecting() {
    let origin = "https://googlechromelabs.github.io";
    let fake = Fake::new(vec![tool("searchFlights", origin)], vec![]);
    let out = call(
        &fake,
        &catalog(),
        "googlechromelabs.github.io/searchFlights",
        json!({ "origin": "SFO" }),
        &fast(),
    )
    .await
    .unwrap();
    assert_eq!(out.origin, origin);
    assert_eq!(out.output["echo"]["origin"], "SFO");
    let log = fake.log();
    assert!(log[0].starts_with("open https://googlechromelabs.github.io/"));
    assert!(!log.iter().any(|l| l.starts_with("inject")), "{log:?}");
}

#[tokio::test]
async fn community_tool_is_injected_before_it_is_called() {
    let fake = Fake::new(vec![], vec![tool("say_hello", "https://example.com")]);
    let out = call(&fake, &catalog(), "example.com/say_hello", json!({ "name": "Ada" }), &fast())
        .await
        .unwrap();
    assert_eq!(out.output["echo"]["name"], "Ada");
    let log = fake.log();
    assert!(log[0].starts_with("open https://example.com/"));
    assert!(log[1].starts_with("inject "), "{log:?}");
    assert!(log[2].starts_with("invoke say_hello"), "{log:?}");
}

#[tokio::test]
async fn waits_for_a_page_that_registers_its_tool_late() {
    let fake = Fake::new(vec![tool("searchFlights", "https://googlechromelabs.github.io")], vec![]);
    *fake.late_by.lock().unwrap() = 2;
    let out = call(&fake, &catalog(), "googlechromelabs.github.io/searchFlights", json!({}), &fast()).await;
    assert!(out.is_ok(), "{out:?}");
}

#[tokio::test]
async fn gives_up_and_names_the_tools_the_page_does_have() {
    let fake = Fake::new(vec![tool("other", "https://example.com")], vec![]);
    let err = call(&fake, &catalog(), "example.com/say_hello", json!({}), &fast())
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("say_hello is not registered"), "{err}");
    assert!(err.contains("other"), "{err}");
}

#[tokio::test]
async fn refuses_a_tool_registered_by_another_origin() {
    let fake = Fake::new(vec![tool("say_hello", "https://evil.example")], vec![]);
    let err = call(&fake, &catalog(), "example.com/say_hello", json!({}), &fast())
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("different origin"), "{err}");
    assert!(!fake.log().iter().any(|l| l.starts_with("invoke")));
}

#[tokio::test]
async fn accepts_a_www_redirect_of_the_same_site() {
    let fake = Fake::new(vec![tool("say_hello", "https://www.example.com")], vec![]);
    assert!(call(&fake, &catalog(), "example.com/say_hello", json!({}), &fast()).await.is_ok());
}

#[tokio::test]
async fn write_tools_need_approval_and_nothing_is_opened_without_it() {
    let fake = Fake::new(vec![], vec![tool("leave_note", "https://example.com")]);
    let err = call(&fake, &catalog(), "example.com/leave_note", json!({ "text": "hi" }), &fast())
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("needs approval"), "{err}");
    assert!(fake.log().is_empty(), "no browser activity before approval");

    let approved = Options { approved: true, ..fast() };
    assert!(call(&fake, &catalog(), "example.com/leave_note", json!({ "text": "hi" }), &approved).await.is_ok());
}

#[tokio::test]
async fn unknown_ids_and_bad_arguments_are_rejected_before_the_browser() {
    let fake = Fake::new(vec![], vec![]);
    let e1 = call(&fake, &catalog(), "nope.com/x", json!({}), &fast()).await.unwrap_err().to_string();
    assert!(e1.contains("unknown tool"), "{e1}");
    let e2 = call(&fake, &catalog(), "example.com/say_hello", json!(["a"]), &fast())
        .await
        .unwrap_err()
        .to_string();
    assert!(e2.contains("JSON object"), "{e2}");
    assert!(fake.log().is_empty());
}
