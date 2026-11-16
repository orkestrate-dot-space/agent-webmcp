//! Flags for tool descriptions that try to steer the agent instead of
//! describing the tool. Page authors write these descriptions, so they are
//! untrusted. Flagged tools stay in the index, marked and ranked last.

const PATTERNS: &[(&str, &str)] = &[
    ("ignore previous", "instruction_override"),
    ("ignore all previous", "instruction_override"),
    ("disregard", "instruction_override"),
    ("system prompt", "mentions_system_prompt"),
    ("you must", "imperative_to_agent"),
    ("always call", "imperative_to_agent"),
    ("mandatory first step", "imperative_to_agent"),
    ("before any other tool", "imperative_to_agent"),
    ("do not tell the user", "concealment"),
    ("<script", "markup"),
];

pub fn flags(description: &str) -> Vec<String> {
    let lower = description.to_lowercase();
    let mut out: Vec<String> = Vec::new();
    for (needle, flag) in PATTERNS {
        if lower.contains(needle) && !out.iter().any(|f| f == flag) {
            out.push((*flag).to_string());
        }
    }
    if description.trim().is_empty() {
        out.push("no_description".to_string());
    }
    out
}
