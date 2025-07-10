use crate::core::{Block, Transaction, UtxoEntry};
use crate::core::blockchain::ChainState;
use crate::core::transaction::OutPoint;
use crate::crypto::hash::{Hash256, Hashable};
use crate::wallet::WalletInfo;
use crate::{QtcError, Result};
use sled::{Db, Tree};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Arc;

// Database tree names (equivalent to column families)
const TREE_BLOCKS: &str = "blocks";
const TREE_BLOCK_INDEX: &str = "block_index";
const TREE_TRANSACTIONS: &str = "transactions";
const TREE_UTXOS: &str = "utxos";
const TREE_CHAIN_STATE: &str = "chain_state";
const TREE_WALLETS: &str = "wallets";
const TREE_ADDRESSES: &str = "addresses";

#[derive(Debug, Clone)]
pub struct Database {
    db: Arc<Db>,
}

impl Database {
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self> {
        let db = sled::open(path)
            .map_err(|e| QtcError::Storage(format!("Failed to open database: {}", e)))?;
        
        Ok(Self {
            db: Arc::new(db),
        })
    }
    
    fn get_tree(&self, tree_name: &str) -> Result<Tree> {
        self.db.open_tree(tree_name)
            .map_err(|e| QtcError::Storage(format!("Failed to open tree {}: {}", tree_name, e)))
    }
    
    // Block operations
    pub fn save_block(&self, block: &Block) -> Result<()> {
        let blocks_tree = self.get_tree(TREE_BLOCKS)?;
        let index_tree = self.get_tree(TREE_BLOCK_INDEX)?;
        
        let block_hash = block.hash();
        let block_data = bincode::serialize(block)
            .map_err(|e| QtcError::Storage(format!("Failed to serialize block: {}", e)))?;
        
        // Save block by hash
        blocks_tree.insert(block_hash.as_bytes(), block_data)
            .map_err(|e| QtcError::Storage(format!("Failed to save block: {}", e)))?;
        
        // Save block hash by height
        let height_key = format!("height_{}", block.header.height);
        index_tree.insert(height_key.as_bytes(), block_hash.as_bytes())
            .map_err(|e| QtcError::Storage(format!("Failed to save block index: {}", e)))?;
        
        log::debug!("💾 Saved block {} at height {}", block_hash, block.header.height);
        Ok(())
    }
    
    pub fn get_block(&self, hash: &Hash256) -> Result<Option<Block>> {
        let blocks_tree = self.get_tree(TREE_BLOCKS)?;
        
        match blocks_tree.get(hash.as_bytes())
            .map_err(|e| QtcError::Storage(format!("Failed to get block: {}", e)))? {
            Some(data) => {
                let block: Block = bincode::deserialize(&data)
                    .map_err(|e| QtcError::Storage(format!("Failed to deserialize block: {}", e)))?;
                Ok(Some(block))
            }
            None => Ok(None),
        }
    }
    
    pub fn get_block_by_height(&self, height: u64) -> Result<Option<Block>> {
        let index_tree = self.get_tree(TREE_BLOCK_INDEX)?;
        let height_key = format!("height_{}", height);
        
        match index_tree.get(height_key.as_bytes())
            .map_err(|e| QtcError::Storage(format!("Failed to get block index: {}", e)))? {
            Some(hash_bytes) => {
                if hash_bytes.len() != 32 {
                    return Err(QtcError::Storage("Invalid block hash length".to_string()));
                }
                
                let mut hash_array = [0u8; 32];
                hash_array.copy_from_slice(&hash_bytes);
                let block_hash = Hash256::new(hash_array);
                
                self.get_block(&block_hash)
            }
            None => Ok(None),
        }
    }
    
    pub fn get_latest_blocks(&self, count: usize) -> Result<Vec<Block>> {
        let index_tree = self.get_tree(TREE_BLOCK_INDEX)?;
        let mut blocks = Vec::new();
        
        // Iterate through height keys in reverse order
        for item in index_tree.iter().rev().take(count) {
            match item {
                Ok((key, value)) => {
                    if let Ok(key_str) = String::from_utf8(key.to_vec()) {
                        if key_str.starts_with("height_") && value.len() == 32 {
                            let mut hash_array = [0u8; 32];
                            hash_array.copy_from_slice(&value);
                            let block_hash = Hash256::new(hash_array);
                            
                            if let Ok(Some(block)) = self.get_block(&block_hash) {
                                blocks.push(block);
                            }
                        }
                    }
                }
                Err(e) => {
                    log::warn!("Error iterating blocks: {}", e);
                    break;
                }
            }
        }
        
        Ok(blocks)
    }
    
    // Transaction operations
    pub fn save_transaction(&self, tx: &Transaction) -> Result<()> {
        let tx_tree = self.get_tree(TREE_TRANSACTIONS)?;
        let tx_hash = tx.hash();
        let tx_data = bincode::serialize(tx)
            .map_err(|e| QtcError::Storage(format!("Failed to serialize transaction: {}", e)))?;
        
        tx_tree.insert(tx_hash.as_bytes(), tx_data)
            .map_err(|e| QtcError::Storage(format!("Failed to save transaction: {}", e)))?;
        
        log::debug!("💾 Saved transaction {}", tx_hash);
        Ok(())
    }
    
    pub fn get_transaction(&self, hash: &Hash256) -> Result<Option<Transaction>> {
        let tx_tree = self.get_tree(TREE_TRANSACTIONS)?;
        
        match tx_tree.get(hash.as_bytes())
            .map_err(|e| QtcError::Storage(format!("Failed to get transaction: {}", e)))? {
            Some(data) => {
                let tx: Transaction = bincode::deserialize(&data)
                    .map_err(|e| QtcError::Storage(format!("Failed to deserialize transaction: {}", e)))?;
                Ok(Some(tx))
            }
            None => Ok(None),
        }
    }
    
    // UTXO operations
    pub fn save_utxo(&self, outpoint: &OutPoint, utxo: &UtxoEntry) -> Result<()> {
        let utxo_tree = self.get_tree(TREE_UTXOS)?;
        let key = self.outpoint_to_key(outpoint);
        let data = bincode::serialize(utxo)
            .map_err(|e| QtcError::Storage(format!("Failed to serialize UTXO: {}", e)))?;
        
        utxo_tree.insert(&key, data)
            .map_err(|e| QtcError::Storage(format!("Failed to save UTXO: {}", e)))?;
        
        log::debug!("💾 Saved UTXO {}:{}", hex::encode(outpoint.txid.as_bytes()), outpoint.vout);
        Ok(())
    }
    
    pub fn get_utxo(&self, outpoint: &OutPoint) -> Result<Option<UtxoEntry>> {
        let utxo_tree = self.get_tree(TREE_UTXOS)?;
        let key = self.outpoint_to_key(outpoint);
        
        match utxo_tree.get(&key)
            .map_err(|e| QtcError::Storage(format!("Failed to get UTXO: {}", e)))? {
            Some(data) => {
                let utxo: UtxoEntry = bincode::deserialize(&data)
                    .map_err(|e| QtcError::Storage(format!("Failed to deserialize UTXO: {}", e)))?;
                Ok(Some(utxo))
            }
            None => Ok(None),
        }
    }
    
    pub fn delete_utxo(&self, outpoint: &OutPoint) -> Result<()> {
        let utxo_tree = self.get_tree(TREE_UTXOS)?;
        let key = self.outpoint_to_key(outpoint);
        
        utxo_tree.remove(&key)
            .map_err(|e| QtcError::Storage(format!("Failed to delete UTXO: {}", e)))?;
        
        log::debug!("🗑️ Deleted UTXO {}:{}", hex::encode(outpoint.txid.as_bytes()), outpoint.vout);
        Ok(())
    }
    
    pub fn get_utxos_for_address(&self, address: &str) -> Result<Vec<(OutPoint, UtxoEntry)>> {
        let utxo_tree = self.get_tree(TREE_UTXOS)?;
        let mut utxos = Vec::new();
        
        for item in utxo_tree.iter() {
            match item {
                Ok((key, value)) => {
                    if let Ok(utxo) = bincode::deserialize::<UtxoEntry>(&value) {
                        if utxo.address == address {
                            if let Ok(outpoint) = self.key_to_outpoint(&key) {
                                utxos.push((outpoint, utxo));
                            }
                        }
                    }
                }
                Err(e) => {
                    log::warn!("Error iterating UTXOs: {}", e);
                    break;
                }
            }
        }
        
        Ok(utxos)
    }
    
    // Chain state operations
    pub fn save_chain_state(&self, state: &ChainState) -> Result<()> {
        let state_tree = self.get_tree(TREE_CHAIN_STATE)?;
        let data = bincode::serialize(state)
            .map_err(|e| QtcError::Storage(format!("Failed to serialize chain state: {}", e)))?;
        
        state_tree.insert(b"current", data)
            .map_err(|e| QtcError::Storage(format!("Failed to save chain state: {}", e)))?;
        
        log::debug!("💾 Saved chain state at height {}", state.height);
        Ok(())
    }
    
    pub fn get_chain_state(&self) -> Result<Option<ChainState>> {
        let state_tree = self.get_tree(TREE_CHAIN_STATE)?;
        
        match state_tree.get(b"current")
            .map_err(|e| QtcError::Storage(format!("Failed to get chain state: {}", e)))? {
            Some(data) => {
                let state: ChainState = bincode::deserialize(&data)
                    .map_err(|e| QtcError::Storage(format!("Failed to deserialize chain state: {}", e)))?;
                Ok(Some(state))
            }
            None => Ok(None),
        }
    }
    
    // Wallet operations
    pub fn save_wallet(&self, wallet_id: &str, wallet: &WalletInfo) -> Result<()> {
        let wallet_tree = self.get_tree(TREE_WALLETS)?;
        let data = bincode::serialize(wallet)
            .map_err(|e| QtcError::Storage(format!("Failed to serialize wallet: {}", e)))?;
        
        wallet_tree.insert(wallet_id.as_bytes(), data)
            .map_err(|e| QtcError::Storage(format!("Failed to save wallet: {}", e)))?;
        
        log::debug!("💾 Saved wallet {}", wallet_id);
        Ok(())
    }
    
    pub fn get_wallet(&self, wallet_id: &str) -> Result<Option<WalletInfo>> {
        let wallet_tree = self.get_tree(TREE_WALLETS)?;
        
        match wallet_tree.get(wallet_id.as_bytes())
            .map_err(|e| QtcError::Storage(format!("Failed to get wallet: {}", e)))? {
            Some(data) => {
                let wallet: WalletInfo = bincode::deserialize(&data)
                    .map_err(|e| QtcError::Storage(format!("Failed to deserialize wallet: {}", e)))?;
                Ok(Some(wallet))
            }
            None => Ok(None),
        }
    }
    
    pub fn list_wallets(&self) -> Result<Vec<String>> {
        let wallet_tree = self.get_tree(TREE_WALLETS)?;
        let mut wallets = Vec::new();
        
        for item in wallet_tree.iter() {
            match item {
                Ok((key, _)) => {
                    if let Ok(wallet_id) = String::from_utf8(key.to_vec()) {
                        wallets.push(wallet_id);
                    }
                }
                Err(e) => {
                    log::warn!("Error iterating wallets: {}", e);
                    break;
                }
            }
        }
        
        Ok(wallets)
    }
    
    pub fn load_wallet(&self, wallet_id: &str, blockchain: Arc<std::sync::RwLock<crate::core::Blockchain>>) -> Result<crate::wallet::Wallet> {
        let wallet_info = self.get_wallet(wallet_id)?
            .ok_or_else(|| QtcError::Wallet(format!("Wallet not found: {}", wallet_id)))?;
        
        // Convert WalletInfo to Wallet (this is a simplified implementation)
        let wallet = crate::wallet::Wallet {
            info: wallet_info,
            addresses: std::collections::HashMap::new(), // Would load from wallet data
            hd_wallet: None, // Would restore from seed if available
            db: Arc::new(self.clone()),
            blockchain,
        };
        
        Ok(wallet)
    }
    
    pub fn delete_wallet(&self, wallet_id: &str) -> Result<()> {
        let wallet_tree = self.get_tree(TREE_WALLETS)?;
        
        wallet_tree.remove(wallet_id.as_bytes())
            .map_err(|e| QtcError::Storage(format!("Failed to delete wallet: {}", e)))?;
        
        log::debug!("🗑️ Deleted wallet {}", wallet_id);
        Ok(())
    }
    
    // Address operations
    pub fn save_address_info(&self, address: &str, wallet_id: &str, derivation_path: &str) -> Result<()> {
        let addr_tree = self.get_tree(TREE_ADDRESSES)?;
        let info = AddressInfo {
            wallet_id: wallet_id.to_string(),
            derivation_path: derivation_path.to_string(),
        };
        
        let data = bincode::serialize(&info)
            .map_err(|e| QtcError::Storage(format!("Failed to serialize address info: {}", e)))?;
        
        addr_tree.insert(address.as_bytes(), data)
            .map_err(|e| QtcError::Storage(format!("Failed to save address info: {}", e)))?;
        
        Ok(())
    }
    
    pub fn get_address_info(&self, address: &str) -> Result<Option<AddressInfo>> {
        let addr_tree = self.get_tree(TREE_ADDRESSES)?;
        
        match addr_tree.get(address.as_bytes())
            .map_err(|e| QtcError::Storage(format!("Failed to get address info: {}", e)))? {
            Some(data) => {
                let info: AddressInfo = bincode::deserialize(&data)
                    .map_err(|e| QtcError::Storage(format!("Failed to deserialize address info: {}", e)))?;
                Ok(Some(info))
            }
            None => Ok(None),
        }
    }
    
    // Helper methods
    fn outpoint_to_key(&self, outpoint: &OutPoint) -> Vec<u8> {
        let mut key = Vec::with_capacity(36); // 32 bytes for txid + 4 bytes for vout
        key.extend_from_slice(outpoint.txid.as_bytes());
        key.extend_from_slice(&outpoint.vout.to_le_bytes());
        key
    }
    
    fn key_to_outpoint(&self, key: &[u8]) -> Result<OutPoint> {
        if key.len() != 36 {
            return Err(QtcError::Storage("Invalid outpoint key length".to_string()));
        }
        
        let mut txid_bytes = [0u8; 32];
        txid_bytes.copy_from_slice(&key[0..32]);
        let txid = Hash256::new(txid_bytes);
        
        let mut vout_bytes = [0u8; 4];
        vout_bytes.copy_from_slice(&key[32..36]);
        let vout = u32::from_le_bytes(vout_bytes);
        
        Ok(OutPoint { txid, vout })
    }
    
    // Database maintenance
    pub fn flush(&self) -> Result<()> {
        self.db.flush()
            .map_err(|e| QtcError::Storage(format!("Failed to flush database: {}", e)))?;
        Ok(())
    }
    
    pub fn compact(&self) -> Result<()> {
        // Sled doesn't have explicit compaction, but we can trigger a flush
        self.flush()
    }
    
    pub fn get_all_utxos(&self) -> Result<Vec<(OutPoint, UtxoEntry)>> {
        let utxo_tree = self.get_tree(TREE_UTXOS)?;
        let mut utxos = Vec::new();
        
        for item in utxo_tree.iter() {
            match item {
                Ok((key, value)) => {
                    if let Ok(utxo) = bincode::deserialize::<UtxoEntry>(&value) {
                        if let Ok(outpoint) = self.key_to_outpoint(&key) {
                            utxos.push((outpoint, utxo));
                        }
                    }
                }
                Err(e) => {
                    log::warn!("Error iterating UTXOs: {}", e);
                    break;
                }
            }
        }
        
        Ok(utxos)
    }
    
    pub fn get_database_stats(&self) -> Result<DatabaseStats> {
        let mut stats = DatabaseStats::default();
        
        // Count items in each tree
        for tree_name in &[TREE_BLOCKS, TREE_TRANSACTIONS, TREE_UTXOS, TREE_WALLETS] {
            if let Ok(tree) = self.get_tree(tree_name) {
                let count = tree.iter().count();
                match *tree_name {
                    TREE_BLOCKS => stats.block_count = count,
                    TREE_TRANSACTIONS => stats.transaction_count = count,
                    TREE_UTXOS => stats.utxo_count = count,
                    TREE_WALLETS => stats.wallet_count = count,
                    _ => {}
                }
            }
        }
        
        Ok(stats)
    }
    

}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddressInfo {
    pub wallet_id: String,
    pub derivation_path: String,
}

#[derive(Debug, Default)]
pub struct DatabaseStats {
    pub block_count: usize,
    pub transaction_count: usize,
    pub utxo_count: usize,
    pub wallet_count: usize,
    pub blocks_size: usize,
    pub utxo_size: usize,
    pub total_size: u64,
}

impl DatabaseStats {
    pub fn total_size(&self) -> u64 {
        self.total_size
    }
}

// Error handling for sled database is handled by the thiserror derive macro