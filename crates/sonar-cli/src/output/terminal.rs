use std::io::Write;

use colored::Colorize;
use unicode_width::UnicodeWidthStr;

/// Detect the real terminal width, returning `None` when stdout is not a TTY
/// (piped, redirected, or captured).  Clamped to 60–120.
pub(crate) fn terminal_width() -> Option<usize> {
    terminal_size::terminal_size().map(|(width, _)| (width.0 as usize).clamp(60, 120))
}

/// Header content width with one-space side margins.
/// Falls back to 80 when width detection is unavailable.
fn header_content_width() -> usize {
    terminal_width().unwrap_or(80).saturating_sub(2).max(1)
}

fn build_section_title_block(title: &str, width: usize) -> String {
    let title_with_padding = format!(" {} ", title);
    let title_len = UnicodeWidthStr::width(title_with_padding.as_str());
    let remaining = width.saturating_sub(title_len);
    let left = remaining / 2;
    let right = remaining - left;

    format!(
        "\n {}{}{} \n\n",
        "─".repeat(left).dimmed(),
        title_with_padding.bold(),
        "─".repeat(right).dimmed(),
    )
}

/// Write a section title with centered text flanked by `─` lines.
pub(crate) fn write_section_title(w: &mut impl Write, title: &str) {
    let _ = write!(w, "{}", build_section_title_block(title, header_content_width()));
}

#[cfg(test)]
mod tests {
    use super::build_section_title_block;
    use unicode_width::UnicodeWidthStr;

    #[test]
    fn section_title_block_has_blank_line_after_header() {
        colored::control::set_override(false);
        let width = 40;
        let block = build_section_title_block("Account Summary", width);

        assert!(block.starts_with('\n'));
        assert!(block.ends_with("\n\n"));

        let header_line = block.trim_matches('\n');
        assert_eq!(UnicodeWidthStr::width(header_line), width + 2);
    }
}
