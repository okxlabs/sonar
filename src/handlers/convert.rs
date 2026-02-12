use anyhow::Result;

use crate::cli::ConvertArgs;

pub(crate) fn handle(args: ConvertArgs) -> Result<()> {
    let output = crate::cli::convert(&args).map_err(|e| anyhow::anyhow!("{}", e))?;
    println!("{}", output);
    Ok(())
}
