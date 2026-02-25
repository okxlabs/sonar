use anyhow::Result;

use crate::cli::{ConvertArgs, ConvertOutputFormat};

pub(crate) fn handle(args: ConvertArgs) -> Result<()> {
    if args.escape && args.to != ConvertOutputFormat::Text {
        log::warn!("--escape has no effect when output format is not 'text'");
    }
    if args.no_prefix && args.to != ConvertOutputFormat::HexBytes {
        log::warn!("--no-prefix has no effect when output format is not 'hex-bytes'");
    }
    if args.sep != ","
        && !matches!(args.to, ConvertOutputFormat::HexBytes | ConvertOutputFormat::Bytes)
    {
        log::warn!("--sep has no effect when output format is not 'hex-bytes' or 'bytes'");
    }

    let output = crate::cli::convert(&args).map_err(|e| anyhow::anyhow!("{}", e))?;
    println!("{}", output);
    Ok(())
}
