use thiserror::Error;

#[derive(Error, Debug)]
pub enum WalletError {
    #[error("Database error: {0}")]
    DatabaseError(#[from] rusqlite::Error),

    #[error("BDK wallet error: {0}")]
    BdkError(#[from] Box<bdk_wallet::CreateWithPersistError<rusqlite::Error>>),

    #[error("BDK wallet load error: {0}")]
    BdkLoadError(#[from] Box<bdk_wallet::LoadWithPersistError<rusqlite::Error>>),

    #[error("Bitcoin Core RPC error: {0}")]
    RpcError(#[from] bitcoincore_rpc::Error),

    #[error("Bitcoin address error: {0}")]
    AddressError(#[from] bitcoin::address::ParseError),

    #[error("Bitcoin amount error: {0}")]
    AmountError(#[from] bitcoin::amount::ParseAmountError),

    #[error("Descriptor error: {0}")]
    DescriptorError(String),

    #[error("Transaction error: {0}")]
    TransactionError(String),

    #[error("Insufficient funds: available {available}, required {required}")]
    InsufficientFunds { available: u64, required: u64 },

    #[error("Configuration error: {0}")]
    ConfigError(String),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Invalid network: {0}")]
    InvalidNetwork(String),

    #[error("Wallet not found at path: {0}")]
    WalletNotFound(String),

    #[error("Invalid mnemonic: {0}")]
    InvalidMnemonic(String),

    #[error("Persistence error: {0}")]
    PersistenceError(String),
}

pub type Result<T> = std::result::Result<T, WalletError>;
