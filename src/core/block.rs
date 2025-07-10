use crate::core::Transaction;
use crate::crypto::hash::{Hash256, Hashable};
use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Block {
    pub header: BlockHeader,
    pub transactions: Vec<Transaction>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockHeader {
    pub previous_hash: Hash256,
    pub merkle_root: Hash256,
    pub timestamp: u64,
    pub difficulty: u32,
    pub nonce: u64,
    pub height: u64,
}

impl Block {
    pub fn new(previous_hash: Hash256, transactions: Vec<Transaction>, difficulty: u32, height: u64) -> Self {
        let merkle_root = Self::calculate_merkle_root(&transactions);
        let timestamp = Utc::now().timestamp() as u64;
        
        Self {
            header: BlockHeader {
                previous_hash,
                merkle_root,
                timestamp,
                difficulty,
                nonce: 0,
                height,
            },
            transactions,
        }
    }
    
    pub fn calculate_merkle_root(transactions: &[Transaction]) -> Hash256 {
        if transactions.is_empty() {
            return Hash256::zero();
        }
        
        let mut hashes: Vec<Hash256> = transactions.iter().map(|tx| tx.hash()).collect();
        
        while hashes.len() > 1 {
            let mut next_level = Vec::new();
            
            for chunk in hashes.chunks(2) {
                let combined_bytes = if chunk.len() == 2 {
                    let mut bytes = Vec::new();
                    bytes.extend_from_slice(chunk[0].as_bytes());
                    bytes.extend_from_slice(chunk[1].as_bytes());
                    bytes
                } else {
                    // Duplicate the last hash if odd number
                    let mut bytes = Vec::new();
                    bytes.extend_from_slice(chunk[0].as_bytes());
                    bytes.extend_from_slice(chunk[0].as_bytes());
                    bytes
                };
                
                next_level.push(Hash256::hash(&combined_bytes));
            }
            
            hashes = next_level;
        }
        
        hashes[0]
    }
    
    pub fn set_nonce(&mut self, nonce: u64) {
        self.header.nonce = nonce;
    }
    
    pub fn increment_nonce(&mut self) {
        self.header.nonce = self.header.nonce.wrapping_add(1);
    }
    
    pub fn get_coinbase_transaction(&self) -> Option<&Transaction> {
        self.transactions.first()
    }
    
    pub fn total_fees(&self) -> u64 {
        self.transactions.iter()
            .skip(1) // Skip coinbase transaction
            .map(|tx| tx.fee())
            .sum()
    }
    
    pub fn size(&self) -> usize {
        bincode::serialize(self).map(|data| data.len()).unwrap_or(0)
    }
    
    pub fn transaction_count(&self) -> usize {
        self.transactions.len()
    }
    
    pub fn contains_transaction(&self, tx_hash: &Hash256) -> bool {
        self.transactions.iter().any(|tx| &tx.hash() == tx_hash)
    }
}

impl Hashable for Block {
    fn hash(&self) -> Hash256 {
        self.header.hash()
    }
}

impl Hashable for BlockHeader {
    fn hash(&self) -> Hash256 {
        let mut data = Vec::new();
        data.extend_from_slice(self.previous_hash.as_bytes());
        data.extend_from_slice(self.merkle_root.as_bytes());
        data.extend_from_slice(&self.timestamp.to_le_bytes());
        data.extend_from_slice(&self.difficulty.to_le_bytes());
        data.extend_from_slice(&self.nonce.to_le_bytes());
        data.extend_from_slice(&self.height.to_le_bytes());
        
        Hash256::hash(&data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::Transaction;
    
    #[test]
    fn test_block_creation() {
        let transactions = vec![
            Transaction::new_coinbase(
                "qtc1test".to_string(),
                2710000000,
                "Test coinbase".to_string(),
            )
        ];
        
        let block = Block::new(Hash256::zero(), transactions, 4, 0);
        
        assert_eq!(block.header.height, 0);
        assert_eq!(block.header.difficulty, 4);
        assert_eq!(block.header.previous_hash, Hash256::zero());
        assert_eq!(block.transactions.len(), 1);
    }
    
    #[test]
    fn test_merkle_root_calculation() {
        let transactions = vec![
            Transaction::new_coinbase("addr1".to_string(), 1000, "test".to_string()),
            Transaction::new_coinbase("addr2".to_string(), 1000, "test2".to_string()),
        ];
        
        let root = Block::calculate_merkle_root(&transactions);
        assert_ne!(root, Hash256::zero());
    }
}
