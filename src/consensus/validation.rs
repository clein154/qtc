use crate::core::{Block, Transaction, Blockchain};
use crate::crypto::hash::{Hash256, Hashable};
use crate::{QtcError, Result};
use std::collections::HashSet;

#[derive(Debug, Clone)]
pub struct BlockValidator {
    max_block_size: usize,
    max_transaction_size: usize,
    min_transaction_fee: u64,
    max_coinbase_value: u64,
}

impl BlockValidator {
    pub fn new() -> Self {
        Self {
            max_block_size: 1024 * 1024,    // 1MB
            max_transaction_size: 100_000,   // 100KB
            min_transaction_fee: 1000,       // 0.00001 QTC
            max_coinbase_value: 2710000000,  // 27.1 QTC
        }
    }
    
    pub fn with_params(
        max_block_size: usize,
        max_transaction_size: usize,
        min_transaction_fee: u64,
        max_coinbase_value: u64,
    ) -> Self {
        Self {
            max_block_size,
            max_transaction_size,
            min_transaction_fee,
            max_coinbase_value,
        }
    }
    
    /// Validate a complete block including all transactions
    pub fn validate_block(&self, block: &Block, blockchain: &Blockchain) -> Result<()> {
        log::debug!("Validating block at height {}", block.header.height);
        
        // Basic block structure validation
        self.validate_block_structure(block)?;
        
        // Block header validation
        self.validate_block_header(block, blockchain)?;
        
        // Transaction validation
        self.validate_block_transactions(block, blockchain)?;
        
        // Merkle root validation
        self.validate_merkle_root(block)?;
        
        // Coinbase validation
        self.validate_coinbase_transaction(block, blockchain)?;
        
        // Block size validation
        self.validate_block_size(block)?;
        
        log::debug!("✅ Block {} validation successful", block.header.height);
        Ok(())
    }
    
    /// Validate block structure and basic requirements
    fn validate_block_structure(&self, block: &Block) -> Result<()> {
        // Must have at least one transaction (coinbase)
        if block.transactions.is_empty() {
            return Err(QtcError::Consensus("Block must contain at least one transaction".to_string()));
        }
        
        // First transaction must be coinbase
        if !block.transactions[0].is_coinbase() {
            return Err(QtcError::Consensus("First transaction must be coinbase".to_string()));
        }
        
        // Only first transaction can be coinbase
        for (i, tx) in block.transactions.iter().enumerate().skip(1) {
            if tx.is_coinbase() {
                return Err(QtcError::Consensus(format!("Non-first transaction {} is coinbase", i)));
            }
        }
        
        Ok(())
    }
    
    /// Validate block header fields
    fn validate_block_header(&self, block: &Block, blockchain: &Blockchain) -> Result<()> {
        let header = &block.header;
        
        // Height validation
        let expected_height = blockchain.height + 1;
        if header.height != expected_height {
            return Err(QtcError::Consensus(format!(
                "Invalid block height: expected {}, got {}",
                expected_height, header.height
            )));
        }
        
        // Previous hash validation
        if header.previous_hash != blockchain.tip {
            return Err(QtcError::Consensus("Invalid previous block hash".to_string()));
        }
        
        // Timestamp validation
        let now = chrono::Utc::now().timestamp() as u64;
        let max_future_time = 2 * 60 * 60; // 2 hours
        
        if header.timestamp > now + max_future_time {
            return Err(QtcError::Consensus("Block timestamp too far in the future".to_string()));
        }
        
        // Minimum timestamp (greater than median of last 11 blocks)
        if header.height > 11 {
            let mut recent_timestamps = Vec::new();
            for i in (header.height.saturating_sub(11))..header.height {
                if let Ok(Some(prev_block)) = blockchain.get_block_by_height(i) {
                    recent_timestamps.push(prev_block.header.timestamp);
                }
            }
            
            if recent_timestamps.len() >= 11 {
                recent_timestamps.sort();
                let median = recent_timestamps[5]; // Middle element
                
                if header.timestamp <= median {
                    return Err(QtcError::Consensus("Block timestamp too old".to_string()));
                }
            }
        }
        
        // Difficulty validation
        let expected_difficulty = blockchain.calculate_next_difficulty(header.height)?;
        if header.difficulty != expected_difficulty {
            return Err(QtcError::Consensus(format!(
                "Invalid block difficulty: expected {}, got {}",
                expected_difficulty, header.difficulty
            )));
        }
        
        Ok(())
    }
    
    /// Validate all transactions in the block
    fn validate_block_transactions(&self, block: &Block, blockchain: &Blockchain) -> Result<()> {
        let mut seen_txids = HashSet::new();
        let mut total_fees = 0u64;
        
        // Skip coinbase transaction (index 0) for most validations
        for (i, tx) in block.transactions.iter().enumerate() {
            // Check for duplicate transactions
            let txid = tx.hash();
            if seen_txids.contains(&txid) {
                return Err(QtcError::Consensus(format!("Duplicate transaction in block: {}", hex::encode(txid.as_bytes()))));
            }
            seen_txids.insert(txid);
            
            // Validate individual transaction
            if i == 0 {
                // Coinbase transaction - different validation
                self.validate_coinbase_structure(&tx)?;
            } else {
                // Regular transaction
                self.validate_transaction(tx, blockchain)?;
                total_fees += tx.fee();
            }
            
            // Transaction size limit
            if tx.size() > self.max_transaction_size {
                return Err(QtcError::Consensus(format!(
                    "Transaction size {} exceeds maximum {}",
                    tx.size(), self.max_transaction_size
                )));
            }
        }
        
        // Validate total fees don't exceed coinbase output value
        let coinbase_value = block.transactions[0].total_output_value();
        let expected_reward = crate::consensus::monetary::MonetaryPolicy::new().coinbase_reward(block.header.height);
        
        if coinbase_value > expected_reward + total_fees {
            return Err(QtcError::Consensus("Coinbase value exceeds allowed amount".to_string()));
        }
        
        Ok(())
    }
    
    /// Validate a single transaction
    pub fn validate_transaction(&self, tx: &Transaction, blockchain: &Blockchain) -> Result<bool> {
        // Basic structure validation
        if tx.inputs.is_empty() {
            return Err(QtcError::Transaction("Transaction has no inputs".to_string()));
        }
        
        if tx.outputs.is_empty() {
            return Err(QtcError::Transaction("Transaction has no outputs".to_string()));
        }
        
        // Coinbase transactions should not be validated here
        if tx.is_coinbase() {
            return Err(QtcError::Transaction("Coinbase transaction in regular validation".to_string()));
        }
        
        // Check for duplicate inputs within transaction
        let mut seen_outpoints = HashSet::new();
        for input in &tx.inputs {
            let outpoint = &input.previous_output;
            if seen_outpoints.contains(outpoint) {
                return Err(QtcError::Transaction("Duplicate inputs in transaction".to_string()));
            }
            seen_outpoints.insert(outpoint.clone());
        }
        
        // Validate inputs exist and are unspent
        let mut total_input_value = 0u64;
        for input in &tx.inputs {
            // Check if UTXO exists
            let utxo_set = blockchain.utxo_set.read().unwrap();
            
            match utxo_set.get_utxo(&input.previous_output)? {
                Some(utxo) => {
                    total_input_value = total_input_value.saturating_add(utxo.value);
                    
                    // Validate coinbase maturity
                    if utxo.is_coinbase {
                        let current_height = blockchain.height;
                        let coinbase_maturity = 100; // 100 block maturity for coinbase
                        
                        if current_height < utxo.height + coinbase_maturity {
                            return Err(QtcError::Transaction(
                                "Coinbase UTXO not yet mature".to_string()
                            ));
                        }
                    }
                    
                    // TODO: Validate signature against UTXO script
                    // This would require implementing script validation
                }
                None => {
                    return Err(QtcError::Transaction(format!(
                        "Referenced UTXO not found: {}:{}",
                        hex::encode(input.previous_output.txid.as_bytes()),
                        input.previous_output.vout
                    )));
                }
            }
        }
        
        // Validate outputs
        let total_output_value = tx.total_output_value();
        
        // Check for negative or zero outputs
        for output in &tx.outputs {
            if output.value == 0 {
                return Err(QtcError::Transaction("Transaction output value is zero".to_string()));
            }
            
            // Check for dust outputs (very small values)
            let dust_threshold = 546; // satoshis
            if output.value < dust_threshold {
                return Err(QtcError::Transaction("Transaction output below dust threshold".to_string()));
            }
        }
        
        // Validate input value >= output value (with fee)
        if total_input_value < total_output_value {
            return Err(QtcError::Transaction("Total input value less than total output value".to_string()));
        }
        
        // Validate minimum fee
        let fee = total_input_value - total_output_value;
        if fee < self.min_transaction_fee {
            return Err(QtcError::Transaction(format!(
                "Transaction fee {} below minimum {}",
                fee, self.min_transaction_fee
            )));
        }
        
        // Validate fee is reasonable (not excessive)
        let max_fee = total_output_value; // Fee shouldn't exceed output value
        if fee > max_fee {
            return Err(QtcError::Transaction("Transaction fee is excessive".to_string()));
        }
        
        Ok(true)
    }
    
    /// Validate coinbase transaction structure
    fn validate_coinbase_structure(&self, tx: &Transaction) -> Result<()> {
        if !tx.is_coinbase() {
            return Err(QtcError::Consensus("Not a coinbase transaction".to_string()));
        }
        
        // Coinbase should have exactly one input
        if tx.inputs.len() != 1 {
            return Err(QtcError::Consensus("Coinbase transaction must have exactly one input".to_string()));
        }
        
        // Coinbase input validation
        let input = &tx.inputs[0];
        if !input.previous_output.is_null() {
            return Err(QtcError::Consensus("Coinbase input must reference null outpoint".to_string()));
        }
        
        // Coinbase script size limits
        if input.signature_script.len() < 2 || input.signature_script.len() > 100 {
            return Err(QtcError::Consensus("Coinbase script size invalid".to_string()));
        }
        
        Ok(())
    }
    
    /// Validate coinbase transaction value and outputs
    fn validate_coinbase_transaction(&self, block: &Block, _blockchain: &Blockchain) -> Result<()> {
        let coinbase = &block.transactions[0];
        let monetary_policy = crate::consensus::monetary::MonetaryPolicy::new();
        
        // Calculate expected reward
        let block_reward = monetary_policy.coinbase_reward(block.header.height);
        let total_fees: u64 = block.transactions.iter().skip(1).map(|tx| tx.fee()).sum();
        let expected_value = block_reward + total_fees;
        
        // Validate coinbase output value
        let coinbase_value = coinbase.total_output_value();
        if coinbase_value > expected_value {
            return Err(QtcError::Consensus(format!(
                "Coinbase value {} exceeds allowed {}",
                coinbase_value, expected_value
            )));
        }
        
        // Coinbase can have multiple outputs (e.g., to pool members)
        if coinbase.outputs.is_empty() {
            return Err(QtcError::Consensus("Coinbase transaction must have outputs".to_string()));
        }
        
        // Validate individual output values
        for output in &coinbase.outputs {
            if output.value == 0 {
                return Err(QtcError::Consensus("Coinbase output value cannot be zero".to_string()));
            }
        }
        
        Ok(())
    }
    
    /// Validate block's merkle root
    fn validate_merkle_root(&self, block: &Block) -> Result<()> {
        let calculated_root = Block::calculate_merkle_root(&block.transactions);
        
        if calculated_root != block.header.merkle_root {
            return Err(QtcError::Consensus("Invalid merkle root".to_string()));
        }
        
        Ok(())
    }
    
    /// Validate block size
    fn validate_block_size(&self, block: &Block) -> Result<()> {
        let block_size = block.size();
        
        if block_size > self.max_block_size {
            return Err(QtcError::Consensus(format!(
                "Block size {} exceeds maximum {}",
                block_size, self.max_block_size
            )));
        }
        
        Ok(())
    }
    
    /// Validate proof of work
    pub fn validate_proof_of_work(&self, block: &Block) -> Result<()> {
        let hash = block.hash();
        let difficulty = block.header.difficulty;
        
        // Check if hash meets difficulty target
        let required_zeros = difficulty / 4; // 4 bits per hex digit
        let remaining_bits = difficulty % 4;
        
        // Check full zero bytes
        for i in 0..required_zeros as usize {
            if i >= 32 || hash.as_bytes()[i] != 0 {
                return Err(QtcError::Consensus("Block hash does not meet difficulty target".to_string()));
            }
        }
        
        // Check remaining bits
        if remaining_bits > 0 {
            let byte_index = required_zeros as usize;
            if byte_index < 32 {
                let mask = 0xFF << (8 - remaining_bits);
                if hash.as_bytes()[byte_index] & mask != 0 {
                    return Err(QtcError::Consensus("Block hash does not meet difficulty target".to_string()));
                }
            }
        }
        
        Ok(())
    }
    
    /// Validate chain of blocks (for initial sync or verification)
    pub fn validate_chain(&self, blocks: &[Block], start_height: u64) -> Result<()> {
        log::info!("Validating chain of {} blocks starting at height {}", blocks.len(), start_height);
        
        for (i, block) in blocks.iter().enumerate() {
            let expected_height = start_height + i as u64;
            
            if block.header.height != expected_height {
                return Err(QtcError::Consensus(format!(
                    "Block height mismatch: expected {}, got {}",
                    expected_height, block.header.height
                )));
            }
            
            // Validate proof of work
            self.validate_proof_of_work(block)?;
            
            // Validate block structure
            self.validate_block_structure(block)?;
            
            // Validate merkle root
            self.validate_merkle_root(block)?;
            
            // Check previous hash linkage
            if i > 0 {
                let prev_block = &blocks[i - 1];
                if block.header.previous_hash != prev_block.hash() {
                    return Err(QtcError::Consensus("Invalid previous block hash in chain".to_string()));
                }
            }
        }
        
        log::info!("✅ Chain validation successful");
        Ok(())
    }
    
    /// Check if a transaction is final (can be included in a block)
    pub fn is_transaction_final(&self, tx: &Transaction, height: u64, time: u64) -> bool {
        // BIP68: Relative lock-time using consensus-enforced sequence numbers
        
        // Transaction is final if lock_time is 0
        if tx.lock_time == 0 {
            return true;
        }
        
        // If lock_time < 500000000, it's a block height
        if tx.lock_time < 500_000_000 {
            return tx.lock_time <= height;
        } else {
            // Otherwise it's a timestamp
            return tx.lock_time <= time;
        }
    }
    
    /// Get validation configuration
    pub fn get_config(&self) -> (usize, usize, u64, u64) {
        (
            self.max_block_size,
            self.max_transaction_size,
            self.min_transaction_fee,
            self.max_coinbase_value,
        )
    }
    
    /// Update validation parameters
    pub fn update_config(
        &mut self,
        max_block_size: Option<usize>,
        max_transaction_size: Option<usize>,
        min_transaction_fee: Option<u64>,
        max_coinbase_value: Option<u64>,
    ) {
        if let Some(size) = max_block_size {
            self.max_block_size = size;
        }
        if let Some(size) = max_transaction_size {
            self.max_transaction_size = size;
        }
        if let Some(fee) = min_transaction_fee {
            self.min_transaction_fee = fee;
        }
        if let Some(value) = max_coinbase_value {
            self.max_coinbase_value = value;
        }
    }
}

impl Default for BlockValidator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::Database;
    use tempfile::TempDir;
    
    #[test]
    fn test_validator_creation() {
        let validator = BlockValidator::new();
        assert_eq!(validator.max_block_size, 1024 * 1024);
        assert_eq!(validator.min_transaction_fee, 1000);
    }
    
    #[test]
    fn test_coinbase_structure_validation() -> Result<()> {
        let validator = BlockValidator::new();
        
        // Valid coinbase
        let coinbase = Transaction::new_coinbase(
            "qtc1test".to_string(),
            2710000000,
            "test coinbase".to_string(),
        );
        
        validator.validate_coinbase_structure(&coinbase)?;
        
        Ok(())
    }
    
    #[test]
    fn test_merkle_root_validation() -> Result<()> {
        let validator = BlockValidator::new();
        
        let coinbase = Transaction::new_coinbase(
            "qtc1test".to_string(),
            2710000000,
            "test".to_string(),
        );
        
        let block = Block::new(Hash256::zero(), vec![coinbase], 4, 0);
        
        validator.validate_merkle_root(&block)?;
        
        Ok(())
    }
    
    #[test]
    fn test_block_size_validation() -> Result<()> {
        let validator = BlockValidator::new();
        
        let coinbase = Transaction::new_coinbase(
            "qtc1test".to_string(),
            2710000000,
            "test".to_string(),
        );
        
        let block = Block::new(Hash256::zero(), vec![coinbase], 4, 0);
        
        validator.validate_block_size(&block)?;
        
        Ok(())
    }
}
