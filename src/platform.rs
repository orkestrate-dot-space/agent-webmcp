//! Platforms whose sites share one set of tools. Search shows each
//! platform's shared tools once, with a site count, instead of once per site.

/// Tool names a platform ships on every site. Only these collapse in search.
const SHARED: &[(&str, &[&str])] = &[(
    "mintlify",
    &[
        "search_docs",
        "read_page",
        "navigate_page",
        "get_site_overview",
        "read_skill",
    ],
)];

pub fn is_shared(platform: &str, tool: &str) -> bool {
    SHARED
        .iter()
        .any(|(p, names)| *p == platform && names.contains(&tool))
}
