//! Mining module for Quantum Goldchain

pub mod randomx;
pub mod miner;
pub mod difficulty;

pub use randomx::{RandomXHash, RandomXMiner};
pub use miner::{Miner, MiningResult, MiningStats};
pub use difficulty::{DifficultyCalculator, DifficultyTarget};
