use solana_pubkey::Pubkey;

pub type Result<T> = std::result::Result<T, SonarSimError>;

#[derive(Debug, thiserror::Error)]
pub enum SonarSimError {
    /// Network / RPC communication failures.
    ///
    /// The underlying transport error (e.g. `solana_client::ClientError`) is
    /// stringified into `message` so that the public API does not expose
    /// third-party client types.
    #[error("RPC error: {reason}")]
    Rpc { reason: String },

    /// Account not present in expected store (SVM, cache, resolved set).
    #[error("Account not found: {pubkey}")]
    AccountNotFound { pubkey: Pubkey },

    /// Account data format / validation issues (wrong owner, bad length, etc.).
    ///
    /// `pubkey` is populated when the error is attributable to a single account.
    #[error("{reason}")]
    AccountData { pubkey: Option<Pubkey>, reason: String },

    /// Raw transaction decoding or parsing failures.
    #[error("{reason}")]
    TransactionParse { reason: String },

    /// SPL Token / Token-2022 operation failures.
    ///
    /// `account` is populated when the error relates to a specific token/mint account.
    #[error("{reason}")]
    Token { account: Option<Pubkey>, reason: String },

    /// Address lookup table resolution failures.
    ///
    /// `table` is populated when a specific lookup table address is known.
    #[error("{reason}")]
    LookupTable { table: Option<Pubkey>, reason: String },

    /// SVM engine errors.
    #[error("{reason}")]
    Svm { reason: String },

    /// Serialization / deserialization failures (bincode, etc.).
    #[error("{reason}")]
    Serialization { reason: String },

    /// User-supplied parameter validation errors.
    #[error("{reason}")]
    Validation { reason: String },

    /// Unexpected internal failures.
    #[error("Internal error: {reason}")]
    Internal { reason: String },
}

impl From<bincode::Error> for SonarSimError {
    fn from(err: bincode::Error) -> Self {
        Self::Serialization { reason: err.to_string() }
    }
}
