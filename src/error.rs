use thiserror::Error;

pub type Result<T> = std::result::Result<T, QtcError>;

#[derive(Error, Debug)]
pub enum QtcError {
    #[error("Blockchain error: {0}")]
    Blockchain(String),
    
    #[error("Transaction error: {0}")]
    Transaction(String),
    
    #[error("Crypto error: {0}")]
    Crypto(String),
    
    #[error("Wallet error: {0}")]
    Wallet(String),
    
    #[error("Mining error: {0}")]
    Mining(String),
    
    #[error("Network error: {0}")]
    Network(String),
    
    #[error("Storage error: {0}")]
    Storage(String),
    
    #[error("Consensus error: {0}")]
    Consensus(String),
    
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
    
    #[error("Database error: {0}")]
    Database(#[from] sled::Error),
    
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    
    #[error("Invalid input: {0}")]
    InvalidInput(String),
    
    #[error("Insufficient funds: required {required}, available {available}")]
    InsufficientFunds { required: u64, available: u64 },
    
    #[error("Double spend detected for transaction: {0}")]
    DoubleSpend(String),
    
    #[error("Invalid signature")]
    InvalidSignature,
    
    #[error("Invalid block hash")]
    InvalidBlockHash,
    
    #[error("Invalid difficulty target")]
    InvalidDifficulty,
    
    #[error("Multisig error: {0}")]
    Multisig(String),
    
    #[error("P2P connection denied")]
    ConnectionDenied,
}

impl From<libp2p::swarm::ConnectionDenied> for QtcError {
    fn from(_: libp2p::swarm::ConnectionDenied) -> Self {
        QtcError::ConnectionDenied
    }
}

impl From<std::convert::Infallible> for QtcError {
    fn from(_: std::convert::Infallible) -> Self {
        // This should never happen since Infallible represents impossible errors
        unreachable!("Infallible errors should never occur")
    }
}



impl From<bitcoin::bip32::Error> for QtcError {
    fn from(err: bitcoin::bip32::Error) -> Self {
        QtcError::Crypto(format!("BIP32 error: {}", err))
    }
}
