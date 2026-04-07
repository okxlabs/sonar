//! Error types for sonar-idl.

/// Result type alias for sonar-idl operations.
pub type Result<T> = std::result::Result<T, IdlError>;

/// Errors that can occur when working with IDL data.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum IdlError {
    /// Unknown primitive type encountered during parsing.
    #[error("unknown primitive type: {0}")]
    UnknownPrimitiveType(String),

    /// Insufficient data to parse a value.
    #[error("insufficient data: need {required} bytes at offset {offset}, have {available}")]
    InsufficientData { required: usize, offset: usize, available: usize },

    /// Invalid discriminator length (must be 8 bytes).
    #[error("invalid discriminator length: expected 8, got {0}")]
    InvalidDiscriminatorLength(usize),

    /// Unknown type definition referenced.
    #[error("unknown type definition: {0}")]
    UnknownTypeDefinition(String),

    /// Invalid UTF-8 in string data.
    #[error("invalid UTF-8 in string data")]
    InvalidUtf8,

    /// Invalid pubkey data.
    #[error("invalid pubkey data")]
    InvalidPubkey,

    /// JSON deserialization error.
    #[error("JSON deserialization error: {0}")]
    JsonError(#[from] serde_json::Error),

    /// Generic parsing error.
    #[error("parse error: {0}")]
    ParseError(String),
}

impl IdlError {
    /// Create an insufficient data error.
    pub fn insufficient_data(required: usize, offset: usize, available: usize) -> Self {
        Self::InsufficientData { required, offset, available }
    }
}
