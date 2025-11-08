use std::{path::PathBuf, str::FromStr};

use clap::{Args, Parser, Subcommand, ValueEnum};
use solana_sdk::pubkey::Pubkey;

#[derive(Parser, Debug)]
#[command(name = "solsim", version, about = "基于 LiteSVM 的 Solana 交易模拟器")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// 模拟指定原始交易
    Simulate(SimulateArgs),
}

#[derive(Args, Debug)]
pub struct SimulateArgs {
    /// 原始交易字符串（Base58/Base64），与 --tx-file 互斥
    #[arg(short = 't', long, conflicts_with = "tx_file", value_name = "STRING")]
    pub tx: Option<String>,
    /// 包含原始交易的文件路径，与 --tx 互斥
    #[arg(long = "tx-file", value_name = "PATH", conflicts_with = "tx")]
    pub tx_file: Option<PathBuf>,
    /// Solana RPC 节点地址
    #[arg(
        long = "rpc-url",
        default_value = "https://api.mainnet-beta.solana.com"
    )]
    pub rpc_url: String,
    /// 自定义替换程序，格式：<PROGRAM_ID>=<PATH_TO_ELF_OR_SO>
    #[arg(
        long = "replace",
        value_name = "MAPPING",
        value_parser = clap::builder::NonEmptyStringValueParser::new()
    )]
    pub replacements: Vec<String>,
    /// 输出格式
    #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
    pub output: OutputFormat,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, ValueEnum, Default)]
pub enum OutputFormat {
    #[default]
    Text,
    Json,
}

#[derive(Clone, Debug)]
pub struct ProgramReplacement {
    pub program_id: Pubkey,
    pub so_path: PathBuf,
}

pub fn parse_program_replacement(raw: &str) -> Result<ProgramReplacement, String> {
    let (program_str, path_str) = raw
        .split_once('=')
        .ok_or_else(|| "替换项必须采用 <PROGRAM_ID>=<PATH> 格式".to_string())?;
    let program_id = Pubkey::from_str(program_str)
        .map_err(|err| format!("无法解析程序地址 `{program_str}`: {err}"))?;
    let so_path = PathBuf::from(path_str.trim());
    if !so_path.exists() {
        return Err(format!("指定的程序文件 `{}` 不存在", so_path.display()));
    }
    Ok(ProgramReplacement {
        program_id,
        so_path,
    })
}
