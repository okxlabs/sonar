//! Minimal table writer for aligned, per-cell-colored terminal output.

use std::io::Write;

use colored::Colorize;
use unicode_width::UnicodeWidthStr;

/// Column alignment.
#[derive(Clone, Copy)]
#[allow(dead_code)]
pub(crate) enum Align {
    Left,
    Right,
}

/// A single table cell: plain text for width calculation + optional color.
pub(crate) struct Cell {
    text: String,
    color: Option<(u8, u8, u8)>,
}

impl Cell {
    /// Uncolored cell.
    pub(crate) fn plain(text: &str) -> Self {
        Self { text: text.to_string(), color: None }
    }

    /// Cell with an RGB color applied.
    pub(crate) fn colored(text: &str, color: (u8, u8, u8)) -> Self {
        Self { text: text.to_string(), color: Some(color) }
    }

    fn display_width(&self) -> usize {
        UnicodeWidthStr::width(self.text.as_str())
    }
}

/// Column definition.
struct Column {
    align: Align,
}

/// A table that auto-sizes columns and writes aligned, colored rows.
///
/// ```text
/// let mut table = TableWriter::new("  ")
///     .column(Align::Left)
///     .column(Align::Right);
/// table.row(vec![Cell::plain("hello"), Cell::colored("+1", (0,255,0))]);
/// table.print(&mut std::io::stdout().lock());
/// ```
pub(crate) struct TableWriter {
    columns: Vec<Column>,
    rows: Vec<Vec<Cell>>,
    indent: String,
    max_width: Option<usize>,
}

impl TableWriter {
    pub(crate) fn new(indent: &str) -> Self {
        Self { columns: Vec::new(), rows: Vec::new(), indent: indent.to_string(), max_width: None }
    }

    /// Add a column definition. Call once per column, in order.
    pub(crate) fn column(mut self, align: Align) -> Self {
        self.columns.push(Column { align });
        self
    }

    /// Set maximum total line width. Columns exceeding this are truncated.
    pub(crate) fn max_width(mut self, width: usize) -> Self {
        self.max_width = Some(width);
        self
    }

    /// Push a row of cells. Must have the same number of cells as columns.
    pub(crate) fn row(&mut self, cells: Vec<Cell>) {
        debug_assert_eq!(cells.len(), self.columns.len(), "cell count must match column count");
        self.rows.push(cells);
    }

    /// Compute per-column widths from the data.
    fn column_widths(&self) -> Vec<usize> {
        let mut widths = vec![0usize; self.columns.len()];
        for row in &self.rows {
            for (i, cell) in row.iter().enumerate() {
                widths[i] = widths[i].max(cell.display_width());
            }
        }

        // If max_width is set, shrink the widest column(s) to fit.
        if let Some(max) = self.max_width {
            let indent_width = UnicodeWidthStr::width(self.indent.as_str());
            // 2 chars gap between columns
            let gaps = if self.columns.is_empty() { 0 } else { (self.columns.len() - 1) * 2 };
            let total: usize = widths.iter().sum::<usize>() + indent_width + gaps;
            if total > max {
                let overflow = total - max;
                // Find the widest column and shrink it
                if let Some((widest_idx, widest)) =
                    widths.iter().enumerate().max_by_key(|(_, w)| **w)
                {
                    let min_col = 12; // never shrink below 12 chars
                    let new_width = (*widest).saturating_sub(overflow).max(min_col);
                    widths[widest_idx] = new_width;
                }
            }
        }

        widths
    }

    /// Write the table to a writer.
    pub(crate) fn print(&self, w: &mut impl Write) {
        let widths = self.column_widths();

        for row in &self.rows {
            let _ = write!(w, "{}", self.indent);
            for (i, cell) in row.iter().enumerate() {
                if i > 0 {
                    let _ = write!(w, "  ");
                }
                let width = widths.get(i).copied().unwrap_or(0);
                let text = if cell.display_width() > width {
                    // Truncate to fit column width
                    super::fmt::truncate_display(&cell.text, width.saturating_sub(1))
                } else {
                    cell.text.clone()
                };
                let align = self.columns.get(i).map(|c| c.align).unwrap_or(Align::Left);
                let formatted = match align {
                    Align::Left => format!("{:<width$}", text, width = width),
                    Align::Right => format!("{:>width$}", text, width = width),
                };
                match cell.color {
                    Some(color) => {
                        let _ = write!(w, "{}", formatted.custom_color(color));
                    }
                    None => {
                        let _ = write!(w, "{}", formatted);
                    }
                }
            }
            let _ = writeln!(w);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_alignment() {
        colored::control::set_override(false);
        let mut table = TableWriter::new("  ").column(Align::Left).column(Align::Right);
        table.row(vec![Cell::plain("a"), Cell::plain("1")]);
        table.row(vec![Cell::plain("bbb"), Cell::plain("22")]);

        let mut buf = Vec::new();
        table.print(&mut buf);
        let output = String::from_utf8(buf).unwrap();
        let lines: Vec<&str> = output.lines().collect();
        assert_eq!(lines[0], "  a     1");
        assert_eq!(lines[1], "  bbb  22");
    }

    #[test]
    fn colored_cells_align_correctly() {
        colored::control::set_override(false);
        let mut table = TableWriter::new("").column(Align::Left).column(Align::Left);
        table.row(vec![Cell::colored("short", (255, 0, 0)), Cell::plain("x")]);
        table.row(vec![Cell::colored("longer", (0, 255, 0)), Cell::plain("y")]);

        let mut buf = Vec::new();
        table.print(&mut buf);
        let output = String::from_utf8(buf).unwrap();
        let lines: Vec<&str> = output.lines().collect();
        // "short " should be padded to match "longer"
        assert_eq!(lines[0], "short   x");
        assert_eq!(lines[1], "longer  y");
    }

    #[test]
    fn max_width_truncates_widest_column() {
        colored::control::set_override(false);
        let mut table = TableWriter::new("")
            .column(Align::Left)
            .column(Align::Left)
            .max_width(25);
        // col0: 20 chars, col1: 5 chars → total 20+2+5 = 27, overflow = 2
        table.row(vec![Cell::plain("a]234567890123456789"), Cell::plain("hello")]);

        let mut buf = Vec::new();
        table.print(&mut buf);
        let output = String::from_utf8(buf).unwrap();
        // The widest column (col0) should be truncated to fit
        let first_line = output.lines().next().unwrap();
        let display_width = UnicodeWidthStr::width(first_line);
        assert!(display_width <= 25, "line too wide: {} ({})", first_line, display_width);
    }
}
