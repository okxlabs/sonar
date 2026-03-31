use anyhow::Result;
use clap::ValueEnum;

use crate::cli::{ConvertArgs, ConvertOutputFormat};

#[derive(serde::Serialize)]
struct ConvertOutput {
    input: String,
    output: String,
    from: String,
    to: String,
}

pub(crate) fn handle(mut args: ConvertArgs, json: bool) -> Result<()> {
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

    // Eagerly resolve stdin so the input string is available for JSON output
    // and is not consumed twice.
    let input_str = crate::utils::read_cli_input(args.input.as_deref(), "input")
        .map_err(|e| anyhow::anyhow!("{}", e))?;
    if args.input.is_none() {
        args.input = Some(input_str.clone());
    }

    let output = crate::cli::convert(&args).map_err(|e| anyhow::anyhow!("{}", e))?;

    if json {
        let from_name = args.from.to_possible_value().unwrap().get_name().to_string();
        let to_name = args.to.to_possible_value().unwrap().get_name().to_string();
        let json_output = ConvertOutput { input: input_str, output, from: from_name, to: to_name };
        println!("{}", serde_json::to_string_pretty(&json_output)?);
    } else {
        println!("{}", output);
    }
    Ok(())
}
