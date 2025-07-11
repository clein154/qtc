use crate::core::{Block, Transaction};
use crate::core::utxo::UtxoSet;
use crate::storage::Database;
use crate::consensus::validation::BlockValidator;
use crate::consensus::monetary::MonetaryPolicy;
use crate::crypto::hash::{Hash256, Hashable};
use crate::{QtcError, Result};
use serde::{Deserialize, Serialize};
// use chrono::{DateTime, Utc};
use std::sync::{Arc, RwLock};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockchainStats {
    pub height: u64,
    pub difficulty: u32,
    pub total_supply: u64,
    pub total_addresses: usize,
    pub avg_block_time: u64,
    pub network_hashrate: f64,
}

#[derive(Debug, Clone)]
pub struct Blockchain {
    pub tip: Hash256,
    pub height: u64,
    db: Arc<Database>,
    pub utxo_set: Arc<RwLock<UtxoSet>>,
    validator: BlockValidator,
    monetary_policy: MonetaryPolicy,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ChainState {
    pub tip: Hash256,
    pub height: u64,
    pub total_work: u128,
    pub difficulty: u32,
    pub total_supply: u64,
}

impl Blockchain {
    pub fn new(db: Arc<Database>) -> Result<Self> {
        let utxo_set = Arc::new(RwLock::new(UtxoSet::new(db.clone())));
        let validator = BlockValidator::new();
        let monetary_policy = MonetaryPolicy::new();
        
        // Try to load existing blockchain
        if let Ok(state) = db.get_chain_state() {
            if let Some(chain_state) = state {
                Ok(Self {
                    tip: chain_state.tip,
                    height: chain_state.height,
                    db,
                    utxo_set,
                    validator,
                    monetary_policy,
                })
            } else {
                // No existing state, create genesis
                Self::create_new_blockchain(db, utxo_set, validator, monetary_policy)
            }
        } else {
            // Create genesis block
            Self::create_new_blockchain(db, utxo_set, validator, monetary_policy)
        }
    }
    
    fn create_new_blockchain(
        db: Arc<Database>,
        utxo_set: Arc<RwLock<UtxoSet>>,
        validator: BlockValidator,
        monetary_policy: MonetaryPolicy,
    ) -> Result<Self> {
        // Create genesis block
        let genesis = Self::create_genesis_block();
        let genesis_hash = genesis.hash();
        
        // Save genesis block
        db.save_block(&genesis)?;
        db.save_chain_state(&ChainState {
            tip: genesis_hash,
            height: 0,
            total_work: 0,
            difficulty: 6, // Very easy initial difficulty for testing
            total_supply: 0, // Genesis block has no reward
        })?;
        
        // Initialize UTXO set with genesis coinbase
        let mut utxo_set_lock = utxo_set.write().unwrap();
        utxo_set_lock.apply_block(&genesis)?;
        drop(utxo_set_lock);
        
        Ok(Self {
            tip: genesis_hash,
            height: 0,
            db,
            utxo_set,
            validator,
            monetary_policy,
        })
    }

    pub fn create_genesis_block() -> Block {
        let genesis_message = "The Times 10/Jul/2025 Chancellor on brink of second bailout for banks - QTC Genesis";
        // Genesis block has NO REWARD - this is the pre-mine prevention
        let coinbase_tx = Transaction::new_coinbase(
            "qtc1qw508d6qejxtdg4y5r3zarvary0c5xw7kxdz6v9".to_string(), // Genesis address
            0, // NO REWARD for genesis block (0 QTC)
            genesis_message.to_string(),
        );
        
        Block::new(
            Hash256::zero(), // Previous hash (genesis)
            vec![coinbase_tx],
            6, // Very easy initial difficulty for testing
            0, // Height
        )
    }
    
    pub fn add_block(&mut self, block: Block) -> Result<()> {
        // Validate block
        self.validator.validate_block(&block, self)?;
        
        // Mine the block if not already mined
        if !self.is_valid_proof_of_work(&block) {
            return Err(QtcError::Blockchain("Invalid proof of work".to_string()));
        }
        
        let block_hash = block.hash();
        
        // Update UTXO set
        {
            let mut utxo_set = self.utxo_set.write().unwrap();
            utxo_set.apply_block(&block)?;
        }
        
        // Save block
        self.db.save_block(&block)?;
        
        // Update chain state
        let new_height = self.height + 1;
        let new_difficulty = self.calculate_next_difficulty(new_height)?;
        let total_supply = self.calculate_total_supply(new_height);
        
        let new_state = ChainState {
            tip: block_hash,
            height: new_height,
            total_work: 0, // TODO: Implement total work calculation
            difficulty: new_difficulty,
            total_supply,
        };
        
        self.db.save_chain_state(&new_state)?;
        
        // Update in-memory state
        self.tip = block_hash;
        self.height = new_height;
        
        log::info!("✅ Block {} added to blockchain", new_height);
        Ok(())
    }
    
    pub fn get_block(&self, hash: &Hash256) -> Result<Option<Block>> {
        self.db.get_block(hash)
    }
    
    pub fn get_block_by_height(&self, height: u64) -> Result<Option<Block>> {
        self.db.get_block_by_height(height)
    }
    
    pub fn get_balance(&self, address: &str) -> Result<u64> {
        let utxo_set = self.utxo_set.read().unwrap();
        utxo_set.get_balance(address)
    }
    
    pub fn get_utxos(&self, address: &str) -> Result<Vec<(Hash256, u32, u64)>> {
        let utxo_set = self.utxo_set.read().unwrap();
        utxo_set.get_utxos(address)
    }
    
    /// Get all addresses that have ever been used (for blockchain explorer)
    pub fn get_all_addresses(&self) -> Result<Vec<String>> {
        self.db.get_all_addresses()
    }
    
    /// Get transaction history for an address (for blockchain explorer)
    pub fn get_address_transactions(&self, address: &str, limit: Option<usize>) -> Result<Vec<(Hash256, Transaction, u64)>> {
        self.db.get_address_transactions(address, limit.unwrap_or(100))
    }
    
    /// Get rich list of addresses with highest balances (for blockchain explorer)
    pub fn get_rich_list(&self, limit: usize) -> Result<Vec<(String, u64)>> {
        let mut balances = Vec::new();
        let addresses = self.get_all_addresses()?;
        
        for address in addresses {
            let balance = self.get_balance(&address)?;
            if balance > 0 {
                balances.push((address, balance));
            }
        }
        
        // Sort by balance descending
        balances.sort_by(|a, b| b.1.cmp(&a.1));
        balances.truncate(limit);
        
        Ok(balances)
    }
    
    /// Get comprehensive blockchain statistics (for blockchain explorer)
    pub fn get_blockchain_stats(&self) -> Result<BlockchainStats> {
        let chain_state = self.get_chain_info()?;
        let total_addresses = self.get_all_addresses()?.len();
        let recent_blocks = self.get_latest_blocks(10)?;
        
        // Calculate average block time from recent blocks
        let mut total_time = 0u64;
        let mut block_count = 0u64;
        
        for i in 1..recent_blocks.len() {
            if recent_blocks[i-1].header.height > 0 {
                total_time += recent_blocks[i-1].header.timestamp - recent_blocks[i].header.timestamp;
                block_count += 1;
            }
        }
        
        let avg_block_time = if block_count > 0 { total_time / block_count } else { 450 };
        
        Ok(BlockchainStats {
            height: chain_state.height,
            difficulty: chain_state.difficulty,
            total_supply: chain_state.total_supply,
            total_addresses,
            avg_block_time,
            network_hashrate: self.estimate_network_hashrate()?,
        })
    }
    
    fn estimate_network_hashrate(&self) -> Result<f64> {
        // Simplified hashrate estimation based on difficulty and block time
        let difficulty = self.get_current_difficulty()? as f64;
        let target_time = 450.0; // 7.5 minutes in seconds
        
        // Rough estimate: hashrate = difficulty * 2^difficulty / target_time
        let hashrate = difficulty * (2.0_f64.powf(difficulty / 8.0)) / target_time;
        Ok(hashrate)
    }
    
    pub fn is_valid_transaction(&self, tx: &Transaction) -> Result<bool> {
        self.validator.validate_transaction(tx, self)
    }
    
    pub fn calculate_next_difficulty(&self, height: u64) -> Result<u32> {
        use crate::mining::difficulty::DifficultyCalculator;
        
        // Use production-grade difficulty calculator
        let calculator = DifficultyCalculator::new();
        
        if height < calculator.adjustment_interval {
            return Ok(20); // Initial difficulty - higher for realistic mining times
        }
        
        // Collect block timestamps for last adjustment interval
        let mut block_times = Vec::new();
        let start_height = height.saturating_sub(calculator.adjustment_interval);
        
        for i in start_height..=height {
            if let Some(block) = self.get_block_by_height(i)? {
                block_times.push(block.header.timestamp);
            }
        }
        
        if block_times.len() < 2 {
            return Ok(self.get_current_difficulty()?);
        }
        
        let current_difficulty = self.get_current_difficulty()?;
        
        // Use robust difficulty adjustment algorithm
        let new_difficulty = calculator.calculate_next_difficulty(current_difficulty, &block_times)?;
        
        log::info!(
            "Difficulty adjustment at height {}: {} -> {} (target: {} seconds per block)",
            height,
            current_difficulty,
            new_difficulty,
            calculator.target_block_time
        );
        
        Ok(new_difficulty)
    }
    
    pub fn get_current_difficulty(&self) -> Result<u32> {
        let state = self.db.get_chain_state()?;
        Ok(state.unwrap_or_default().difficulty)
    }
    
    pub fn calculate_total_supply(&self, height: u64) -> u64 {
        self.monetary_policy.total_supply_at_height(height)
    }
    
    pub fn is_valid_proof_of_work(&self, block: &Block) -> bool {
        let hash = block.hash();
        let difficulty = block.header.difficulty;
        
        // Check if hash has required number of leading zeros
        let required_zeros = difficulty / 4; // 4 bits per hex digit
        let remaining_bits = difficulty % 4;
        
        for i in 0..required_zeros as usize {
            if hash.as_bytes()[i] != 0 {
                return false;
            }
        }
        
        // Check remaining bits if any
        if remaining_bits > 0 {
            let byte_index = required_zeros as usize;
            if byte_index < hash.as_bytes().len() {
                let mask = 0xFF << (8 - remaining_bits);
                if hash.as_bytes()[byte_index] & mask != 0 {
                    return false;
                }
            }
        }
        
        true
    }
    
    pub fn get_latest_blocks(&self, count: usize) -> Result<Vec<Block>> {
        let mut blocks = Vec::new();
        let mut current_height = self.height;
        
        for _ in 0..count {
            if let Some(block) = self.get_block_by_height(current_height)? {
                blocks.push(block);
                if current_height == 0 {
                    break;
                }
                current_height -= 1;
            } else {
                break;
            }
        }
        
        Ok(blocks)
    }
    
    pub fn get_chain_info(&self) -> Result<ChainState> {
        self.db.get_chain_state().map(|opt| opt.unwrap_or_default())
    }
}
