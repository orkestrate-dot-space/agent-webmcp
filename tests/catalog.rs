use std::collections::HashMap;

use agent_webmcp::catalog::{Catalog, Kind, Source};

fn scripts() -> HashMap<String, String> {
    HashMap::from([("s.js".to_string(), "// script".to_string())])
}

fn entry(id: &str, extra: &str) -> String {
    let (site, name) = id.split_once('/').unwrap();
    format!(
        r#"{{"id":"{id}","site":"{site}","url":"https://{site}/","name":"{name}",
        "description":"Does a thing.","kind":"read","input_schema":{{"type":"object"}}{extra}}}"#
    )
}

fn parse(entries: &[String]) -> anyhow::Result<Catalog> {
    Catalog::parse(&format!("[{}]", entries.join(",")), scripts())
}

#[test]
fn embedded_catalog_is_valid() {
    let c = Catalog::embedded().unwrap();
    assert!(c.get("example.com/say_hello").is_some());
    let flights = c.get("googlechromelabs.github.io/searchFlights").unwrap();
    assert_eq!(flights.source, Source::Native);
    assert_eq!(flights.kind, Kind::Read);
    let hello = c.get("example.com/say_hello").unwrap();
    assert!(c.script(hello).unwrap().contains("registerTool"));
}

#[test]
fn native_entry_parses() {
    let c = parse(&[entry("a.com/t", r#","source":"native""#)]).unwrap();
    assert_eq!(c.entries.len(), 1);
}

#[test]
fn community_entry_resolves_its_script() {
    let c = parse(&[entry("a.com/t", r#","source":"community","script":"s.js""#)]).unwrap();
    let e = c.get("a.com/t").unwrap();
    assert_eq!(c.script(e), Some("// script"));
}

#[test]
fn rejects_id_that_does_not_match_site_and_name() {
    let json = r#"[{"id":"wrong","site":"a.com","url":"https://a.com/","name":"t",
        "description":"x","kind":"read","source":"native","input_schema":{"type":"object"}}]"#;
    let err = Catalog::parse(json, scripts()).unwrap_err().to_string();
    assert!(err.contains("id must be"), "{err}");
}

#[test]
fn rejects_duplicate_ids() {
    let e = entry("a.com/t", r#","source":"native""#);
    let err = parse(&[e.clone(), e]).unwrap_err().to_string();
    assert!(err.contains("duplicate"), "{err}");
}

#[test]
fn rejects_community_entry_without_script() {
    let err = parse(&[entry("a.com/t", r#","source":"community""#)])
        .unwrap_err()
        .to_string();
    assert!(err.contains("need a script"), "{err}");
}

#[test]
fn rejects_missing_script_file() {
    let err = parse(&[entry("a.com/t", r#","source":"community","script":"nope.js""#)])
        .unwrap_err()
        .to_string();
    assert!(err.contains("not found"), "{err}");
}

#[test]
fn rejects_native_entry_with_script() {
    let err = parse(&[entry("a.com/t", r#","source":"native","script":"s.js""#)])
        .unwrap_err()
        .to_string();
    assert!(err.contains("must not have a script"), "{err}");
}

#[test]
fn rejects_non_http_url() {
    let json = r#"[{"id":"a.com/t","site":"a.com","url":"javascript:alert(1)","name":"t",
        "description":"x","kind":"read","source":"native","input_schema":{"type":"object"}}]"#;
    let err = Catalog::parse(json, scripts()).unwrap_err().to_string();
    assert!(err.contains("url must start"), "{err}");
}

#[test]
fn rejects_empty_description() {
    let json = r#"[{"id":"a.com/t","site":"a.com","url":"https://a.com/","name":"t",
        "description":"  ","kind":"read","source":"native","input_schema":{"type":"object"}}]"#;
    let err = Catalog::parse(json, scripts()).unwrap_err().to_string();
    assert!(err.contains("description is empty"), "{err}");
}

#[test]
fn rejects_an_input_schema_that_is_not_an_object_schema() {
    let json = r#"[{"id":"a.com/t","site":"a.com","url":"https://a.com/","name":"t",
        "description":"x","kind":"read","source":"native","input_schema":{"type":"string"}}]"#;
    let err = Catalog::parse(json, scripts()).unwrap_err().to_string();
    assert!(err.contains("input_schema"), "{err}");
}

#[test]
fn every_embedded_entry_has_a_usable_schema() {
    let c = Catalog::embedded().unwrap();
    let flights = c.get("googlechromelabs.github.io/searchFlights").unwrap();
    let required = flights.input_schema["required"].as_array().unwrap();
    assert!(required.iter().any(|r| r == "origin"));
    assert_eq!(flights.input_schema["properties"]["tripType"]["enum"][0], "one-way");
}
