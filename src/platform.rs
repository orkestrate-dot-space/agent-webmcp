//! Known platforms whose sites share one set of tools. Search shows each
//! platform's shared tools once, with a site count, instead of once per site.

/// (platform, tool names that identify it). All names must be present.
const SIGNATURES: &[(&str, &[&str])] = &[
    ("mintlify", &["search_docs", "read_page", "navigate_page", "get_site_overview"]),
];

pub fn detect(tool_names: &[&str]) -> Option<String> {
    SIGNATURES
        .iter()
        .find(|(_, required)| required.iter().all(|r| tool_names.contains(r)))
        .map(|(name, _)| (*name).to_string())
}

/// Tool names a platform ships on every site. Only these collapse in search.
const SHARED: &[(&str, &[&str])] = &[(
    "mintlify",
    &["search_docs", "read_page", "navigate_page", "get_site_overview", "read_skill"],
)];

pub fn is_shared(platform: &str, tool: &str) -> bool {
    SHARED
        .iter()
        .any(|(p, names)| *p == platform && names.contains(&tool))
}
