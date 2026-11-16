use agent_webmcp::catalog::Kind;
use agent_webmcp::policy::{Reason, approval_needed};

#[test]
fn decision_table() {
    assert_eq!(approval_needed(Kind::Read, false), None);
    assert_eq!(approval_needed(Kind::Read, true), Some(Reason::PageConsequential));
    assert_eq!(approval_needed(Kind::Write, false), Some(Reason::CatalogWrite));
    assert_eq!(approval_needed(Kind::Write, true), Some(Reason::CatalogWrite));
}
