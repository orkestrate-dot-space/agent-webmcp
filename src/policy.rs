use crate::catalog::Kind;

/// Why a call needs the user's approval.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    /// The catalog marks the tool as one that changes things.
    CatalogWrite,
    /// The page marks the tool as consequential when it registers it.
    PageConsequential,
}

/// Whether a call must be approved by the user before it runs, and why.
///
/// The catalog is reviewed, so its `kind` is the authority. `consequential` is what the
/// page claims when it registers the tool. A page can only make a tool stricter, never
/// looser: a catalog read tool that the page marks consequential still needs approval.
///
/// The page's `readOnly` hint is deliberately ignored. Pages routinely leave it unset;
/// Chrome's own flight-search demo reports `readOnly: false` for a plain search, so
/// treating "not read-only" as "dangerous" would ask about everything.
pub fn approval_needed(kind: Kind, consequential: bool) -> Option<Reason> {
    match (kind, consequential) {
        (Kind::Write, _) => Some(Reason::CatalogWrite),
        (Kind::Read, true) => Some(Reason::PageConsequential),
        (Kind::Read, false) => None,
    }
}
