//! Core blockchain components

pub mod blockchain;
pub mod block;
pub mod transaction;
pub mod utxo;

pub use blockchain::Blockchain;
pub use block::{Block, BlockHeader};
pub use transaction::{Transaction, TxInput, TxOutput};
pub use utxo::{UtxoSet, UtxoEntry};
