use anyhow::Result;

use super::report::{BundleReport, Report};

pub(super) fn render_json(report: &Report) -> Result<()> {
    let json = serde_json::to_string_pretty(report)?;
    println!("{json}");
    Ok(())
}

pub(super) fn render_bundle_json(bundle: &BundleReport) -> Result<()> {
    let json = serde_json::to_string_pretty(bundle)?;
    println!("{json}");
    Ok(())
}
