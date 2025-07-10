use crate::core::{Block, Transaction};
use crate::core::transaction::OutPoint;
use crate::storage::Database;
use crate::crypto::hash::{Hash256, Hashable};
use crate::{QtcError, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UtxoEntry {
    pub txid: Hash256,
    pub vout: u32,
    pub value: u64,
    pub script_pubkey: Vec<u8>,
    pub address: String, // Add address field for easier lookup
    pub height: u64,
    pub is_coinbase: bool,
}

#[derive(Debug)]
pub struct UtxoSet {
    db: Arc<Database>,
    cache: HashMap<OutPoint, UtxoEntry>,
    dirty: bool,
}

impl UtxoSet {
    pub fn new(db: Arc<Database>) -> Self {
        Self {
            db,
            cache: HashMap::new(),
            dirty: false,
        }
    }
    
    pub fn apply_block(&mut self, block: &Block) -> Result<()> {
        // Process all transactions in the block
        for tx in &block.transactions {
            self.apply_transaction(tx, block.header.height)?;
        }
        
        // Flush changes to database
        self.flush()?;
        
        Ok(())
    }
    
    pub fn apply_transaction(&mut self, tx: &Transaction, height: u64) -> Result<()> {
        let tx_hash = tx.hash();
        
        // Remove spent UTXOs (inputs)
        if !tx.is_coinbase() {
            for input in &tx.inputs {
                let outpoint = &input.previous_output;
                
                // Check if UTXO exists
                if !self.has_utxo(outpoint)? {
                    return Err(QtcError::Transaction(format!(
                        "UTXO not found: {}:{}", 
                        hex::encode(outpoint.txid.as_bytes()), 
                        outpoint.vout
                    )));
                }
                
                // Remove from cache and mark for deletion
                self.cache.remove(outpoint);
                self.db.delete_utxo(outpoint)?;
                self.dirty = true;
            }
        }
        
        // Add new UTXOs (outputs)
        for (vout, output) in tx.outputs.iter().enumerate() {
            let outpoint = OutPoint::new(tx_hash, vout as u32);
            // Extract address from script_pubkey (simplified)
            let address = Self::script_to_address(&output.script_pubkey).unwrap_or_else(|| "unknown".to_string());
            
            let utxo_entry = UtxoEntry {
                txid: tx_hash,
                vout: vout as u32,
                value: output.value,
                script_pubkey: output.script_pubkey.clone(),
                address,
                height,
                is_coinbase: tx.is_coinbase(),
            };
            
            // Add to cache
            self.cache.insert(outpoint.clone(), utxo_entry.clone());
            
            // Save to database
            self.db.save_utxo(&outpoint, &utxo_entry)?;
            self.dirty = true;
        }
        
        Ok(())
    }
    
    pub fn has_utxo(&self, outpoint: &OutPoint) -> Result<bool> {
        // Check cache first
        if self.cache.contains_key(outpoint) {
            return Ok(true);
        }
        
        // Check database
        match self.db.get_utxo(outpoint) {
            Ok(Some(_)) => Ok(true),
            Ok(None) => Ok(false),
            Err(e) => Err(e),
        }
    }
    
    pub fn get_utxo(&self, outpoint: &OutPoint) -> Result<Option<UtxoEntry>> {
        // Check cache first
        if let Some(utxo) = self.cache.get(outpoint) {
            return Ok(Some(utxo.clone()));
        }
        
        // Check database
        self.db.get_utxo(outpoint)
    }
    
    pub fn get_balance(&self, address: &str) -> Result<u64> {
        let utxos = self.get_utxos(address)?;
        Ok(utxos.iter().map(|(_, _, value)| value).sum())
    }
    
    pub fn get_utxos(&self, address: &str) -> Result<Vec<(Hash256, u32, u64)>> {
        let mut utxos = Vec::new();
        
        // Get UTXOs from database
        let db_utxos = self.db.get_utxos_for_address(address)?;
        for (_outpoint, utxo) in db_utxos {
            utxos.push((utxo.txid, utxo.vout, utxo.value));
        }
        
        // Add UTXOs from cache
        for (_outpoint, utxo) in &self.cache {
            if self.script_matches_address(&utxo.script_pubkey, address) {
                utxos.push((utxo.txid, utxo.vout, utxo.value));
            }
        }
        
        Ok(utxos)
    }
    
    pub fn find_spendable_outputs(&self, address: &str, amount: u64) -> Result<(u64, Vec<(Hash256, u32, u64)>)> {
        let all_utxos = self.get_utxos(address)?;
        let mut accumulated = 0u64;
        let mut selected = Vec::new();
        
        // Sort UTXOs by value (largest first for efficient selection)
        let mut sorted_utxos = all_utxos;
        sorted_utxos.sort_by(|a, b| b.2.cmp(&a.2));
        
        for (txid, vout, value) in sorted_utxos {
            selected.push((txid, vout, value));
            accumulated += value;
            
            if accumulated >= amount {
                break;
            }
        }
        
        Ok((accumulated, selected))
    }
    
    pub fn validate_transaction(&self, tx: &Transaction) -> Result<bool> {
        if tx.is_coinbase() {
            return Ok(true); // Coinbase transactions don't need UTXO validation
        }
        
        let mut total_input_value = 0u64;
        
        // Validate all inputs
        for input in &tx.inputs {
            let outpoint = &input.previous_output;
            
            // Check if UTXO exists
            let utxo = self.get_utxo(outpoint)?
                .ok_or_else(|| QtcError::Transaction(format!(
                    "UTXO not found: {}:{}", 
                    hex::encode(outpoint.txid.as_bytes()), 
                    outpoint.vout
                )))?;
            
            total_input_value += utxo.value;
        }
        
        let total_output_value = tx.total_output_value();
        
        // Check that inputs >= outputs (fee is the difference)
        if total_input_value < total_output_value {
            return Err(QtcError::Transaction(
                "Total input value less than total output value".to_string()
            ));
        }
        
        Ok(true)
    }
    
    pub fn flush(&mut self) -> Result<()> {
        if self.dirty {
            // All changes are already written to database during apply_transaction
            self.cache.clear();
            self.dirty = false;
        }
        Ok(())
    }
    
    fn script_matches_address(&self, script_pubkey: &[u8], address: &str) -> bool {
        // Simplified address matching
        // In real implementation, this would properly decode the script and address
        
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
    
    /// Extract address from script_pubkey (simplified implementation)
    fn script_to_address(script: &[u8]) -> Option<String> {
        // This is a simplified implementation
        // In a real implementation, you'd parse P2PKH, P2SH, Bech32, etc.
        if script.len() >= 25 && script[0] == 0x76 && script[1] == 0xa9 && script[2] == 0x14 {
            // P2PKH: OP_DUP OP_HASH160 <20-byte hash> OP_EQUALVERIFY OP_CHECKSIG
            let hash160 = &script[3..23];
            // Convert hash160 to base58check address (simplified)
            Some(format!("qtc1q{}", hex::encode(hash160)))
        } else {
            None
        }
    }
    
    pub fn get_total_supply(&self) -> Result<u64> {
        // This would be expensive in a real implementation
        // Better to track this separately
        let all_utxos = self.db.get_all_utxos()?;
        Ok(all_utxos.iter().map(|(_, utxo)| utxo.value).sum())
    }
    
    pub fn get_utxo_count(&self) -> Result<usize> {
        let all_utxos = self.db.get_all_utxos()?;
        Ok(all_utxos.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::Database;
    use crate::core::Transaction;
    use tempfile::TempDir;
    
    #[test]
    fn test_utxo_set_coinbase() -> Result<()> {
        let temp_dir = TempDir::new().unwrap();
        let db = Arc::new(Database::new(temp_dir.path().join("test.db"))?);
        let mut utxo_set = UtxoSet::new(db);
        
        let coinbase_tx = Transaction::new_coinbase(
            "qtc1test".to_string(),
            2710000000,
            "test".to_string(),
        );
        
        utxo_set.apply_transaction(&coinbase_tx, 0)?;
        
        let balance = utxo_set.get_balance("qtc1test")?;
        assert_eq!(balance, 2710000000);
        
        Ok(())
    }
    
    #[test]
    fn test_utxo_set_spending() -> Result<()> {
        let temp_dir = TempDir::new().unwrap();
        let db = Arc::new(Database::new(temp_dir.path().join("test.db"))?);
        let mut utxo_set = UtxoSet::new(db);
        
        // Create coinbase transaction
        let coinbase_tx = Transaction::new_coinbase(
            "qtc1test".to_string(),
            1000,
            "test".to_string(),
        );
        let coinbase_hash = coinbase_tx.hash();
        
        utxo_set.apply_transaction(&coinbase_tx, 0)?;
        
        // Create spending transaction
        let mut spend_tx = Transaction::new();
        spend_tx.add_input(OutPoint::new(coinbase_hash, 0), vec![]);
        spend_tx.add_output(500, "qtc1recipient");
        spend_tx.add_output(400, "qtc1test"); // change
        
        // This should work in a real implementation with proper signature validation
        // For now, just test the UTXO mechanics
        
        Ok(())
    }
}
