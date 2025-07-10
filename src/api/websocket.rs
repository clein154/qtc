use crate::core::{Blockchain, Block, Transaction};
use crate::crypto::hash::Hashable;
use crate::network::protocol::Message;
use crate::{QtcError, Result};
use axum::{
    extract::{
        ws::{WebSocket, WebSocketUpgrade},
        State,
    },
    response::Response,
    routing::get,
    Router,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use tokio::sync::{broadcast, mpsc};
use tokio_tungstenite::tungstenite::protocol::Message as WsMessage;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum WebSocketEvent {
    #[serde(rename = "new_block")]
    NewBlock {
        block: BlockNotification,
    },
    
    #[serde(rename = "new_transaction")]
    NewTransaction {
        transaction: TransactionNotification,
    },
    
    #[serde(rename = "mempool_update")]
    MempoolUpdate {
        size: usize,
        fee_rate: u64,
    },
    
    #[serde(rename = "difficulty_update")]
    DifficultyUpdate {
        height: u64,
        difficulty: u32,
        network_hashrate: f64,
    },
    
    #[serde(rename = "peer_update")]
    PeerUpdate {
        connected: usize,
        total: usize,
    },
    
    #[serde(rename = "error")]
    Error {
        message: String,
    },
    
    #[serde(rename = "subscription_confirmed")]
    SubscriptionConfirmed {
        subscription: String,
    },
    
    #[serde(rename = "heartbeat")]
    Heartbeat {
        timestamp: u64,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockNotification {
    pub hash: String,
    pub height: u64,
    pub timestamp: u64,
    pub difficulty: u32,
    pub size: usize,
    pub transaction_count: usize,
    pub miner: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionNotification {
    pub hash: String,
    pub size: usize,
    pub fee: u64,
    pub fee_rate: u64,
    pub input_count: usize,
    pub output_count: usize,
    pub value: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum WebSocketRequest {
    #[serde(rename = "subscribe")]
    Subscribe {
        events: Vec<String>,
    },
    
    #[serde(rename = "unsubscribe")]
    Unsubscribe {
        events: Vec<String>,
    },
    
    #[serde(rename = "get_status")]
    GetStatus,
    
    #[serde(rename = "ping")]
    Ping,
}

#[derive(Debug, Clone)]
pub struct WebSocketClient {
    pub id: String,
    pub sender: mpsc::UnboundedSender<WebSocketEvent>,
    pub subscriptions: HashMap<String, bool>,
    pub connected_at: u64,
    pub last_ping: u64,
}

#[derive(Debug, Clone)]
pub struct WebSocketState {
    pub blockchain: Arc<RwLock<Blockchain>>,
    pub event_sender: broadcast::Sender<WebSocketEvent>,
    pub clients: Arc<RwLock<HashMap<String, WebSocketClient>>>,
}

pub struct WebSocketServer {
    blockchain: Arc<RwLock<Blockchain>>,
    port: u16,
    event_sender: broadcast::Sender<WebSocketEvent>,
    clients: Arc<RwLock<HashMap<String, WebSocketClient>>>,
}

impl WebSocketServer {
    pub fn new(blockchain: Arc<RwLock<Blockchain>>, port: u16) -> Self {
        let (event_sender, _) = broadcast::channel(1000);
        let clients = Arc::new(RwLock::new(HashMap::new()));
        
        Self {
            blockchain,
            port,
            event_sender,
            clients,
        }
    }
    
    pub async fn start(self) -> Result<()> {
        log::info!("🔌 Starting QTC WebSocket server on port {}", self.port);
        
        let state = WebSocketState {
            blockchain: self.blockchain.clone(),
            event_sender: self.event_sender.clone(),
            clients: self.clients.clone(),
        };
        
        let app = Router::new()
            .route("/ws", get(websocket_handler))
            .route("/ws/health", get(websocket_health))
            .with_state(state.clone());
        
        // Start background tasks
        let heartbeat_task = self.start_heartbeat_task(state.clone());
        let cleanup_task = self.start_cleanup_task(state.clone());
        let blockchain_monitor_task = self.start_blockchain_monitor(state.clone());
        
        let addr = format!("0.0.0.0:{}", self.port);
        let listener = tokio::net::TcpListener::bind(&addr).await
            .map_err(|e| QtcError::Network(format!("Failed to bind to {}: {}", addr, e)))?;
        
        log::info!("✅ WebSocket server listening on ws://{}/ws", addr);
        
        // Run all tasks concurrently
        tokio::select! {
            result = axum::serve(listener, app) => {
                if let Err(e) = result {
                    log::error!("WebSocket server error: {}", e);
                }
            }
            _ = heartbeat_task => {
                log::info!("Heartbeat task completed");
            }
            _ = cleanup_task => {
                log::info!("Cleanup task completed");
            }
            _ = blockchain_monitor_task => {
                log::info!("Blockchain monitor task completed");
            }
        }
        
        Ok(())
    }
    
    async fn start_heartbeat_task(&self, state: WebSocketState) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(30));
            
            loop {
                interval.tick().await;
                
                let heartbeat = WebSocketEvent::Heartbeat {
                    timestamp: chrono::Utc::now().timestamp() as u64,
                };
                
                if let Err(e) = state.event_sender.send(heartbeat) {
                    log::debug!("Failed to send heartbeat: {}", e);
                }
            }
        })
    }
    
    async fn start_cleanup_task(&self, state: WebSocketState) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(60));
            
            loop {
                interval.tick().await;
                
                let now = chrono::Utc::now().timestamp() as u64;
                let timeout = 300; // 5 minutes
                
                let mut clients = state.clients.write().unwrap();
                let mut to_remove = Vec::new();
                
                for (id, client) in clients.iter() {
                    if now - client.last_ping > timeout {
                        to_remove.push(id.clone());
                    }
                }
                
                for id in to_remove {
                    clients.remove(&id);
                    log::debug!("Removed inactive WebSocket client: {}", id);
                }
            }
        })
    }
    
    async fn start_blockchain_monitor(&self, state: WebSocketState) -> tokio::task::JoinHandle<()> {
        let blockchain = self.blockchain.clone();
        
        tokio::spawn(async move {
            let mut last_height = 0u64;
            let mut last_difficulty = 0u32;
            let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(5));
            
            loop {
                interval.tick().await;
                
                if let Ok(blockchain) = blockchain.read() {
                    let current_height = blockchain.height;
                    
                    // Check for new blocks
                    if current_height > last_height {
                        if let Ok(Some(block)) = blockchain.get_block_by_height(current_height) {
                            let notification = WebSocketEvent::NewBlock {
                                block: BlockNotification {
                                    hash: block.hash().to_hex(),
                                    height: block.header.height,
                                    timestamp: block.header.timestamp,
                                    difficulty: block.header.difficulty,
                                    size: block.size(),
                                    transaction_count: block.transactions.len(),
                                    miner: block.get_coinbase_transaction()
                                        .and_then(|tx| tx.outputs.first())
                                        .map(|_| "Unknown".to_string()), // Would extract miner address
                                },
                            };
                            
                            if let Err(e) = state.event_sender.send(notification) {
                                log::debug!("Failed to send new block notification: {}", e);
                            }
                        }
                        last_height = current_height;
                    }
                    
                    // Check for difficulty changes
                    if let Ok(current_difficulty) = blockchain.get_current_difficulty() {
                        if current_difficulty != last_difficulty && last_difficulty != 0 {
                            let notification = WebSocketEvent::DifficultyUpdate {
                                height: current_height,
                                difficulty: current_difficulty,
                                network_hashrate: 0.0, // Would be calculated
                            };
                            
                            if let Err(e) = state.event_sender.send(notification) {
                                log::debug!("Failed to send difficulty update: {}", e);
                            }
                        }
                        last_difficulty = current_difficulty;
                    }
                }
            }
        })
    }
    
    pub fn broadcast_transaction(&self, tx: &Transaction) {
        let notification = WebSocketEvent::NewTransaction {
            transaction: TransactionNotification {
                hash: tx.hash().to_hex(),
                size: tx.size(),
                fee: tx.fee(),
                fee_rate: if tx.size() > 0 { tx.fee() / tx.size() as u64 } else { 0 },
                input_count: tx.inputs.len(),
                output_count: tx.outputs.len(),
                value: tx.total_output_value(),
            },
        };
        
        if let Err(e) = self.event_sender.send(notification) {
            log::debug!("Failed to broadcast transaction: {}", e);
        }
    }
    
    pub fn broadcast_mempool_update(&self, size: usize, fee_rate: u64) {
        let notification = WebSocketEvent::MempoolUpdate { size, fee_rate };
        
        if let Err(e) = self.event_sender.send(notification) {
            log::debug!("Failed to broadcast mempool update: {}", e);
        }
    }
}

async fn websocket_health() -> &'static str {
    "WebSocket server is healthy"
}

async fn websocket_handler(
    ws: WebSocketUpgrade,
    State(state): State<WebSocketState>,
) -> Response {
    ws.on_upgrade(|socket| handle_websocket(socket, state))
}

async fn handle_websocket(socket: WebSocket, state: WebSocketState) {
    let client_id = uuid::Uuid::new_v4().to_string();
    log::info!("New WebSocket client connected: {}", client_id);
    
    let (mut sender, mut receiver) = socket.split();
    let (tx, mut rx) = mpsc::unbounded_channel::<WebSocketEvent>();
    
    // Create client
    let client = WebSocketClient {
        id: client_id.clone(),
        sender: tx.clone(),
        subscriptions: HashMap::new(),
        connected_at: chrono::Utc::now().timestamp() as u64,
        last_ping: chrono::Utc::now().timestamp() as u64,
    };
    
    // Add client to the list
    {
        let mut clients = state.clients.write().unwrap();
        clients.insert(client_id.clone(), client);
    }
    
    // Subscribe to global events
    let mut event_receiver = state.event_sender.subscribe();
    
    // Send welcome message
    let welcome = WebSocketEvent::SubscriptionConfirmed {
        subscription: "connected".to_string(),
    };
    
    if let Err(_) = tx.send(welcome) {
        log::error!("Failed to send welcome message to client {}", client_id);
        return;
    }
    
    // Spawn task to handle outgoing messages
    let client_id_clone = client_id.clone();
    let outgoing_task = tokio::spawn(async move {
        loop {
            tokio::select! {
                // Handle messages from the client-specific channel
                Some(event) = rx.recv() => {
                    let message = match serde_json::to_string(&event) {
                        Ok(msg) => msg,
                        Err(e) => {
                            log::error!("Failed to serialize WebSocket event: {}", e);
                            continue;
                        }
                    };
                    
                    if sender.send(axum::extract::ws::Message::Text(message)).await.is_err() {
                        log::debug!("Client {} disconnected", client_id_clone);
                        break;
                    }
                }
                
                // Handle global broadcast events
                Ok(event) = event_receiver.recv() => {
                    // Check if client is subscribed to this event type
                    let should_send = match &event {
                        WebSocketEvent::NewBlock { .. } => true,
                        WebSocketEvent::NewTransaction { .. } => true,
                        WebSocketEvent::Heartbeat { .. } => true,
                        _ => true, // Send all events for now
                    };
                    
                    if should_send {
                        let message = match serde_json::to_string(&event) {
                            Ok(msg) => msg,
                            Err(e) => {
                                log::error!("Failed to serialize WebSocket event: {}", e);
                                continue;
                            }
                        };
                        
                        if sender.send(axum::extract::ws::Message::Text(message)).await.is_err() {
                            log::debug!("Client {} disconnected", client_id_clone);
                            break;
                        }
                    }
                }
            }
        }
    });
    
    // Handle incoming messages
    let client_id_clone = client_id.clone();
    let state_clone = state.clone();
    let incoming_task = tokio::spawn(async move {
        while let Some(msg) = receiver.next().await {
            match msg {
                Ok(axum::extract::ws::Message::Text(text)) => {
                    if let Err(e) = handle_client_message(&client_id_clone, &text, &state_clone).await {
                        log::error!("Error handling client message: {}", e);
                    }
                }
                
                Ok(axum::extract::ws::Message::Pong(_)) => {
                    // Update last ping time
                    if let Ok(mut clients) = state_clone.clients.write() {
                        if let Some(client) = clients.get_mut(&client_id_clone) {
                            client.last_ping = chrono::Utc::now().timestamp() as u64;
                        }
                    }
                }
                
                Ok(axum::extract::ws::Message::Close(_)) => {
                    log::info!("Client {} requested close", client_id_clone);
                    break;
                }
                
                Err(e) => {
                    log::error!("WebSocket error for client {}: {}", client_id_clone, e);
                    break;
                }
                
                _ => {} // Ignore other message types
            }
        }
    });
    
    // Wait for either task to complete
    tokio::select! {
        _ = outgoing_task => {
            log::debug!("Outgoing task completed for client {}", client_id);
        }
        _ = incoming_task => {
            log::debug!("Incoming task completed for client {}", client_id);
        }
    }
    
    // Remove client from the list
    {
        let mut clients = state.clients.write().unwrap();
        clients.remove(&client_id);
    }
    
    log::info!("WebSocket client disconnected: {}", client_id);
}

async fn handle_client_message(
    client_id: &str,
    message: &str,
    state: &WebSocketState,
) -> Result<()> {
    let request: WebSocketRequest = serde_json::from_str(message)
        .map_err(|e| QtcError::Network(format!("Invalid WebSocket message: {}", e)))?;
    
    match request {
        WebSocketRequest::Subscribe { events } => {
            // Update client subscriptions
            if let Ok(mut clients) = state.clients.write() {
                if let Some(client) = clients.get_mut(client_id) {
                    for event in &events {
                        client.subscriptions.insert(event.clone(), true);
                    }
                    
                    // Send confirmation
                    let confirmation = WebSocketEvent::SubscriptionConfirmed {
                        subscription: events.join(", "),
                    };
                    
                    if let Err(_) = client.sender.send(confirmation) {
                        log::error!("Failed to send subscription confirmation to client {}", client_id);
                    }
                }
            }
        }
        
        WebSocketRequest::Unsubscribe { events } => {
            // Remove client subscriptions
            if let Ok(mut clients) = state.clients.write() {
                if let Some(client) = clients.get_mut(client_id) {
                    for event in &events {
                        client.subscriptions.remove(event);
                    }
                }
            }
        }
        
        WebSocketRequest::GetStatus => {
            // Send current status
            if let Ok(clients) = state.clients.read() {
                if let Some(client) = clients.get(client_id) {
                    if let Ok(blockchain) = state.blockchain.read() {
                        let chain_info = blockchain.get_chain_info().unwrap_or_default();
                        
                        // Create a status event (using difficulty update format)
                        let status = WebSocketEvent::DifficultyUpdate {
                            height: chain_info.height,
                            difficulty: chain_info.difficulty,
                            network_hashrate: 0.0,
                        };
                        
                        if let Err(_) = client.sender.send(status) {
                            log::error!("Failed to send status to client {}", client_id);
                        }
                    }
                }
            }
        }
        
        WebSocketRequest::Ping => {
            // Update ping time and send pong via heartbeat
            if let Ok(mut clients) = state.clients.write() {
                if let Some(client) = clients.get_mut(client_id) {
                    client.last_ping = chrono::Utc::now().timestamp() as u64;
                    
                    let pong = WebSocketEvent::Heartbeat {
                        timestamp: client.last_ping,
                    };
                    
                    if let Err(_) = client.sender.send(pong) {
                        log::error!("Failed to send pong to client {}", client_id);
                    }
                }
            }
        }
    }
    
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::Database;
    use tempfile::TempDir;
    
    #[tokio::test]
    async fn test_websocket_server_creation() {
        let temp_dir = TempDir::new().unwrap();
        let db = Arc::new(Database::new(temp_dir.path().join("test.db")).unwrap());
        let blockchain = Arc::new(RwLock::new(Blockchain::new(db).unwrap()));
        
        let server = WebSocketServer::new(blockchain, 0);
        assert_eq!(server.port, 0);
    }
    
    #[test]
    fn test_websocket_event_serialization() {
        let event = WebSocketEvent::NewBlock {
            block: BlockNotification {
                hash: "test_hash".to_string(),
                height: 100,
                timestamp: 1234567890,
                difficulty: 8,
                size: 1024,
                transaction_count: 5,
                miner: Some("test_miner".to_string()),
            },
        };
        
        let serialized = serde_json::to_string(&event).unwrap();
        assert!(serialized.contains("new_block"));
        assert!(serialized.contains("test_hash"));
        
        let deserialized: WebSocketEvent = serde_json::from_str(&serialized).unwrap();
        match deserialized {
            WebSocketEvent::NewBlock { block } => {
                assert_eq!(block.hash, "test_hash");
                assert_eq!(block.height, 100);
            }
            _ => panic!("Wrong event type"),
        }
    }
}
