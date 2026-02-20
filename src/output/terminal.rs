use colored::Colorize;
use unicode_width::UnicodeWidthStr;

/// Get effective terminal width for text rendering.
/// Falls back to 80 when width detection is unavailable.
fn terminal_width() -> usize {
    terminal_size::terminal_size().map(|(width, _)| (width.0 as usize).clamp(60, 120)).unwrap_or(80)
}

/// Header content width with one-space side margins.
fn header_content_width() -> usize {
    terminal_width().saturating_sub(2).max(1)
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
        title_with_padding.dimmed(),
        "─".repeat(right).dimmed(),
    )
}

/// Render a section title with centered text flanked by `-` lines.
pub(crate) fn render_section_title(title: &str) {
    print!("{}", build_section_title_block(title, header_content_width()));
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
