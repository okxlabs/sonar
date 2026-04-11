//! Shared formatting utilities for terminal output.

/// Format a number with comma separators for readability (e.g. `1000000` → `1,000,000`).
pub(crate) fn format_with_commas(n: u64) -> String {
    let s = n.to_string();
    let mut result = String::new();
    for (i, c) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            result.push(',');
        }
        result.push(c);
    }
    result.chars().rev().collect()
}

/// Truncate a display string to `limit` characters, appending `…` if truncated.
/// Safe for multi-byte UTF-8 — slices on char boundaries.
pub(crate) fn truncate_display(value: &str, limit: usize) -> String {
    if value.len() <= limit {
        value.to_string()
    } else {
        // Find the largest char boundary <= limit (equivalent to floor_char_boundary,
        // which requires Rust 1.93+).
        let mut end = limit;
        while end > 0 && !value.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}…", &value[..end])
    }
}

/// Truncate a signature to `prefix…suffix` form (e.g. `AbcDef...xyzXYZ`).
pub(crate) fn truncate_sig(sig: &str, prefix_len: usize) -> String {
    if sig.len() <= prefix_len * 2 + 3 {
        sig.to_string()
    } else {
        format!("{}...{}", &sig[..prefix_len], &sig[sig.len() - prefix_len..])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_with_commas_basic() {
        assert_eq!(format_with_commas(0), "0");
        assert_eq!(format_with_commas(999), "999");
        assert_eq!(format_with_commas(1_000), "1,000");
        assert_eq!(format_with_commas(1_000_000), "1,000,000");
    }

    #[test]
    fn truncate_display_within_limit() {
        assert_eq!(truncate_display("hello", 10), "hello");
    }

    #[test]
    fn truncate_display_at_limit() {
        assert_eq!(truncate_display("hello", 5), "hello");
    }

    #[test]
    fn truncate_display_over_limit() {
        assert_eq!(truncate_display("hello world", 5), "hello…");
    }

    #[test]
    fn truncate_display_multibyte_safe() {
        // 3-byte chars: "café" is c(1) a(1) f(1) é(2 bytes)
        let s = "café";
        let result = truncate_display(s, 4);
        // Should not panic; truncates at char boundary
        assert!(result.ends_with('…') || result == s);
    }

    #[test]
    fn truncate_sig_short() {
        assert_eq!(truncate_sig("abcdef", 3), "abcdef");
    }

    #[test]
    fn truncate_sig_long() {
        assert_eq!(truncate_sig("abcdefghijklmnop", 3), "abc...nop");
    }
}
