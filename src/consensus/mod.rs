//! Consensus module for blockchain validation and monetary policy

pub mod validation;
pub mod monetary;

pub use validation::BlockValidator;
pub use monetary::MonetaryPolicy;
