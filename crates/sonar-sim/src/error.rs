use solana_pubkey::Pubkey;

pub type Result<T> = std::result::Result<T, SonarSimError>;

#[derive(Debug, thiserror::Error)]
pub enum SonarSimError {
    /// Network / RPC communication failures.
    #[error("{0}")]
    Rpc(Box<dyn std::error::Error + Send + Sync>),

    /// Account not present in expected store (SVM, cache, resolved set).
    #[error("Account not found: {0}")]
    AccountNotFound(Pubkey),

    /// Account data format / validation issues (wrong owner, bad length, etc.).
    #[error("{0}")]
    AccountData(String),

    /// Raw transaction decoding or parsing failures.
    #[error("{0}")]
    TransactionParse(String),

    /// SPL Token / Token-2022 operation failures.
    #[error("{0}")]
    Token(String),

    /// Address lookup table resolution failures.
    #[error("{0}")]
    LookupTable(String),

    /// LiteSVM engine errors.
    #[error("{0}")]
    Svm(String),

    /// Serialization / deserialization failures (bincode, etc.).
    #[error("{0}")]
    Serialization(String),

    /// User-supplied parameter validation errors.
    #[error("{0}")]
    Validation(String),

    /// Mutex poisoning or other unexpected internal failures.
    #[error("Internal error: {0}")]
    Internal(String),
}

impl From<bincode::Error> for SonarSimError {
    fn from(err: bincode::Error) -> Self {
        Self::Serialization(err.to_string())
    }
}
