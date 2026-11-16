use agent_webmcp::ledger::{Ledger, Outcome, Record, hash_args};
use serde_json::json;

#[test]
fn appends_one_line_per_record_and_reads_them_back_in_order() {
    let dir = tempfile::tempdir().unwrap();
    let ledger = Ledger::at(dir.path().join("nested/ledger.jsonl"));
    assert!(ledger.read_all().unwrap().is_empty(), "a missing file is an empty ledger");

    for (id, outcome) in [("a.com/x", Outcome::Ok), ("b.com/y", Outcome::Declined)] {
        ledger.append(&Record::new(id, outcome, false, &json!({}))).unwrap();
    }
    let text = std::fs::read_to_string(ledger.path()).unwrap();
    assert_eq!(text.lines().count(), 2);
    let records = ledger.read_all().unwrap();
    assert_eq!(records[0].id, "a.com/x");
    assert_eq!(records[1].outcome, Outcome::Declined);
}

#[test]
fn a_new_ledger_handle_appends_to_the_same_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ledger.jsonl");
    Ledger::at(&path).append(&Record::new("a.com/x", Outcome::Ok, false, &json!({}))).unwrap();
    Ledger::at(&path).append(&Record::new("a.com/y", Outcome::Ok, false, &json!({}))).unwrap();
    assert_eq!(Ledger::at(&path).read_all().unwrap().len(), 2);
}

#[test]
fn hashes_are_stable_and_depend_on_the_arguments() {
    let a = hash_args(&json!({ "q": 1 }));
    assert_eq!(a, hash_args(&json!({ "q": 1 })));
    assert_ne!(a, hash_args(&json!({ "q": 2 })));
    assert_eq!(a.len(), 64);
}
