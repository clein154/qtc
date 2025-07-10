use crate::core::{Block, Transaction, ChainState, UtxoEntry, OutPoint};
use crate::crypto::hash::Hash256;
use crate::wallet::{Wallet, WalletInfo};
use crate::{QtcError, Result};
use rocksdb::{DB, Options, ColumnFamily, IteratorMode, Direction};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Arc;

// Database column families
const CF_BLOCKS: &str = "blocks";
const CF_BLOCK_INDEX: &str = "block_index";
const CF_TRANSACTIONS: &str = "transactions";
const CF_UTXOS: &str = "utxos";
const CF_CHAIN_STATE: &str = "chain_state";
const CF_WALLETS: &str = "wallets";
const CF_ADDRESSES: &str = "addresses";

pub struct Database {
    db: Arc<DB>,
}

impl Database {
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self> {
        let mut opts = Options::default();
        opts.create_if_missing(true);
        opts.create_missing_column_families(true);
        
        // Performance optimizations
        opts.set_max_open_files(10000);
        opts.set_write_buffer_size(64 * 1024 * 1024); // 64MB
        opts.set_max_write_buffer_number(3);
        opts.set_target_file_size_base(64 * 1024 * 1024); // 64MB
        opts.set_level_zero_file_num_compaction_trigger(8);
        opts.set_level_zero_slowdown_writes_trigger(17);
        opts.set_level_zero_stop_writes_trigger(24);
        opts.set_num_levels(4);
        opts.set_max_bytes_for_level_base(512 * 1024 * 1024); // 512MB
        opts.set_max_bytes_for_level_multiplier(8.0);
        
        // Enable compression
        opts.set_compression_type(rocksdb::DBCompressionType::Snappy);
        
        let cfs = vec![
            CF_BLOCKS,
            CF_BLOCK_INDEX,
            CF_TRANSACTIONS,
            CF_UTXOS,
            CF_CHAIN_STATE,
            CF_WALLETS,
            CF_ADDRESSES,
        ];
        
        let db = DB::open_cf(&opts, path, &cfs)
            .map_err(|e| QtcError::Database(e))?;
        
        Ok(Self {
            db: Arc::new(db),
        })
    }
    
    // Block operations
    pub fn save_block(&self, block: &Block) -> Result<()> {
        let cf = self.get_cf(CF_BLOCKS)?;
        let block_hash = block.hash();
        let block_data = bincode::serialize(block)
            .map_err(|e| QtcError::Storage(format!("Failed to serialize block: {}", e)))?;
        
        // Save block by hash
        self.db.put_cf(&cf, block_hash.as_bytes(), &block_data)?;
        
        // Save block hash by height
        let index_cf = self.get_cf(CF_BLOCK_INDEX)?;
        let height_key = format!("height_{}", block.header.height);
        self.db.put_cf(&index_cf, height_key.as_bytes(), block_hash.as_bytes())?;
        
        log::debug!("💾 Saved block {} at height {}", block_hash, block.header.height);
        Ok(())
    }
    
    pub fn get_block(&self, hash: &Hash256) -> Result<Option<Block>> {
        let cf = self.get_cf(CF_BLOCKS)?;
        
        match self.db.get_cf(&cf, hash.as_bytes())? {
            Some(data) => {
                let block: Block = bincode::deserialize(&data)
                    .map_err(|e| QtcError::Storage(format!("Failed to deserialize block: {}", e)))?;
                Ok(Some(block))
            }
            None => Ok(None),
        }
    }
    
    pub fn get_block_by_height(&self, height: u64) -> Result<Option<Block>> {
        let index_cf = self.get_cf(CF_BLOCK_INDEX)?;
        let height_key = format!("height_{}", height);
        
        match self.db.get_cf(&index_cf, height_key.as_bytes())? {
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
        let index_cf = self.get_cf(CF_BLOCK_INDEX)?;
        let mut blocks = Vec::new();
        
        let iter = self.db.iterator_cf(&index_cf, IteratorMode::End);
        
        for (key, value) in iter.take(count) {
            if let Ok(key_str) = String::from_utf8(key.to_vec()) {
                if key_str.starts_with("height_") {
                    if value.len() == 32 {
                        let mut hash_array = [0u8; 32];
                        hash_array.copy_from_slice(&value);
                        let block_hash = Hash256::new(hash_array);
                        
                        if let Ok(Some(block)) = self.get_block(&block_hash) {
                            blocks.push(block);
                        }
                    }
                }
            }
        }
        
        // Reverse to get newest first
        blocks.reverse();
        Ok(blocks)
    }
    
    // Transaction operations
    pub fn save_transaction(&self, tx: &Transaction) -> Result<()> {
        let cf = self.get_cf(CF_TRANSACTIONS)?;
        let tx_hash = tx.hash();
        let tx_data = bincode::serialize(tx)
            .map_err(|e| QtcError::Storage(format!("Failed to serialize transaction: {}", e)))?;
        
        self.db.put_cf(&cf, tx_hash.as_bytes(), &tx_data)?;
        
        log::debug!("💾 Saved transaction {}", tx_hash);
        Ok(())
    }
    
    pub fn get_transaction(&self, hash: &Hash256) -> Result<Option<Transaction>> {
        let cf = self.get_cf(CF_TRANSACTIONS)?;
        
        match self.db.get_cf(&cf, hash.as_bytes())? {
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
        let cf = self.get_cf(CF_UTXOS)?;
        let key = self.outpoint_to_key(outpoint);
        let data = bincode::serialize(utxo)
            .map_err(|e| QtcError::Storage(format!("Failed to serialize UTXO: {}", e)))?;
        
        self.db.put_cf(&cf, &key, &data)?;
        
        log::debug!("💾 Saved UTXO {}:{}", hex::encode(outpoint.txid.as_bytes()), outpoint.vout);
        Ok(())
    }
    
    pub fn get_utxo(&self, outpoint: &OutPoint) -> Result<Option<UtxoEntry>> {
        let cf = self.get_cf(CF_UTXOS)?;
        let key = self.outpoint_to_key(outpoint);
        
        match self.db.get_cf(&cf, &key)? {
            Some(data) => {
                let utxo: UtxoEntry = bincode::deserialize(&data)
                    .map_err(|e| QtcError::Storage(format!("Failed to deserialize UTXO: {}", e)))?;
                Ok(Some(utxo))
            }
            None => Ok(None),
        }
    }
    
    pub fn delete_utxo(&self, outpoint: &OutPoint) -> Result<()> {
        let cf = self.get_cf(CF_UTXOS)?;
        let key = self.outpoint_to_key(outpoint);
        
        self.db.delete_cf(&cf, &key)?;
        
        log::debug!("🗑️ Deleted UTXO {}:{}", hex::encode(outpoint.txid.as_bytes()), outpoint.vout);
        Ok(())
    }
    
    pub fn get_utxos_for_address(&self, address: &str) -> Result<Vec<(OutPoint, UtxoEntry)>> {
        let cf = self.get_cf(CF_UTXOS)?;
        let mut utxos = Vec::new();
        
        let iter = self.db.iterator_cf(&cf, IteratorMode::Start);
        
        for (key, value) in iter {
            if let Ok(utxo) = bincode::deserialize::<UtxoEntry>(&value) {
                if self.script_matches_address(&utxo.script_pubkey, address) {
                    if let Some(outpoint) = self.key_to_outpoint(&key) {
                        utxos.push((outpoint, utxo));
                    }
                }
            }
        }
        
        Ok(utxos)
    }
    
    pub fn get_all_utxos(&self) -> Result<Vec<(OutPoint, UtxoEntry)>> {
        let cf = self.get_cf(CF_UTXOS)?;
        let mut utxos = Vec::new();
        
        let iter = self.db.iterator_cf(&cf, IteratorMode::Start);
        
        for (key, value) in iter {
            if let Ok(utxo) = bincode::deserialize::<UtxoEntry>(&value) {
                if let Some(outpoint) = self.key_to_outpoint(&key) {
                    utxos.push((outpoint, utxo));
                }
            }
        }
        
        Ok(utxos)
    }
    
    // Chain state operations
    pub fn save_chain_state(&self, state: &ChainState) -> Result<()> {
        let cf = self.get_cf(CF_CHAIN_STATE)?;
        let data = bincode::serialize(state)
            .map_err(|e| QtcError::Storage(format!("Failed to serialize chain state: {}", e)))?;
        
        self.db.put_cf(&cf, b"current", &data)?;
        
        log::debug!("💾 Saved chain state: height {}, tip {}", state.height, state.tip);
        Ok(())
    }
    
    pub fn get_chain_state(&self) -> Result<ChainState> {
        let cf = self.get_cf(CF_CHAIN_STATE)?;
        
        match self.db.get_cf(&cf, b"current")? {
            Some(data) => {
                let state: ChainState = bincode::deserialize(&data)
                    .map_err(|e| QtcError::Storage(format!("Failed to deserialize chain state: {}", e)))?;
                Ok(state)
            }
            None => Err(QtcError::Storage("Chain state not found".to_string())),
        }
    }
    
    // Wallet operations
    pub fn save_wallet(&self, name: &str, wallet: &Wallet) -> Result<()> {
        let cf = self.get_cf(CF_WALLETS)?;
        let wallet_data = WalletData {
            info: wallet.info.clone(),
            addresses: wallet.addresses.clone(),
            hd_wallet: wallet.hd_wallet.clone(),
        };
        
        let data = bincode::serialize(&wallet_data)
            .map_err(|e| QtcError::Storage(format!("Failed to serialize wallet: {}", e)))?;
        
        self.db.put_cf(&cf, name.as_bytes(), &data)?;
        
        log::debug!("💾 Saved wallet: {}", name);
        Ok(())
    }
    
    pub fn load_wallet(&self, name: &str, blockchain: Arc<crate::core::Blockchain>) -> Result<Wallet> {
        let cf = self.get_cf(CF_WALLETS)?;
        
        match self.db.get_cf(&cf, name.as_bytes())? {
            Some(data) => {
                let wallet_data: WalletData = bincode::deserialize(&data)
                    .map_err(|e| QtcError::Storage(format!("Failed to deserialize wallet: {}", e)))?;
                
                Ok(Wallet {
                    info: wallet_data.info,
                    addresses: wallet_data.addresses,
                    hd_wallet: wallet_data.hd_wallet,
                    db: Arc::new(self.clone()),
                    blockchain,
                })
            }
            None => Err(QtcError::Wallet(format!("Wallet '{}' not found", name))),
        }
    }
    
    pub fn list_wallets(&self) -> Result<Vec<String>> {
        let cf = self.get_cf(CF_WALLETS)?;
        let mut wallets = Vec::new();
        
        let iter = self.db.iterator_cf(&cf, IteratorMode::Start);
        
        for (key, _) in iter {
            if let Ok(name) = String::from_utf8(key.to_vec()) {
                wallets.push(name);
            }
        }
        
        Ok(wallets)
    }
    
    pub fn delete_wallet(&self, name: &str) -> Result<()> {
        let cf = self.get_cf(CF_WALLETS)?;
        self.db.delete_cf(&cf, name.as_bytes())?;
        
        log::info!("🗑️ Deleted wallet: {}", name);
        Ok(())
    }
    
    // Address indexing
    pub fn index_address(&self, address: &str, wallet_name: &str) -> Result<()> {
        let cf = self.get_cf(CF_ADDRESSES)?;
        self.db.put_cf(&cf, address.as_bytes(), wallet_name.as_bytes())?;
        Ok(())
    }
    
    pub fn get_wallet_for_address(&self, address: &str) -> Result<Option<String>> {
        let cf = self.get_cf(CF_ADDRESSES)?;
        
        match self.db.get_cf(&cf, address.as_bytes())? {
            Some(data) => {
                let wallet_name = String::from_utf8(data)
                    .map_err(|e| QtcError::Storage(format!("Invalid wallet name: {}", e)))?;
                Ok(Some(wallet_name))
            }
            None => Ok(None),
        }
    }
    
    // Utility methods
    fn get_cf(&self, name: &str) -> Result<Arc<ColumnFamily>> {
        self.db.cf_handle(name)
            .ok_or_else(|| QtcError::Storage(format!("Column family '{}' not found", name)))
    }
    
    fn outpoint_to_key(&self, outpoint: &OutPoint) -> Vec<u8> {
        let mut key = Vec::new();
        key.extend_from_slice(outpoint.txid.as_bytes());
        key.extend_from_slice(&outpoint.vout.to_le_bytes());
        key
    }
    
    fn key_to_outpoint(&self, key: &[u8]) -> Option<OutPoint> {
        if key.len() != 36 { // 32 bytes hash + 4 bytes vout
            return None;
        }
        
        let mut txid_bytes = [0u8; 32];
        txid_bytes.copy_from_slice(&key[0..32]);
        let txid = Hash256::new(txid_bytes);
        
        let vout = u32::from_le_bytes([key[32], key[33], key[34], key[35]]);
        
        Some(OutPoint::new(txid, vout))
    }
    
    fn script_matches_address(&self, script_pubkey: &[u8], address: &str) -> bool {
        // Simplified address matching - in production would properly decode scripts
        if script_pubkey.len() < 25 {
            return false;
        }
        
        // Extract hash160 from P2PKH script
        if script_pubkey[0] == 0x76 && script_pubkey[1] == 0xa9 && script_pubkey[2] == 20 {
            let script_hash = &script_pubkey[3..23];
            let address_hash = Hash256::hash(address.as_bytes());
            return script_hash == &address_hash.as_bytes()[0..20];
        }
        
        false
    }
    
    // Database maintenance
    pub fn compact(&self) -> Result<()> {
        log::info!("🔧 Starting database compaction");
        
        self.db.compact_range::<&[u8], &[u8]>(None, None);
        
        // Compact each column family
        for cf_name in [CF_BLOCKS, CF_BLOCK_INDEX, CF_TRANSACTIONS, CF_UTXOS, CF_CHAIN_STATE, CF_WALLETS, CF_ADDRESSES] {
            if let Ok(cf) = self.get_cf(cf_name) {
                self.db.compact_range_cf(&cf, None::<&[u8]>, None::<&[u8]>);
            }
        }
        
        log::info!("✅ Database compaction completed");
        Ok(())
    }
    
    pub fn get_stats(&self) -> Result<DatabaseStats> {
        let mut stats = DatabaseStats::default();
        
        // Get approximate sizes
        if let Ok(cf) = self.get_cf(CF_BLOCKS) {
            stats.blocks_size = self.get_cf_size(&cf)?;
        }
        
        if let Ok(cf) = self.get_cf(CF_UTXOS) {
            stats.utxos_size = self.get_cf_size(&cf)?;
            stats.utxo_count = self.count_keys_in_cf(&cf)?;
        }
        
        if let Ok(cf) = self.get_cf(CF_TRANSACTIONS) {
            stats.transactions_size = self.get_cf_size(&cf)?;
        }
        
        if let Ok(cf) = self.get_cf(CF_WALLETS) {
            stats.wallets_count = self.count_keys_in_cf(&cf)?;
        }
        
        Ok(stats)
    }
    
    fn get_cf_size(&self, cf: &ColumnFamily) -> Result<u64> {
        let size_str = self.db.property_value_cf(cf, "rocksdb.estimate-live-data-size")?
            .unwrap_or_else(|| "0".to_string());
        
        Ok(size_str.parse().unwrap_or(0))
    }
    
    fn count_keys_in_cf(&self, cf: &ColumnFamily) -> Result<u64> {
        let count_str = self.db.property_value_cf(cf, "rocksdb.estimate-num-keys")?
            .unwrap_or_else(|| "0".to_string());
        
        Ok(count_str.parse().unwrap_or(0))
    }
    
    // Backup and restore
    pub fn backup<P: AsRef<Path>>(&self, backup_path: P) -> Result<()> {
        // Create a checkpoint (backup) of the database
        let checkpoint = rocksdb::checkpoint::Checkpoint::new(&self.db)
            .map_err(|e| QtcError::Storage(format!("Failed to create checkpoint: {}", e)))?;
        
        checkpoint.create_checkpoint(backup_path)
            .map_err(|e| QtcError::Storage(format!("Failed to create backup: {}", e)))?;
        
        log::info!("💾 Database backup created");
        Ok(())
    }
}

impl Clone for Database {
    fn clone(&self) -> Self {
        Self {
            db: self.db.clone(),
        }
    }
}

// Helper structs for serialization
#[derive(Debug, Clone, Serialize, Deserialize)]
struct WalletData {
    info: WalletInfo,
    addresses: std::collections::HashMap<String, crate::wallet::WalletAddress>,
    hd_wallet: Option<crate::wallet::bip39::HdWallet>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DatabaseStats {
    pub blocks_size: u64,
    pub transactions_size: u64,
    pub utxos_size: u64,
    pub utxo_count: u64,
    pub wallets_count: u64,
    pub total_size: u64,
}

impl DatabaseStats {
    pub fn total_size(&mut self) {
        self.total_size = self.blocks_size + self.transactions_size + self.utxos_size;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use crate::core::Transaction;
    
    #[test]
    fn test_database_creation() -> Result<()> {
        let temp_dir = TempDir::new().unwrap();
        let _db = Database::new(temp_dir.path().join("test.db"))?;
        Ok(())
    }
    
    #[test]
    fn test_chain_state_storage() -> Result<()> {
        let temp_dir = TempDir::new().unwrap();
        let db = Database::new(temp_dir.path().join("test.db"))?;
        
        let state = ChainState {
            tip: Hash256::hash(b"test"),
            height: 123,
            total_work: 456,
            difficulty: 8,
            total_supply: 1000000,
        };
        
        db.save_chain_state(&state)?;
        let loaded_state = db.get_chain_state()?;
        
        assert_eq!(state.height, loaded_state.height);
        assert_eq!(state.tip, loaded_state.tip);
        
        Ok(())
    }
    
    #[test]
    fn test_utxo_operations() -> Result<()> {
        let temp_dir = TempDir::new().unwrap();
        let db = Database::new(temp_dir.path().join("test.db"))?;
        
        let txid = Hash256::hash(b"test_tx");
        let outpoint = OutPoint::new(txid, 0);
        let utxo = UtxoEntry {
            txid,
            vout: 0,
            value: 50000000,
            script_pubkey: vec![0x76, 0xa9, 0x14],
            height: 100,
            is_coinbase: false,
        };
        
        db.save_utxo(&outpoint, &utxo)?;
        let loaded_utxo = db.get_utxo(&outpoint)?;
        
        assert!(loaded_utxo.is_some());
        assert_eq!(loaded_utxo.unwrap().value, 50000000);
        
        db.delete_utxo(&outpoint)?;
        let deleted_utxo = db.get_utxo(&outpoint)?;
        assert!(deleted_utxo.is_none());
        
        Ok(())
    }
}
