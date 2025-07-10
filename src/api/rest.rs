use crate::core::{Blockchain, Block, Transaction};
use crate::crypto::hash::Hashable;
use crate::crypto::hash::Hash256;
use crate::storage::Database;
use crate::config::ApiConfig;
use crate::{QtcError, Result};
use axum::{
    extract::{Path, Query, State},
    http::{StatusCode, HeaderMap},
    response::Json,
    routing::{get, post},
    Router,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use tower::ServiceBuilder;
use tower_http::cors::{CorsLayer, Any};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiResponse<T> {
    pub success: bool,
    pub data: Option<T>,
    pub error: Option<String>,
    pub timestamp: u64,
}

impl<T> ApiResponse<T> {
    pub fn success(data: T) -> Self {
        Self {
            success: true,
            data: Some(data),
            error: None,
            timestamp: chrono::Utc::now().timestamp() as u64,
        }
    }
    
    pub fn error(message: String) -> Self {
        Self {
            success: false,
            data: None,
            error: Some(message),
            timestamp: chrono::Utc::now().timestamp() as u64,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainInfo {
    pub height: u64,
    pub tip: String,
    pub difficulty: u32,
    pub total_supply: u64,
    pub total_work: u128,
    pub block_count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockInfo {
    pub hash: String,
    pub height: u64,
    pub previous_hash: String,
    pub merkle_root: String,
    pub timestamp: u64,
    pub difficulty: u32,
    pub nonce: u64,
    pub size: usize,
    pub transaction_count: usize,
    pub transactions: Vec<String>, // Transaction hashes
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionInfo {
    pub hash: String,
    pub version: u32,
    pub lock_time: u64,
    pub size: usize,
    pub input_count: usize,
    pub output_count: usize,
    pub total_input_value: u64,
    pub total_output_value: u64,
    pub fee: u64,
    pub is_coinbase: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddressInfo {
    pub address: String,
    pub balance: u64,
    pub transaction_count: u64,
    pub received: u64,
    pub sent: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UtxoInfo {
    pub txid: String,
    pub vout: u32,
    pub value: u64,
    pub height: u64,
    pub confirmations: u64,
    pub is_coinbase: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MempoolInfo {
    pub size: usize,
    pub bytes: usize,
    pub usage: usize,
    pub max_mempool: usize,
    pub fee_rate: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkInfo {
    pub version: String,
    pub protocol_version: u32,
    pub connections: usize,
    pub networks: Vec<String>,
    pub relay_fee: u64,
    pub incremental_fee: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MiningInfo {
    pub blocks: u64,
    pub difficulty: u32,
    pub network_hashrate: f64,
    pub pooled_tx: usize,
    pub chain: String,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SendTransactionRequest {
    pub raw_transaction: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BlockQuery {
    pub verbose: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TransactionQuery {
    pub verbose: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BlocksQuery {
    pub limit: Option<u64>,
    pub offset: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct AppState {
    pub blockchain: Arc<RwLock<Blockchain>>,
    pub db: Arc<Database>,
}

pub struct RestApi {
    blockchain: Arc<RwLock<Blockchain>>,
    db: Arc<Database>,
    config: ApiConfig,
}

impl RestApi {
    pub fn new(blockchain: Arc<RwLock<Blockchain>>, config: ApiConfig) -> Self {
        let db = Arc::new(Database::new("qtc.db").expect("Failed to initialize database"));
        
        Self {
            blockchain,
            db,
            config,
        }
    }
    
    pub async fn start(self) -> Result<()> {
        log::info!("🚀 Starting QTC REST API on port {}", self.config.rest_port);
        
        let state = AppState {
            blockchain: self.blockchain.clone(),
            db: self.db.clone(),
        };
        
        let app = self.create_router(state);
        let addr = format!("0.0.0.0:{}", self.config.rest_port);
        let listener = tokio::net::TcpListener::bind(&addr).await
            .map_err(|e| QtcError::Network(format!("Failed to bind to {}: {}", addr, e)))?;
        
        log::info!("✅ REST API listening on http://{}", addr);
        
        axum::serve(listener, app).await
            .map_err(|e| QtcError::Network(format!("Server error: {}", e)))?;
        
        Ok(())
    }
    
    fn create_router(&self, state: AppState) -> Router {
        let cors = CorsLayer::new()
            .allow_methods(Any)
            .allow_headers(Any)
            .allow_origin(Any);
        
        Router::new()
            // Blockchain info endpoints
            .route("/api/v1/info", get(get_chain_info))
            .route("/api/v1/stats", get(get_chain_stats))
            
            // Block endpoints
            .route("/api/v1/blocks", get(get_blocks))
            .route("/api/v1/blocks/latest", get(get_latest_block))
            .route("/api/v1/blocks/height/:height", get(get_block_by_height))
            .route("/api/v1/blocks/:hash", get(get_block_by_hash))
            
            // Transaction endpoints
            .route("/api/v1/transactions", post(send_transaction))
            .route("/api/v1/transactions/:hash", get(get_transaction))
            .route("/api/v1/transactions/raw/:hash", get(get_raw_transaction))
            
            // Address endpoints
            .route("/api/v1/addresses/:address", get(get_address_info))
            .route("/api/v1/addresses/:address/balance", get(get_address_balance))
            .route("/api/v1/addresses/:address/utxos", get(get_address_utxos))
            .route("/api/v1/addresses/:address/transactions", get(get_address_transactions))
            
            // Mempool endpoints
            .route("/api/v1/mempool", get(get_mempool_info))
            .route("/api/v1/mempool/transactions", get(get_mempool_transactions))
            
            // Network endpoints
            .route("/api/v1/network", get(get_network_info))
            .route("/api/v1/peers", get(get_peers))
            
            // Mining endpoints
            .route("/api/v1/mining", get(get_mining_info))
            .route("/api/v1/mining/difficulty", get(get_difficulty))
            
            // Utility endpoints
            .route("/api/v1/validate/address/:address", get(validate_address))
            .route("/api/v1/fee/estimate", get(estimate_fee))
            
            // Health check
            .route("/health", get(health_check))
            .route("/", get(api_root))
            
            .layer(ServiceBuilder::new().layer(cors))
            .with_state(state)
    }
}

// Handler functions

async fn api_root() -> Json<ApiResponse<HashMap<String, String>>> {
    let mut info = HashMap::new();
    info.insert("name".to_string(), "Quantum Goldchain API".to_string());
    info.insert("version".to_string(), "1.0.0".to_string());
    info.insert("description".to_string(), "QTC Blockchain REST API".to_string());
    
    Json(ApiResponse::success(info))
}

async fn health_check() -> Json<ApiResponse<HashMap<String, String>>> {
    let mut status = HashMap::new();
    status.insert("status".to_string(), "healthy".to_string());
    status.insert("timestamp".to_string(), chrono::Utc::now().to_rfc3339());
    
    Json(ApiResponse::success(status))
}

async fn get_chain_info(State(state): State<AppState>) -> Json<ApiResponse<ChainInfo>> {
    match state.blockchain.read() {
        Ok(blockchain) => {
            match blockchain.get_chain_info() {
                Ok(chain_state) => {
                    let info = ChainInfo {
                        height: chain_state.height,
                        tip: chain_state.tip.to_hex(),
                        difficulty: chain_state.difficulty,
                        total_supply: chain_state.total_supply,
                        total_work: chain_state.total_work,
                        block_count: chain_state.height + 1,
                    };
                    Json(ApiResponse::success(info))
                }
                Err(e) => Json(ApiResponse::error(format!("Failed to get chain info: {}", e))),
            }
        }
        Err(_) => Json(ApiResponse::error("Failed to access blockchain".to_string())),
    }
}

async fn get_chain_stats(State(state): State<AppState>) -> Json<ApiResponse<HashMap<String, serde_json::Value>>> {
    let mut stats = HashMap::new();
    
    if let Ok(blockchain) = state.blockchain.read() {
        if let Ok(chain_info) = blockchain.get_chain_info() {
            stats.insert("height".to_string(), serde_json::Value::from(chain_info.height));
            stats.insert("difficulty".to_string(), serde_json::Value::from(chain_info.difficulty));
            stats.insert("total_supply".to_string(), serde_json::Value::from(chain_info.total_supply));
        }
    }
    
    if let Ok(db_stats) = state.db.get_database_stats() {
        stats.insert("utxo_count".to_string(), serde_json::Value::from(db_stats.utxo_count));
        stats.insert("database_size".to_string(), serde_json::Value::from(db_stats.total_size));
    }
    
    Json(ApiResponse::success(stats))
}

async fn get_blocks(
    State(state): State<AppState>,
    Query(query): Query<BlocksQuery>,
) -> Json<ApiResponse<Vec<BlockInfo>>> {
    let limit = query.limit.unwrap_or(10).min(100); // Max 100 blocks
    let offset = query.offset.unwrap_or(0);
    
    match state.blockchain.read() {
        Ok(blockchain) => {
            let current_height = blockchain.height;
            let start_height = current_height.saturating_sub(offset + limit - 1);
            let end_height = current_height.saturating_sub(offset);
            
            let mut blocks = Vec::new();
            
            for height in start_height..=end_height {
                if let Ok(Some(block)) = blockchain.get_block_by_height(height) {
                    let block_info = BlockInfo {
                        hash: block.hash().to_hex(),
                        height: block.header.height,
                        previous_hash: block.header.previous_hash.to_hex(),
                        merkle_root: block.header.merkle_root.to_hex(),
                        timestamp: block.header.timestamp,
                        difficulty: block.header.difficulty,
                        nonce: block.header.nonce,
                        size: block.size(),
                        transaction_count: block.transactions.len(),
                        transactions: block.transactions.iter().map(|tx| tx.hash().to_hex()).collect(),
                    };
                    blocks.push(block_info);
                }
            }
            
            blocks.reverse(); // Newest first
            Json(ApiResponse::success(blocks))
        }
        Err(_) => Json(ApiResponse::error("Failed to access blockchain".to_string())),
    }
}

async fn get_latest_block(State(state): State<AppState>) -> Json<ApiResponse<BlockInfo>> {
    match state.blockchain.read() {
        Ok(blockchain) => {
            let height = blockchain.height;
            match blockchain.get_block_by_height(height) {
                Ok(Some(block)) => {
                    let block_info = BlockInfo {
                        hash: block.hash().to_hex(),
                        height: block.header.height,
                        previous_hash: block.header.previous_hash.to_hex(),
                        merkle_root: block.header.merkle_root.to_hex(),
                        timestamp: block.header.timestamp,
                        difficulty: block.header.difficulty,
                        nonce: block.header.nonce,
                        size: block.size(),
                        transaction_count: block.transactions.len(),
                        transactions: block.transactions.iter().map(|tx| tx.hash().to_hex()).collect(),
                    };
                    Json(ApiResponse::success(block_info))
                }
                Ok(None) => Json(ApiResponse::error("Latest block not found".to_string())),
                Err(e) => Json(ApiResponse::error(format!("Failed to get latest block: {}", e))),
            }
        }
        Err(_) => Json(ApiResponse::error("Failed to access blockchain".to_string())),
    }
}

async fn get_block_by_height(
    State(state): State<AppState>,
    Path(height): Path<u64>,
    Query(_query): Query<BlockQuery>,
) -> Json<ApiResponse<BlockInfo>> {
    match state.blockchain.read() {
        Ok(blockchain) => {
            match blockchain.get_block_by_height(height) {
                Ok(Some(block)) => {
                    let block_info = BlockInfo {
                        hash: block.hash().to_hex(),
                        height: block.header.height,
                        previous_hash: block.header.previous_hash.to_hex(),
                        merkle_root: block.header.merkle_root.to_hex(),
                        timestamp: block.header.timestamp,
                        difficulty: block.header.difficulty,
                        nonce: block.header.nonce,
                        size: block.size(),
                        transaction_count: block.transactions.len(),
                        transactions: block.transactions.iter().map(|tx| tx.hash().to_hex()).collect(),
                    };
                    Json(ApiResponse::success(block_info))
                }
                Ok(None) => Json(ApiResponse::error("Block not found".to_string())),
                Err(e) => Json(ApiResponse::error(format!("Failed to get block: {}", e))),
            }
        }
        Err(_) => Json(ApiResponse::error("Failed to access blockchain".to_string())),
    }
}

async fn get_block_by_hash(
    State(state): State<AppState>,
    Path(hash_str): Path<String>,
    Query(_query): Query<BlockQuery>,
) -> Json<ApiResponse<BlockInfo>> {
    let hash = match Hash256::from_hex(&hash_str) {
        Ok(hash) => hash,
        Err(_) => return Json(ApiResponse::error("Invalid block hash".to_string())),
    };
    
    match state.blockchain.read() {
        Ok(blockchain) => {
            match blockchain.get_block(&hash) {
                Ok(Some(block)) => {
                    let block_info = BlockInfo {
                        hash: block.hash().to_hex(),
                        height: block.header.height,
                        previous_hash: block.header.previous_hash.to_hex(),
                        merkle_root: block.header.merkle_root.to_hex(),
                        timestamp: block.header.timestamp,
                        difficulty: block.header.difficulty,
                        nonce: block.header.nonce,
                        size: block.size(),
                        transaction_count: block.transactions.len(),
                        transactions: block.transactions.iter().map(|tx| tx.hash().to_hex()).collect(),
                    };
                    Json(ApiResponse::success(block_info))
                }
                Ok(None) => Json(ApiResponse::error("Block not found".to_string())),
                Err(e) => Json(ApiResponse::error(format!("Failed to get block: {}", e))),
            }
        }
        Err(_) => Json(ApiResponse::error("Failed to access blockchain".to_string())),
    }
}

async fn get_transaction(
    State(state): State<AppState>,
    Path(hash_str): Path<String>,
    Query(_query): Query<TransactionQuery>,
) -> Json<ApiResponse<TransactionInfo>> {
    let hash = match Hash256::from_hex(&hash_str) {
        Ok(hash) => hash,
        Err(_) => return Json(ApiResponse::error("Invalid transaction hash".to_string())),
    };
    
    match state.db.get_transaction(&hash) {
        Ok(Some(tx)) => {
            let tx_info = TransactionInfo {
                hash: tx.hash().to_hex(),
                version: tx.version,
                lock_time: tx.lock_time,
                size: tx.size(),
                input_count: tx.inputs.len(),
                output_count: tx.outputs.len(),
                total_input_value: tx.total_input_value(),
                total_output_value: tx.total_output_value(),
                fee: tx.fee(),
                is_coinbase: tx.is_coinbase(),
            };
            Json(ApiResponse::success(tx_info))
        }
        Ok(None) => Json(ApiResponse::error("Transaction not found".to_string())),
        Err(e) => Json(ApiResponse::error(format!("Failed to get transaction: {}", e))),
    }
}

async fn get_raw_transaction(
    State(state): State<AppState>,
    Path(hash_str): Path<String>,
) -> Json<ApiResponse<String>> {
    let hash = match Hash256::from_hex(&hash_str) {
        Ok(hash) => hash,
        Err(_) => return Json(ApiResponse::error("Invalid transaction hash".to_string())),
    };
    
    match state.db.get_transaction(&hash) {
        Ok(Some(tx)) => {
            match bincode::serialize(&tx) {
                Ok(raw_tx) => {
                    let hex_tx = hex::encode(raw_tx);
                    Json(ApiResponse::success(hex_tx))
                }
                Err(e) => Json(ApiResponse::error(format!("Failed to serialize transaction: {}", e))),
            }
        }
        Ok(None) => Json(ApiResponse::error("Transaction not found".to_string())),
        Err(e) => Json(ApiResponse::error(format!("Failed to get transaction: {}", e))),
    }
}

async fn send_transaction(
    State(state): State<AppState>,
    Json(req): Json<SendTransactionRequest>,
) -> Json<ApiResponse<String>> {
    // Decode the raw transaction
    let raw_bytes = match hex::decode(&req.raw_transaction) {
        Ok(bytes) => bytes,
        Err(_) => return Json(ApiResponse::error("Invalid hex encoding".to_string())),
    };
    
    let tx: Transaction = match bincode::deserialize(&raw_bytes) {
        Ok(tx) => tx,
        Err(e) => return Json(ApiResponse::error(format!("Failed to deserialize transaction: {}", e))),
    };
    
    // Validate transaction
    match state.blockchain.read() {
        Ok(blockchain) => {
            match blockchain.is_valid_transaction(&tx) {
                Ok(true) => {
                    // Save transaction to database (in real implementation, would add to mempool)
                    if let Err(e) = state.db.save_transaction(&tx) {
                        return Json(ApiResponse::error(format!("Failed to save transaction: {}", e)));
                    }
                    
                    let tx_hash = tx.hash().to_hex();
                    Json(ApiResponse::success(tx_hash))
                }
                Ok(false) => Json(ApiResponse::error("Invalid transaction".to_string())),
                Err(e) => Json(ApiResponse::error(format!("Transaction validation failed: {}", e))),
            }
        }
        Err(_) => Json(ApiResponse::error("Failed to access blockchain".to_string())),
    }
}

async fn get_address_info(
    State(state): State<AppState>,
    Path(address): Path<String>,
) -> Json<ApiResponse<AddressInfo>> {
    if !crate::crypto::keys::is_valid_address(&address) {
        return Json(ApiResponse::error("Invalid address".to_string()));
    }
    
    match state.blockchain.read() {
        Ok(blockchain) => {
            match blockchain.get_balance(&address) {
                Ok(balance) => {
                    let info = AddressInfo {
                        address: address.clone(),
                        balance,
                        transaction_count: 0, // Would be calculated in full implementation
                        received: balance,    // Simplified
                        sent: 0,             // Would be calculated in full implementation
                    };
                    Json(ApiResponse::success(info))
                }
                Err(e) => Json(ApiResponse::error(format!("Failed to get address info: {}", e))),
            }
        }
        Err(_) => Json(ApiResponse::error("Failed to access blockchain".to_string())),
    }
}

async fn get_address_balance(
    State(state): State<AppState>,
    Path(address): Path<String>,
) -> Json<ApiResponse<u64>> {
    if !crate::crypto::keys::is_valid_address(&address) {
        return Json(ApiResponse::error("Invalid address".to_string()));
    }
    
    match state.blockchain.read() {
        Ok(blockchain) => {
            match blockchain.get_balance(&address) {
                Ok(balance) => Json(ApiResponse::success(balance)),
                Err(e) => Json(ApiResponse::error(format!("Failed to get balance: {}", e))),
            }
        }
        Err(_) => Json(ApiResponse::error("Failed to access blockchain".to_string())),
    }
}

async fn get_address_utxos(
    State(state): State<AppState>,
    Path(address): Path<String>,
) -> Json<ApiResponse<Vec<UtxoInfo>>> {
    if !crate::crypto::keys::is_valid_address(&address) {
        return Json(ApiResponse::error("Invalid address".to_string()));
    }
    
    match state.blockchain.read() {
        Ok(blockchain) => {
            match blockchain.get_utxos(&address) {
                Ok(utxos) => {
                    let current_height = blockchain.height;
                    let utxo_infos: Vec<UtxoInfo> = utxos.into_iter().map(|(txid, vout, value)| {
                        UtxoInfo {
                            txid: txid.to_hex(),
                            vout,
                            value,
                            height: 0, // Would be looked up in full implementation
                            confirmations: current_height, // Simplified
                            is_coinbase: false, // Would be determined in full implementation
                        }
                    }).collect();
                    
                    Json(ApiResponse::success(utxo_infos))
                }
                Err(e) => Json(ApiResponse::error(format!("Failed to get UTXOs: {}", e))),
            }
        }
        Err(_) => Json(ApiResponse::error("Failed to access blockchain".to_string())),
    }
}

async fn get_address_transactions(
    State(_state): State<AppState>,
    Path(_address): Path<String>,
) -> Json<ApiResponse<Vec<String>>> {
    // Transaction history lookup would be implemented here
    Json(ApiResponse::success(Vec::new()))
}

async fn get_mempool_info(State(_state): State<AppState>) -> Json<ApiResponse<MempoolInfo>> {
    // Mempool would be implemented in full version
    let info = MempoolInfo {
        size: 0,
        bytes: 0,
        usage: 0,
        max_mempool: 300_000_000, // 300MB
        fee_rate: 1000, // 1000 satoshis per byte
    };
    
    Json(ApiResponse::success(info))
}

async fn get_mempool_transactions(State(_state): State<AppState>) -> Json<ApiResponse<Vec<String>>> {
    // Mempool transactions would be returned here
    Json(ApiResponse::success(Vec::new()))
}

async fn get_network_info(State(_state): State<AppState>) -> Json<ApiResponse<NetworkInfo>> {
    let info = NetworkInfo {
        version: "1.0.0".to_string(),
        protocol_version: 1,
        connections: 0, // Would be fetched from P2P layer
        networks: vec!["qtc".to_string()],
        relay_fee: 1000,
        incremental_fee: 1000,
    };
    
    Json(ApiResponse::success(info))
}

async fn get_peers(State(_state): State<AppState>) -> Json<ApiResponse<Vec<HashMap<String, serde_json::Value>>>> {
    // Peer information would be fetched from P2P layer
    Json(ApiResponse::success(Vec::new()))
}

async fn get_mining_info(State(state): State<AppState>) -> Json<ApiResponse<MiningInfo>> {
    match state.blockchain.read() {
        Ok(blockchain) => {
            let chain_info = blockchain.get_chain_info().unwrap_or_default();
            
            let info = MiningInfo {
                blocks: chain_info.height,
                difficulty: chain_info.difficulty,
                network_hashrate: 0.0, // Would be calculated
                pooled_tx: 0, // Mempool size
                chain: "qtc".to_string(),
                warnings: Vec::new(),
            };
            
            Json(ApiResponse::success(info))
        }
        Err(_) => Json(ApiResponse::error("Failed to access blockchain".to_string())),
    }
}

async fn get_difficulty(State(state): State<AppState>) -> Json<ApiResponse<u32>> {
    match state.blockchain.read() {
        Ok(blockchain) => {
            match blockchain.get_current_difficulty() {
                Ok(difficulty) => Json(ApiResponse::success(difficulty)),
                Err(e) => Json(ApiResponse::error(format!("Failed to get difficulty: {}", e))),
            }
        }
        Err(_) => Json(ApiResponse::error("Failed to access blockchain".to_string())),
    }
}

async fn validate_address(
    Path(address): Path<String>,
) -> Json<ApiResponse<HashMap<String, serde_json::Value>>> {
    let is_valid = crate::crypto::keys::is_valid_address(&address);
    
    let mut result = HashMap::new();
    result.insert("address".to_string(), serde_json::Value::String(address));
    result.insert("is_valid".to_string(), serde_json::Value::Bool(is_valid));
    
    if is_valid {
        result.insert("type".to_string(), serde_json::Value::String("pubkeyhash".to_string()));
        result.insert("network".to_string(), serde_json::Value::String("qtc".to_string()));
    }
    
    Json(ApiResponse::success(result))
}

async fn estimate_fee(State(_state): State<AppState>) -> Json<ApiResponse<HashMap<String, u64>>> {
    let mut fees = HashMap::new();
    fees.insert("fast".to_string(), 5000);     // 5000 sat/byte
    fees.insert("medium".to_string(), 2000);   // 2000 sat/byte
    fees.insert("slow".to_string(), 1000);     // 1000 sat/byte
    
    Json(ApiResponse::success(fees))
}
