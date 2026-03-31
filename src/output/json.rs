use anyhow::Result;

use super::report::{BundleReport, Report};

/// Pretty-print any serializable value as JSON to stdout.
pub(crate) fn print_json<T: serde::Serialize + ?Sized>(value: &T) -> Result<()> {
    let json = serde_json::to_string_pretty(value)?;
    println!("{json}");
    Ok(())
}

pub(super) fn render_json(report: &Report) -> Result<()> {
    print_json(report)
}

pub(super) fn render_bundle_json(bundle: &BundleReport) -> Result<()> {
    print_json(bundle)
}

/// Render a slice of serializable values as a single JSON array.
pub(super) fn render_json_array<T: serde::Serialize>(items: &[T]) -> Result<()> {
    print_json(items)
}
