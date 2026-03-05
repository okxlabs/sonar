use clap::{Args, Subcommand, ValueEnum};

#[derive(Args, Debug)]
pub struct BorshArgs {
    #[command(subcommand)]
    pub command: BorshCommands,
}

#[derive(Subcommand, Debug)]
pub enum BorshCommands {
    /// Deserialize Borsh-encoded bytes into JSON using a type descriptor
    De(BorshDeArgs),
    /// Serialize a JSON value into Borsh-encoded bytes using a type descriptor
    Ser(BorshSerArgs),
}

#[derive(Args, Debug)]
#[command(after_help = "\
EXAMPLES:
  sonar borsh de \"u64\" 0x0100000000000000
  sonar borsh de \"(u64,bool)\" 0x010000000000000001
  sonar borsh de \"vec<u32>\" 0x020000000100000002000000
  echo '0x0100000000000000' | sonar borsh der \"u64\"
")]
pub struct BorshDeArgs {
    /// Borsh type descriptor (e.g. \"u64\", \"(u64,bool,vec<u32>)\", \"[u8;32]\")
    pub type_str: String,
    /// Input bytes (hex with 0x prefix, base64, or byte array). Reads from stdin if omitted.
    pub input: Option<String>,
}

#[derive(Args, Debug)]
#[command(after_help = "\
EXAMPLES:
  sonar borsh ser \"u64\" '1'
  sonar borsh ser \"(u64,bool)\" '[1,true]'
  sonar borsh ser \"string\" '\"hello\"'
  echo '[1,true]' | sonar borsh ser \"(u64,bool)\"
")]
pub struct BorshSerArgs {
    /// Borsh type descriptor (e.g. \"u64\", \"(u64,bool,vec<u32>)\", \"[u8;32]\")
    pub type_str: String,
    /// JSON value to serialize. Reads from stdin if omitted.
    pub input: Option<String>,
    /// Output format for serialized bytes
    #[arg(long, value_enum, default_value = "hex")]
    pub output: SerOutputFormat,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum SerOutputFormat {
    /// Hex string with 0x prefix (default)
    Hex,
    /// Base64 encoded
    Base64,
    /// Decimal byte array [1,2,3,...]
    Bytes,
}
