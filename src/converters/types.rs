#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum InputFormat {
    Int,
    Hex,
    HexBytes,
    Bytes,
    Text,
    Base64,
    Binary,
    Base58,
    Pubkey,
    Signature,
    Keypair,
    U8,
    U16,
    U32,
    U64,
    U128,
    I8,
    I16,
    I32,
    I64,
    I128,
    Lamports,
    Sol,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum OutputFormat {
    Int,
    Hex,
    HexBytes,
    Bytes,
    Text,
    Binary,
    Base64,
    Base58,
    Pubkey,
    Signature,
    U8,
    U16,
    U32,
    U64,
    U128,
    I8,
    I16,
    I32,
    I64,
    I128,
    Lamports,
    Sol,
}

#[derive(Clone, Debug)]
pub struct ConvertRequest {
    pub from: InputFormat,
    pub to: OutputFormat,
    pub input: Option<String>,
    pub le: bool,
    pub sep: String,
    pub no_prefix: bool,
    pub escape: bool,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum ByteFormat {
    Hex,
    HexBytes,
    Bytes,
}

#[derive(Clone, Debug)]
pub(crate) enum ConvertValue {
    Bytes(Vec<u8>),
    Number(num_bigint::BigUint),
    FixedUnsigned { value: u128, bits: u16 },
    FixedSigned { value: i128, bits: u16 },
    Lamports(u64),
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum FixedIntSpec {
    U8,
    U16,
    U32,
    U64,
    U128,
    I8,
    I16,
    I32,
    I64,
    I128,
}

impl FixedIntSpec {
    pub fn bits(self) -> u16 {
        match self {
            Self::U8 | Self::I8 => 8,
            Self::U16 | Self::I16 => 16,
            Self::U32 | Self::I32 => 32,
            Self::U64 | Self::I64 => 64,
            Self::U128 | Self::I128 => 128,
        }
    }

    pub fn bytes(self) -> usize {
        usize::from(self.bits() / 8)
    }

    pub fn is_signed(self) -> bool {
        matches!(self, Self::I8 | Self::I16 | Self::I32 | Self::I64 | Self::I128)
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::U8 => "u8",
            Self::U16 => "u16",
            Self::U32 => "u32",
            Self::U64 => "u64",
            Self::U128 => "u128",
            Self::I8 => "i8",
            Self::I16 => "i16",
            Self::I32 => "i32",
            Self::I64 => "i64",
            Self::I128 => "i128",
        }
    }
}

pub(crate) fn input_fixed_int_spec(format: InputFormat) -> Option<FixedIntSpec> {
    match format {
        InputFormat::U8 => Some(FixedIntSpec::U8),
        InputFormat::U16 => Some(FixedIntSpec::U16),
        InputFormat::U32 => Some(FixedIntSpec::U32),
        InputFormat::U64 => Some(FixedIntSpec::U64),
        InputFormat::U128 => Some(FixedIntSpec::U128),
        InputFormat::I8 => Some(FixedIntSpec::I8),
        InputFormat::I16 => Some(FixedIntSpec::I16),
        InputFormat::I32 => Some(FixedIntSpec::I32),
        InputFormat::I64 => Some(FixedIntSpec::I64),
        InputFormat::I128 => Some(FixedIntSpec::I128),
        _ => None,
    }
}

pub(crate) fn output_fixed_int_spec(format: OutputFormat) -> Option<FixedIntSpec> {
    match format {
        OutputFormat::U8 => Some(FixedIntSpec::U8),
        OutputFormat::U16 => Some(FixedIntSpec::U16),
        OutputFormat::U32 => Some(FixedIntSpec::U32),
        OutputFormat::U64 => Some(FixedIntSpec::U64),
        OutputFormat::U128 => Some(FixedIntSpec::U128),
        OutputFormat::I8 => Some(FixedIntSpec::I8),
        OutputFormat::I16 => Some(FixedIntSpec::I16),
        OutputFormat::I32 => Some(FixedIntSpec::I32),
        OutputFormat::I64 => Some(FixedIntSpec::I64),
        OutputFormat::I128 => Some(FixedIntSpec::I128),
        _ => None,
    }
}
