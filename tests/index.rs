use agent_webmcp::catalog::Catalog;
use agent_webmcp::index::Index;

fn top(c: &Catalog, i: &Index, q: &str) -> Vec<String> {
    i.search(q, 10)
        .unwrap()
        .into_iter()
        .map(|(pos, _)| c.entries[pos].id.clone())
        .collect()
}

fn setup() -> (Catalog, Index) {
    let c = Catalog::embedded().unwrap();
    let i = Index::build(&c.entries).unwrap();
    (c, i)
}

#[test]
fn finds_flights_by_intent() {
    let (c, i) = setup();
    let hits = top(&c, &i, "search flights from SFO to JFK");
    assert_eq!(hits.first().map(String::as_str), Some("googlechromelabs.github.io/searchFlights"));
}

#[test]
fn matches_camel_case_tool_names() {
    let (c, i) = setup();
    assert_eq!(top(&c, &i, "search flights")[0], "googlechromelabs.github.io/searchFlights");
}

#[test]
fn finds_a_greeting_tool() {
    let (c, i) = setup();
    assert_eq!(top(&c, &i, "say hello to Ada")[0], "example.com/say_hello");
}

#[test]
fn matches_by_site_name() {
    let (c, i) = setup();
    let hits = top(&c, &i, "example.com");
    assert!(hits.contains(&"example.com/say_hello".to_string()));
    assert!(hits.contains(&"example.com/leave_note".to_string()));
}

#[test]
fn stems_words() {
    let (c, i) = setup();
    assert_eq!(top(&c, &i, "searching")[0], "googlechromelabs.github.io/searchFlights");
}

#[test]
fn common_words_alone_match_nothing() {
    let (c, i) = setup();
    assert!(top(&c, &i, "what is the weather").is_empty());
    assert!(top(&c, &i, "to or the").is_empty());
}

#[test]
fn common_words_do_not_dilute_a_real_query() {
    let (c, i) = setup();
    let hits = top(&c, &i, "I want to search for flights");
    assert_eq!(hits, vec!["googlechromelabs.github.io/searchFlights".to_string()]);
}

#[test]
fn unrelated_query_returns_nothing() {
    let (c, i) = setup();
    assert!(top(&c, &i, "quantum chromodynamics").is_empty());
}

#[test]
fn empty_and_punctuation_queries_are_safe() {
    let (c, i) = setup();
    assert!(top(&c, &i, "").is_empty());
    assert!(top(&c, &i, "   ").is_empty());
    assert!(top(&c, &i, "\"*(:^) -").is_empty());
}

#[test]
fn respects_the_limit() {
    let (_, i) = setup();
    assert_eq!(i.search("example.com", 1).unwrap().len(), 1);
}
