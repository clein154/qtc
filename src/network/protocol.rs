use crate::core::{Block, Transaction, Blockchain};
use crate::crypto::hash::Hash256;
use crate::{QtcError, Result};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, RwLock};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub message_type: MessageType,
    pub timestamp: u64,
    pub version: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MessageType {
    // Blockchain sync messages
    GetBlocks {
        start_height: u64,
        end_height: u64,
        locator_hashes: Vec<Hash256>,
    },
    Block(Block),
    GetBlockHeaders {
        start_height: u64,
        count: u32,
    },
    BlockHeaders(Vec<crate::core::BlockHeader>),
    
    // Transaction messages
    Transaction(Transaction),
    GetMempool,
    Mempool(Vec<Transaction>),
    
    // Peer discovery
    Version {
        version: u32,
        services: u64,
        timestamp: u64,
        addr_recv: String,
        addr_from: String,
        nonce: u64,
        user_agent: String,
        start_height: u64,
    },
    VerAck,
    
    // Network status
    Ping(u64),
    Pong(u64),
    GetAddr,
    Addr(Vec<PeerAddress>),
    
    // Inventory messages
    Inv(Vec<InventoryItem>),
    GetData(Vec<InventoryItem>),
    NotFound(Vec<InventoryItem>),
    
    // Error handling
    Reject {
        message: String,
        code: u8,
        reason: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerAddress {
    pub timestamp: u64,
    pub services: u64,
    pub ip: String,
    pub port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InventoryItem {
    pub item_type: InventoryType,
    pub hash: Hash256,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum InventoryType {
    Transaction,
    Block,
    FilteredBlock,
}

pub struct ProtocolHandler {
    blockchain: Arc<RwLock<Blockchain>>,
    version: u32,
    user_agent: String,
}

impl Message {
    pub fn new(message_type: MessageType) -> Self {
        Self {
            message_type,
            timestamp: chrono::Utc::now().timestamp() as u64,
            version: 1,
        }
    }
    
    pub fn serialize(&self) -> Result<Vec<u8>> {
        bincode::serialize(self)
            .map_err(|e| QtcError::Network(format!("Failed to serialize message: {}", e)))
    }
    
    pub fn deserialize(data: &[u8]) -> Result<Self> {
        bincode::deserialize(data)
            .map_err(|e| QtcError::Network(format!("Failed to deserialize message: {}", e)))
    }
    
    pub fn message_type_name(&self) -> &'static str {
        match &self.message_type {
            MessageType::GetBlocks { .. } => "getblocks",
            MessageType::Block(_) => "block",
            MessageType::GetBlockHeaders { .. } => "getheaders",
            MessageType::BlockHeaders(_) => "headers",
            MessageType::Transaction(_) => "tx",
            MessageType::GetMempool => "getmempool",
            MessageType::Mempool(_) => "mempool",
            MessageType::Version { .. } => "version",
            MessageType::VerAck => "verack",
            MessageType::Ping(_) => "ping",
            MessageType::Pong(_) => "pong",
            MessageType::GetAddr => "getaddr",
            MessageType::Addr(_) => "addr",
            MessageType::Inv(_) => "inv",
            MessageType::GetData(_) => "getdata",
            MessageType::NotFound(_) => "notfound",
            MessageType::Reject { .. } => "reject",
        }
    }
}

impl ProtocolHandler {
    pub fn new(blockchain: Arc<RwLock<Blockchain>>) -> Self {
        Self {
            blockchain,
            version: 1,
            user_agent: "QTC/1.0.0".to_string(),
        }
    }
    
    pub async fn handle_message(&self, message: Message, peer_id: &str) -> Result<Option<Message>> {
        log::debug!("📨 Handling {} message from peer {}", message.message_type_name(), peer_id);
        
        match message.message_type {
            MessageType::GetBlocks { start_height, end_height, locator_hashes } => {
                self.handle_get_blocks(start_height, end_height, locator_hashes).await
            }
            
            MessageType::Block(block) => {
                self.handle_block(block).await
            }
            
            MessageType::GetBlockHeaders { start_height, count } => {
                self.handle_get_block_headers(start_height, count).await
            }
            
            MessageType::Transaction(tx) => {
                self.handle_transaction(tx).await
            }
            
            MessageType::GetMempool => {
                self.handle_get_mempool().await
            }
            
            MessageType::Version { version, start_height, .. } => {
                self.handle_version(version, start_height, peer_id).await
            }
            
            MessageType::Ping(nonce) => {
                Ok(Some(Message::new(MessageType::Pong(nonce))))
            }
            
            MessageType::Pong(_) => {
                // Pong received, update peer stats
                Ok(None)
            }
            
            MessageType::GetAddr => {
                self.handle_get_addr().await
            }
            
            MessageType::Inv(items) => {
                self.handle_inv(items).await
            }
            
            MessageType::GetData(items) => {
                self.handle_get_data(items).await
            }
            
            _ => {
                log::debug!("📭 Unhandled message type: {}", message.message_type_name());
                Ok(None)
            }
        }
    }
    
    async fn handle_get_blocks(
        &self,
        start_height: u64,
        end_height: u64,
        _locator_hashes: Vec<Hash256>,
    ) -> Result<Option<Message>> {
        log::debug!("📦 Handling getblocks request: {} to {}", start_height, end_height);
        
        let blockchain = self.blockchain.read().unwrap();
        let max_blocks = 500; // Limit response size
        let actual_end = end_height.min(start_height + max_blocks);
        
        let mut inventory_items = Vec::new();
        
        for height in start_height..=actual_end {
            if let Ok(Some(block)) = blockchain.get_block_by_height(height) {
                inventory_items.push(InventoryItem {
                    item_type: InventoryType::Block,
                    hash: block.hash(),
                });
            }
        }
        
        if inventory_items.is_empty() {
            Ok(None)
        } else {
            Ok(Some(Message::new(MessageType::Inv(inventory_items))))
        }
    }
    
    async fn handle_block(&self, block: Block) -> Result<Option<Message>> {
        log::info!("📦 Received block at height {}", block.header.height);
        
        // Validate and add block to blockchain
        let mut blockchain = self.blockchain.write().unwrap();
        
        match blockchain.add_block(block.clone()) {
            Ok(()) => {
                log::info!("✅ Successfully added block {}", block.header.height);
                
                // Broadcast the block to other peers (would be handled by P2P layer)
                Ok(None)
            }
            Err(e) => {
                log::warn!("❌ Failed to add block: {}", e);
                
                // Send reject message
                Ok(Some(Message::new(MessageType::Reject {
                    message: "block".to_string(),
                    code: 0x10, // Invalid block
                    reason: e.to_string(),
                })))
            }
        }
    }
    
    async fn handle_get_block_headers(
        &self,
        start_height: u64,
        count: u32,
    ) -> Result<Option<Message>> {
        log::debug!("📋 Handling getheaders request: start={}, count={}", start_height, count);
        
        let blockchain = self.blockchain.read().unwrap();
        let max_headers = 2000u32.min(count);
        let mut headers = Vec::new();
        
        for i in 0..max_headers {
            let height = start_height + i as u64;
            if let Ok(Some(block)) = blockchain.get_block_by_height(height) {
                headers.push(block.header);
            } else {
                break;
            }
        }
        
        if headers.is_empty() {
            Ok(None)
        } else {
            Ok(Some(Message::new(MessageType::BlockHeaders(headers))))
        }
    }
    
    async fn handle_transaction(&self, tx: Transaction) -> Result<Option<Message>> {
        log::debug!("💰 Received transaction: {}", hex::encode(tx.hash().as_bytes()));
        
        // Validate transaction
        let blockchain = self.blockchain.read().unwrap();
        
        match blockchain.is_valid_transaction(&tx) {
            Ok(true) => {
                log::debug!("✅ Transaction is valid");
                // Add to mempool (would be handled by mempool component)
                Ok(None)
            }
            Ok(false) | Err(_) => {
                log::warn!("❌ Invalid transaction received");
                
                Ok(Some(Message::new(MessageType::Reject {
                    message: "tx".to_string(),
                    code: 0x01, // Invalid transaction
                    reason: "Transaction validation failed".to_string(),
                })))
            }
        }
    }
    
    async fn handle_get_mempool(&self) -> Result<Option<Message>> {
        log::debug!("🗂️ Handling getmempool request");
        
        // In a full implementation, this would return mempool transactions
        // For now, return empty mempool
        Ok(Some(Message::new(MessageType::Mempool(vec![]))))
    }
    
    async fn handle_version(
        &self,
        peer_version: u32,
        peer_height: u64,
        peer_id: &str,
    ) -> Result<Option<Message>> {
        log::info!("🤝 Received version from peer {}: version={}, height={}", 
                  peer_id, peer_version, peer_height);
        
        // Check version compatibility
        if peer_version < self.version {
            log::warn!("⚠️ Peer {} has older version {}", peer_id, peer_version);
        }
        
        // Send version acknowledgment
        Ok(Some(Message::new(MessageType::VerAck)))
    }
    
    async fn handle_get_addr(&self) -> Result<Option<Message>> {
        log::debug!("📍 Handling getaddr request");
        
        // In a full implementation, this would return known peer addresses
        // For now, return empty address list
        Ok(Some(Message::new(MessageType::Addr(vec![]))))
    }
    
    async fn handle_inv(&self, items: Vec<InventoryItem>) -> Result<Option<Message>> {
        log::debug!("📋 Received inventory with {} items", items.len());
        
        let mut get_data_items = Vec::new();
        let blockchain = self.blockchain.read().unwrap();
        
        for item in items {
            match item.item_type {
                InventoryType::Block => {
                    // Check if we already have this block
                    if blockchain.get_block(&item.hash)?.is_none() {
                        get_data_items.push(item);
                    }
                }
                InventoryType::Transaction => {
                    // Check if we already have this transaction
                    // For now, always request transactions
                    get_data_items.push(item);
                }
                InventoryType::FilteredBlock => {
                    // Not implemented yet
                }
            }
        }
        
        if get_data_items.is_empty() {
            Ok(None)
        } else {
            Ok(Some(Message::new(MessageType::GetData(get_data_items))))
        }
    }
    
    async fn handle_get_data(&self, items: Vec<InventoryItem>) -> Result<Option<Message>> {
        log::debug!("📤 Handling getdata request for {} items", items.len());
        
        let blockchain = self.blockchain.read().unwrap();
        let mut not_found = Vec::new();
        
        for item in items {
            match item.item_type {
                InventoryType::Block => {
                    if let Ok(Some(block)) = blockchain.get_block(&item.hash) {
                        // Send the block (would normally send to requesting peer directly)
                        log::debug!("📦 Sending block {}", item.hash);
                        // Return the block message
                        return Ok(Some(Message::new(MessageType::Block(block))));
                    } else {
                        not_found.push(item);
                    }
                }
                InventoryType::Transaction => {
                    // Look up transaction (not implemented in this simple version)
                    not_found.push(item);
                }
                InventoryType::FilteredBlock => {
                    not_found.push(item);
                }
            }
        }
        
        if not_found.is_empty() {
            Ok(None)
        } else {
            Ok(Some(Message::new(MessageType::NotFound(not_found))))
        }
    }
    
    pub fn create_version_message(&self, peer_addr: &str) -> Message {
        let blockchain = self.blockchain.read().unwrap();
        
        Message::new(MessageType::Version {
            version: self.version,
            services: 0, // No special services
            timestamp: chrono::Utc::now().timestamp() as u64,
            addr_recv: peer_addr.to_string(),
            addr_from: "127.0.0.1:8333".to_string(), // Our address
            nonce: rand::random(),
            user_agent: self.user_agent.clone(),
            start_height: blockchain.height,
        })
    }
    
    pub fn create_ping_message() -> Message {
        Message::new(MessageType::Ping(rand::random()))
    }
    
    pub fn validate_message(&self, message: &Message) -> Result<()> {
        // Check message version
        if message.version > self.version {
            return Err(QtcError::Network(
                "Unsupported message version".to_string()
            ));
        }
        
        // Check timestamp (shouldn't be too far in the future)
        let now = chrono::Utc::now().timestamp() as u64;
        let max_future_time = 2 * 60 * 60; // 2 hours
        
        if message.timestamp > now + max_future_time {
            return Err(QtcError::Network(
                "Message timestamp too far in the future".to_string()
            ));
        }
        
        // Validate specific message types
        match &message.message_type {
            MessageType::Block(block) => {
                if block.transactions.is_empty() {
                    return Err(QtcError::Network(
                        "Block must contain at least coinbase transaction".to_string()
                    ));
                }
            }
            
            MessageType::Transaction(tx) => {
                if tx.inputs.is_empty() && !tx.is_coinbase() {
                    return Err(QtcError::Network(
                        "Non-coinbase transaction must have inputs".to_string()
                    ));
                }
                
                if tx.outputs.is_empty() {
                    return Err(QtcError::Network(
                        "Transaction must have outputs".to_string()
                    ));
                }
            }
            
            _ => {} // Other message types are valid by default
        }
        
        Ok(())
    }
}

// Utility functions for protocol handling
impl InventoryItem {
    pub fn new_block(hash: Hash256) -> Self {
        Self {
            item_type: InventoryType::Block,
            hash,
        }
    }
    
    pub fn new_transaction(hash: Hash256) -> Self {
        Self {
            item_type: InventoryType::Transaction,
            hash,
        }
    }
}

impl PeerAddress {
    pub fn new(ip: String, port: u16) -> Self {
        Self {
            timestamp: chrono::Utc::now().timestamp() as u64,
            services: 0,
            ip,
            port,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::Database;
    use tempfile::TempDir;
    
    #[tokio::test]
    async fn test_protocol_handler_creation() -> Result<()> {
        let temp_dir = TempDir::new().unwrap();
        let db = Arc::new(Database::new(temp_dir.path().join("test.db"))?);
        let blockchain = Arc::new(RwLock::new(Blockchain::new(db)?));
        
        let handler = ProtocolHandler::new(blockchain);
        assert_eq!(handler.version, 1);
        assert_eq!(handler.user_agent, "QTC/1.0.0");
        
        Ok(())
    }
    
    #[test]
    fn test_message_serialization() -> Result<()> {
        let msg = Message::new(MessageType::Ping(12345));
        let serialized = msg.serialize()?;
        let deserialized = Message::deserialize(&serialized)?;
        
        assert_eq!(msg.version, deserialized.version);
        assert_eq!(msg.message_type_name(), deserialized.message_type_name());
        
        Ok(())
    }
    
    #[test]
    fn test_inventory_item_creation() {
        let hash = Hash256::hash(b"test");
        
        let block_inv = InventoryItem::new_block(hash);
        assert!(matches!(block_inv.item_type, InventoryType::Block));
        assert_eq!(block_inv.hash, hash);
        
        let tx_inv = InventoryItem::new_transaction(hash);
        assert!(matches!(tx_inv.item_type, InventoryType::Transaction));
        assert_eq!(tx_inv.hash, hash);
    }
}
