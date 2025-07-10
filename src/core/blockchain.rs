use crate::core::{Block, BlockHeader, Transaction};
use crate::core::utxo::UtxoSet;
use crate::storage::Database;
use crate::consensus::validation::BlockValidator;
use crate::consensus::monetary::MonetaryPolicy;
use crate::crypto::hash::Hash256;
use crate::{QtcError, Result};
use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};
use std::sync::{Arc, RwLock};

#[derive(Debug, Clone)]
pub struct Blockchain {
    pub tip: Hash256,
    pub height: u64,
    db: Arc<Database>,
    pub utxo_set: Arc<RwLock<UtxoSet>>,
    validator: BlockValidator,
    monetary_policy: MonetaryPolicy,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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
            Ok(Self {
                tip: state.tip,
                height: state.height,
                db,
                utxo_set,
                validator,
                monetary_policy,
            })
        } else {
            // Create genesis block
            let genesis = Self::create_genesis_block();
            let genesis_hash = genesis.hash();
            
            // Save genesis block
            db.save_block(&genesis)?;
            db.save_chain_state(&ChainState {
                tip: genesis_hash,
                height: 0,
                total_work: 0,
                difficulty: 4, // Initial difficulty
                total_supply: monetary_policy.coinbase_reward(0),
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
    }
    
    pub fn create_genesis_block() -> Block {
        let genesis_message = "The Times 10/Jul/2025 Chancellor on brink of second bailout for banks - QTC Genesis";
        let coinbase_tx = Transaction::new_coinbase(
            "qtc1qw508d6qejxtdg4y5r3zarvary0c5xw7kxdz6v9".to_string(), // Genesis address
            2710000000, // 27.1 QTC in satoshis
            genesis_message.to_string(),
        );
        
        Block::new(
            Hash256::zero(), // Previous hash (genesis)
            vec![coinbase_tx],
            4, // Initial difficulty
            0, // Height
        )
    }
    
    pub fn add_block(&mut self, mut block: Block) -> Result<()> {
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
    
    pub fn is_valid_transaction(&self, tx: &Transaction) -> Result<bool> {
        self.validator.validate_transaction(tx, self)
    }
    
    pub fn calculate_next_difficulty(&self, height: u64) -> Result<u32> {
        if height < 10 {
            return Ok(4); // Initial difficulty
        }
        
        // Get last 10 blocks for difficulty adjustment
        let mut total_time = 0u64;
        for i in (height.saturating_sub(10))..height {
            if let Some(block) = self.get_block_by_height(i)? {
                if i > 0 {
                    if let Some(prev_block) = self.get_block_by_height(i - 1)? {
                        total_time += block.header.timestamp - prev_block.header.timestamp;
                    }
                }
            }
        }
        
        let target_time = 450 * 10; // 7.5 minutes * 10 blocks
        let current_difficulty = self.get_current_difficulty()?;
        
        if total_time == 0 {
            return Ok(current_difficulty);
        }
        
        // Adjust difficulty based on time ratio
        let time_ratio = target_time as f64 / total_time as f64;
        let new_difficulty = (current_difficulty as f64 * time_ratio) as u32;
        
        // Limit difficulty changes to prevent wild swings
        let max_change = current_difficulty / 4;
        let new_difficulty = new_difficulty.max(current_difficulty.saturating_sub(max_change))
                                        .min(current_difficulty.saturating_add(max_change));
        
        Ok(new_difficulty.max(1)) // Minimum difficulty of 1
    }
    
    pub fn get_current_difficulty(&self) -> Result<u32> {
        let state = self.db.get_chain_state()?;
        Ok(state.difficulty)
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
        self.db.get_chain_state()
    }
}
