use std::sync::Mutex;
use std::time::Duration;

use agent_webmcp::approval::{AllowAll, Approver, Decision, NoApprover, Request};
use agent_webmcp::backend::{Backend, Invocation, PageTool, Tab};
use agent_webmcp::catalog::Catalog;
use agent_webmcp::engine::{self, CallError, Options, Outcome};
use agent_webmcp::ledger::{Ledger, Outcome as Verdict};
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
        consequential: false,
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
    Options { poll_attempts: 4, poll_delay: Duration::from_millis(1) }
}

/// Says yes or no to every question, and remembers what it was asked.
struct Scripted {
    answer: Decision,
    asked: Mutex<Vec<String>>,
}

impl Scripted {
    fn new(answer: Decision) -> Self {
        Self { answer, asked: Mutex::default() }
    }
    fn asked(&self) -> Vec<String> {
        self.asked.lock().unwrap().clone()
    }
}

impl Approver for Scripted {
    async fn ask(&self, request: &Request<'_>) -> Decision {
        self.asked.lock().unwrap().push(request.id.to_string());
        self.answer
    }
}

/// `call` with a throwaway ledger and everything allowed.
async fn call(
    backend: &Fake,
    catalog: &Catalog,
    id: &str,
    args: Value,
    opts: &Options,
) -> Result<Outcome, CallError> {
    let dir = tempfile::tempdir().unwrap();
    let ledger = Ledger::at(dir.path().join("ledger.jsonl"));
    engine::call(backend, &AllowAll, &ledger, catalog, id, args, opts).await
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
async fn unknown_ids_and_bad_arguments_are_rejected_before_the_browser() {
    let fake = Fake::new(vec![], vec![]);
    let e1 = call(&fake, &catalog(), "nope.com/x", json!({}), &fast()).await.unwrap_err();
    assert!(matches!(e1, CallError::UnknownTool(_)), "{e1}");
    let e2 = call(&fake, &catalog(), "example.com/say_hello", json!(["a"]), &fast()).await.unwrap_err();
    assert!(matches!(e2, CallError::BadArguments), "{e2}");
    assert!(fake.log().is_empty());
}

fn ledger_in(dir: &tempfile::TempDir) -> Ledger {
    Ledger::at(dir.path().join("ledger.jsonl"))
}

#[tokio::test]
async fn a_write_tool_asks_first_and_runs_when_approved() {
    let fake = Fake::new(vec![], vec![tool("leave_note", "https://example.com")]);
    let dir = tempfile::tempdir().unwrap();
    let approver = Scripted::new(Decision::Granted);
    let out = engine::call(
        &fake,
        &approver,
        &ledger_in(&dir),
        &catalog(),
        "example.com/leave_note",
        json!({ "text": "hi" }),
        &fast(),
    )
    .await;
    assert!(out.is_ok(), "{out:?}");
    assert_eq!(approver.asked(), vec!["example.com/leave_note"]);
    let records = ledger_in(&dir).read_all().unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].outcome, Verdict::Ok);
    assert!(records[0].approved);
}

#[tokio::test]
async fn a_declined_write_tool_does_not_touch_the_browser() {
    let fake = Fake::new(vec![], vec![tool("leave_note", "https://example.com")]);
    let dir = tempfile::tempdir().unwrap();
    let approver = Scripted::new(Decision::Declined);
    let err = engine::call(
        &fake,
        &approver,
        &ledger_in(&dir),
        &catalog(),
        "example.com/leave_note",
        json!({ "text": "hi" }),
        &fast(),
    )
    .await
    .unwrap_err();
    assert!(matches!(err, CallError::Declined { .. }), "{err}");
    assert!(fake.log().is_empty(), "no browser activity after a no: {:?}", fake.log());
    let records = ledger_in(&dir).read_all().unwrap();
    assert_eq!(records[0].outcome, Verdict::Declined);
    assert!(!records[0].approved);
}

#[tokio::test]
async fn with_nobody_to_ask_a_write_tool_is_refused_and_says_so() {
    let fake = Fake::new(vec![], vec![tool("leave_note", "https://example.com")]);
    let dir = tempfile::tempdir().unwrap();
    let err = engine::call(
        &fake,
        &NoApprover,
        &ledger_in(&dir),
        &catalog(),
        "example.com/leave_note",
        json!({ "text": "hi" }),
        &fast(),
    )
    .await
    .unwrap_err();
    assert!(matches!(err, CallError::ApprovalUnavailable { .. }), "{err}");
    assert!(err.to_string().contains("Nothing was run"), "{err}");
    assert!(fake.log().is_empty());
    assert_eq!(ledger_in(&dir).read_all().unwrap()[0].outcome, Verdict::ApprovalRequired);
}

#[tokio::test]
async fn read_tools_never_ask() {
    let fake = Fake::new(vec![], vec![tool("say_hello", "https://example.com")]);
    let dir = tempfile::tempdir().unwrap();
    let approver = Scripted::new(Decision::Declined);
    let out = engine::call(
        &fake,
        &approver,
        &ledger_in(&dir),
        &catalog(),
        "example.com/say_hello",
        json!({ "name": "Ada" }),
        &fast(),
    )
    .await;
    assert!(out.is_ok(), "{out:?}");
    assert!(approver.asked().is_empty());
    assert!(!ledger_in(&dir).read_all().unwrap()[0].approved);
}

#[tokio::test]
async fn a_page_can_make_a_read_tool_stricter_but_the_question_comes_after_it_loads() {
    let mut consequential = tool("say_hello", "https://example.com");
    consequential.consequential = true;
    let fake = Fake::new(vec![], vec![consequential]);
    let dir = tempfile::tempdir().unwrap();
    let approver = Scripted::new(Decision::Declined);
    let err = engine::call(
        &fake,
        &approver,
        &ledger_in(&dir),
        &catalog(),
        "example.com/say_hello",
        json!({ "name": "Ada" }),
        &fast(),
    )
    .await
    .unwrap_err();
    assert!(matches!(err, CallError::Declined { .. }), "{err}");
    assert_eq!(approver.asked(), vec!["example.com/say_hello"]);
    assert!(!fake.log().iter().any(|l| l.starts_with("invoke")), "{:?}", fake.log());
}

#[tokio::test]
async fn a_page_that_says_not_read_only_does_not_trigger_a_question() {
    let mut t = tool("searchFlights", "https://googlechromelabs.github.io");
    t.read_only = false;
    let fake = Fake::new(vec![t], vec![]);
    let dir = tempfile::tempdir().unwrap();
    let approver = Scripted::new(Decision::Declined);
    let out = engine::call(
        &fake,
        &approver,
        &ledger_in(&dir),
        &catalog(),
        "googlechromelabs.github.io/searchFlights",
        json!({}),
        &fast(),
    )
    .await;
    assert!(out.is_ok(), "{out:?}");
    assert!(approver.asked().is_empty());
}

#[tokio::test]
async fn failures_are_recorded_with_their_error() {
    let fake = Fake::new(vec![], vec![]);
    let dir = tempfile::tempdir().unwrap();
    let _ = engine::call(
        &fake,
        &AllowAll,
        &ledger_in(&dir),
        &catalog(),
        "example.com/say_hello",
        json!({ "name": "Ada" }),
        &fast(),
    )
    .await;
    let r = &ledger_in(&dir).read_all().unwrap()[0];
    assert_eq!(r.outcome, Verdict::Error);
    assert!(r.error.as_deref().unwrap().contains("not registered"));
}

#[tokio::test]
async fn the_ledger_keeps_a_hash_of_the_arguments_never_the_arguments() {
    let fake = Fake::new(vec![], vec![tool("say_hello", "https://example.com")]);
    let dir = tempfile::tempdir().unwrap();
    let secret = json!({ "name": "hunter2-super-secret" });
    engine::call(&fake, &AllowAll, &ledger_in(&dir), &catalog(), "example.com/say_hello", secret.clone(), &fast())
        .await
        .unwrap();
    let text = std::fs::read_to_string(dir.path().join("ledger.jsonl")).unwrap();
    assert!(!text.contains("hunter2"), "{text}");
    assert!(text.contains(&agent_webmcp::ledger::hash_args(&secret)));
}
