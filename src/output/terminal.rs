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

/// Render a section title with centered text flanked by `-` lines.
pub(crate) fn render_section_title(title: &str) {
    let width = header_content_width();
    let title_with_padding = format!(" {} ", title);
    let title_len = UnicodeWidthStr::width(title_with_padding.as_str());
    let remaining = width.saturating_sub(title_len);
    let left = remaining / 2;
    let right = remaining - left;
    println!();
    println!(
        " {}{}{} ",
        "─".repeat(left).dimmed(),
        title_with_padding.dimmed(),
        "─".repeat(right).dimmed(),
    );
}
