//! Quantum Goldchain (QTC) - A decentralized cryptocurrency with RandomX mining
//! 
//! This library implements a complete blockchain system with:
//! - RandomX CPU mining (ASIC-resistant)
//! - UTXO-based transaction model
//! - BIP39 HD wallets with multi-signature support
//! - P2P networking
//! - Complete CLI interface
//! - REST API and WebSocket endpoints

pub mod core;
pub mod crypto;
pub mod wallet;
pub mod mining;
pub mod network;
pub mod storage;
pub mod cli;
pub mod api;
pub mod consensus;
pub mod error;
pub mod config;

pub use error::{QtcError, Result};
