use agent_webmcp::agent_browser::{parse_invocation, parse_tools};
use serde_json::json;

#[test]
fn parses_a_webmcp_list_response() {
    let data = json!({
        "tools": [{
            "name": "searchFlights",
            "description": "Searches for flights.",
            "frameId": "F1",
            "origin": "https://googlechromelabs.github.io",
            "annotations": { "readOnly": true, "consequential": false },
            "inputSchema": { "type": "object", "properties": { "origin": { "type": "string" } } }
        }]
    });
    let tools = parse_tools(&data);
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].name, "searchFlights");
    assert_eq!(tools[0].frame_id, "F1");
    assert_eq!(tools[0].origin.as_deref(), Some("https://googlechromelabs.github.io"));
    assert!(tools[0].read_only);
    assert_eq!(tools[0].input_schema["type"], "object");
}

#[test]
fn skips_tools_without_a_frame_and_survives_a_missing_list() {
    assert!(parse_tools(&json!({ "tools": [{ "name": "x" }] })).is_empty());
    assert!(parse_tools(&json!({})).is_empty());
    assert!(parse_tools(&json!(null)).is_empty());
}

#[test]
fn parses_a_completed_invocation() {
    let data = json!({
        "status": "completed",
        "output": { "content": [{ "type": "text", "text": "ok" }] },
        "origin": "https://example.com",
        "durationMs": 22
    });
    let inv = parse_invocation(&data).unwrap();
    assert_eq!(inv.output["content"][0]["text"], "ok");
    assert_eq!(inv.origin.as_deref(), Some("https://example.com"));
    assert_eq!(inv.duration_ms, Some(22));
}

#[test]
fn a_tool_that_did_not_complete_is_an_error() {
    let err = parse_invocation(&json!({ "status": "canceled" })).unwrap_err().to_string();
    assert!(err.contains("canceled"), "{err}");
    assert!(parse_invocation(&json!({ "status": "error", "errorText": "boom" })).is_err());
}
