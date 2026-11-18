//! Untrusted text for human output: strip controls and ANSI escapes.

/// Strip control characters (including ANSI escapes) from site-provided text
/// before printing. JSON output stays as-is.
pub fn clean(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            // Skip an ANSI escape sequence: CSI (ESC [ … final byte) or OSC
            // (ESC ] … BEL); anything else drops just the ESC and its
            // introducer.
            match chars.next() {
                Some('[') => {
                    for e in chars.by_ref() {
                        if ('@'..='~').contains(&e) {
                            break;
                        }
                    }
                }
                Some(']') => {
                    for e in chars.by_ref() {
                        if e == '\x07' {
                            break;
                        }
                    }
                }
                _ => {}
            }
        } else if !c.is_control() {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_strips_ansi_escapes_and_controls() {
        assert_eq!(clean("a\x1b[31mred\x1b[0mb"), "aredb");
        assert_eq!(clean("a\x07b\tc\nd"), "abcd");
        assert_eq!(clean("plain text"), "plain text");
        // Unicode is untouched.
        assert_eq!(clean("héllo→world"), "héllo→world");
    }
}
