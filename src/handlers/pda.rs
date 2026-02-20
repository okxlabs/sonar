use std::str::FromStr;

use anyhow::{Context, Result};
use solana_pubkey::Pubkey;

use crate::cli::PdaArgs;

pub(crate) fn handle(args: PdaArgs) -> Result<()> {
    let program_id = Pubkey::from_str(&args.program_id)
        .with_context(|| format!("Invalid program ID: {}", args.program_id))?;

    let parsed_seeds = crate::cli::parse_seeds(&args.seeds)
        .map_err(|e| anyhow::anyhow!("Failed to parse seeds: {}", e))?;

    let seed_bytes = crate::cli::seeds_to_bytes(&parsed_seeds)
        .map_err(|e| anyhow::anyhow!("Failed to convert seeds to bytes: {}", e))?;

    // Convert Vec<Vec<u8>> to Vec<&[u8]> for find_program_address
    let seed_slices: Vec<&[u8]> = seed_bytes.iter().map(|v| v.as_slice()).collect();

    let (pda, bump) = Pubkey::find_program_address(&seed_slices, &program_id);

    println!("{pda} {bump}");

    Ok(())
}
