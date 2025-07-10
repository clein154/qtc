use crate::core::{Block, Transaction, Blockchain};
use crate::network::protocol::{Message, MessageType, ProtocolHandler};
use crate::{QtcError, Result};
use libp2p::{
    futures::StreamExt,
    gossipsub, identify, kad, mdns, noise, ping, swarm::NetworkBehaviour, tcp, yamux, PeerId,
    Swarm, SwarmBuilder,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};
use tokio::sync::{broadcast, mpsc};

#[derive(NetworkBehaviour)]
#[behaviour(out_event = "P2PEvent")]
pub struct QtcBehaviour {
    pub gossipsub: gossipsub::Behaviour,
    pub mdns: mdns::tokio::Behaviour,
    pub kademlia: kad::Behaviour<kad::store::MemoryStore>,
    pub identify: identify::Behaviour,
    pub ping: ping::Behaviour,
}

#[derive(Debug)]
pub enum P2PEvent {
    Gossipsub(gossipsub::Event),
    Mdns(mdns::Event),
    Kademlia(kad::Event),
    Identify(identify::Event),
    Ping(ping::Event),
}

impl From<gossipsub::Event> for P2PEvent {
    fn from(event: gossipsub::Event) -> Self {
        P2PEvent::Gossipsub(event)
    }
}

impl From<mdns::Event> for P2PEvent {
    fn from(event: mdns::Event) -> Self {
        P2PEvent::Mdns(event)
    }
}

impl From<kad::Event> for P2PEvent {
    fn from(event: kad::Event) -> Self {
        P2PEvent::Kademlia(event)
    }
}

impl From<identify::Event> for P2PEvent {
    fn from(event: identify::Event) -> Self {
        P2PEvent::Identify(event)
    }
}

impl From<ping::Event> for P2PEvent {
    fn from(event: ping::Event) -> Self {
        P2PEvent::Ping(event)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerInfo {
    pub peer_id: String,
    pub address: String,
    pub connected_at: u64,
    pub last_seen: u64,
    pub version: String,
    pub height: u64,
    pub ping_ms: Option<u64>,
    pub is_outbound: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkStats {
    pub peer_count: usize,
    pub connected_peers: Vec<PeerInfo>,
    pub blocks_received: u64,
    pub blocks_sent: u64,
    pub transactions_received: u64,
    pub transactions_sent: u64,
    pub bytes_received: u64,
    pub bytes_sent: u64,
    pub uptime_seconds: u64,
}

pub struct P2PNode {
    swarm: Swarm<QtcBehaviour>,
    blockchain: Arc<RwLock<Blockchain>>,
    protocol_handler: ProtocolHandler,
    peers: HashMap<PeerId, PeerInfo>,
    stats: NetworkStats,
    start_time: Instant,
    event_sender: broadcast::Sender<Message>,
    command_receiver: mpsc::Receiver<P2PCommand>,
}

#[derive(Debug)]
pub enum P2PCommand {
    BroadcastBlock(Block),
    BroadcastTransaction(Transaction),
    RequestBlocks(u64, u64), // start_height, end_height
    ConnectPeer(String),
    DisconnectPeer(PeerId),
    GetPeers,
}

impl P2PNode {
    pub async fn new(
        blockchain: Arc<RwLock<Blockchain>>,
        port: u16,
        bootstrap_nodes: Vec<String>,
    ) -> Result<(Self, broadcast::Receiver<Message>, mpsc::Sender<P2PCommand>)> {
        // Generate a random peer ID
        let local_key = libp2p::identity::Keypair::generate_ed25519();
        let local_peer_id = PeerId::from(local_key.public());
        
        log::info!("🌐 Starting P2P node with peer ID: {}", local_peer_id);
        
        // Create transport
        let transport = tcp::tokio::Transport::new(tcp::Config::default().nodelay(true))
            .upgrade(libp2p::core::upgrade::Version::V1)
            .authenticate(noise::Config::new(&local_key)?)
            .multiplex(yamux::Config::default())
            .boxed();
        
        // Configure Gossipsub
        let gossipsub_config = gossipsub::ConfigBuilder::default()
            .heartbeat_interval(Duration::from_secs(10))
            .validation_mode(gossipsub::ValidationMode::Strict)
            .build()
            .map_err(|e| QtcError::Network(format!("Gossipsub config error: {}", e)))?;
        
        let mut gossipsub = gossipsub::Behaviour::new(
            gossipsub::MessageAuthenticity::Signed(local_key.clone()),
            gossipsub_config,
        ).map_err(|e| QtcError::Network(format!("Gossipsub creation error: {}", e)))?;
        
        // Subscribe to topics
        let block_topic = gossipsub::IdentTopic::new("qtc/blocks");
        let tx_topic = gossipsub::IdentTopic::new("qtc/transactions");
        
        gossipsub.subscribe(&block_topic)
            .map_err(|e| QtcError::Network(format!("Block topic subscription error: {}", e)))?;
        gossipsub.subscribe(&tx_topic)
            .map_err(|e| QtcError::Network(format!("Transaction topic subscription error: {}", e)))?;
        
        // Configure mDNS for local peer discovery
        let mdns = mdns::tokio::Behaviour::new(mdns::Config::default(), local_peer_id)
            .map_err(|e| QtcError::Network(format!("mDNS creation error: {}", e)))?;
        
        // Configure Kademlia DHT
        let store = kad::store::MemoryStore::new(local_peer_id);
        let mut kademlia = kad::Behaviour::new(local_peer_id, store);
        
        // Add bootstrap nodes to Kademlia
        for node in &bootstrap_nodes {
            if let Ok(addr) = node.parse() {
                kademlia.add_address(&local_peer_id, addr);
            }
        }
        
        // Configure Identify
        let identify = identify::Behaviour::new(identify::Config::new(
            "/qtc/1.0.0".into(),
            local_key.public(),
        ));
        
        // Configure Ping
        let ping = ping::Behaviour::new(ping::Config::new());
        
        // Create behaviour
        let behaviour = QtcBehaviour {
            gossipsub,
            mdns,
            kademlia,
            identify,
            ping,
        };
        
        // Create swarm with simplified configuration for compatibility
        let mut swarm = SwarmBuilder::with_existing_identity(local_key)
            .with_tokio_executor()
            .with_tcp(tcp::Config::default(), noise::Config::new, yamux::Config::default).unwrap()
            .with_behaviour(|_| behaviour).unwrap()
            .build();
        
        // Listen on the specified port
        swarm.listen_on(format!("/ip4/0.0.0.0/tcp/{}", port).parse()?)
            .map_err(|e| QtcError::Network(format!("Failed to listen: {}", e)))?;
        
        // Create communication channels
        let (event_sender, event_receiver) = broadcast::channel(1000);
        let (command_sender, command_receiver) = mpsc::channel(100);
        
        let protocol_handler = ProtocolHandler::new(blockchain.clone());
        
        let node = Self {
            swarm,
            blockchain,
            protocol_handler,
            peers: HashMap::new(),
            stats: NetworkStats {
                peer_count: 0,
                connected_peers: Vec::new(),
                blocks_received: 0,
                blocks_sent: 0,
                transactions_received: 0,
                transactions_sent: 0,
                bytes_received: 0,
                bytes_sent: 0,
                uptime_seconds: 0,
            },
            start_time: Instant::now(),
            event_sender,
            command_receiver,
        };
        
        Ok((node, event_receiver, command_sender))
    }
    
    pub async fn run(&mut self) -> Result<()> {
        log::info!("🚀 P2P node started and listening for connections");
        
        loop {
            tokio::select! {
                event = self.swarm.next() => {
                    if let Some(event) = event {
                        self.handle_swarm_event(event).await?;
                    }
                }
                command = self.command_receiver.recv() => {
                    if let Some(cmd) = command {
                        self.handle_command(cmd).await?;
                    }
                }
                _ = tokio::time::sleep(Duration::from_secs(30)) => {
                    self.update_stats();
                    self.maintenance_tasks().await?;
                }
            }
        }
    }
    
    async fn handle_swarm_event(&mut self, event: libp2p::swarm::SwarmEvent<P2PEvent>) -> Result<()> {
        match event {
            libp2p::swarm::SwarmEvent::Behaviour(P2PEvent::Gossipsub(gossipsub::Event::Message {
                propagation_source: _,
                message_id: _,
                message,
            })) => {
                self.handle_gossip_message(message).await?;
            }
            
            libp2p::swarm::SwarmEvent::Behaviour(P2PEvent::Mdns(mdns::Event::Discovered(list))) => {
                for (peer_id, multiaddr) in list {
                    log::info!("🔍 Discovered peer via mDNS: {} at {}", peer_id, multiaddr);
                    self.swarm.behaviour_mut().gossipsub.add_explicit_peer(&peer_id);
                    self.swarm.dial(multiaddr)?;
                }
            }
            
            libp2p::swarm::SwarmEvent::Behaviour(P2PEvent::Identify(identify::Event::Received {
                peer_id,
                info,
            })) => {
                log::info!("🆔 Identified peer: {} running {}", peer_id, info.agent_version);
                
                // Add peer to Kademlia
                for addr in info.listen_addrs {
                    self.swarm.behaviour_mut().kademlia.add_address(&peer_id, addr);
                }
            }
            
            libp2p::swarm::SwarmEvent::Behaviour(P2PEvent::Ping(ping::Event { peer, result })) => {
                match result {
                    Ok(duration) => {
                        if let Some(peer_info) = self.peers.get_mut(&peer) {
                            peer_info.ping_ms = Some(duration.as_millis() as u64);
                            peer_info.last_seen = chrono::Utc::now().timestamp() as u64;
                        }
                    }
                    Err(e) => {
                        log::warn!("❌ Ping failed for peer {}: {}", peer, e);
                    }
                }
            }
            
            libp2p::swarm::SwarmEvent::ConnectionEstablished { peer_id, .. } => {
                log::info!("🤝 Connected to peer: {}", peer_id);
                
                let peer_info = PeerInfo {
                    peer_id: peer_id.to_string(),
                    address: "unknown".to_string(),
                    connected_at: chrono::Utc::now().timestamp() as u64,
                    last_seen: chrono::Utc::now().timestamp() as u64,
                    version: "unknown".to_string(),
                    height: 0,
                    ping_ms: None,
                    is_outbound: true,
                };
                
                self.peers.insert(peer_id, peer_info);
                self.stats.peer_count = self.peers.len();
                
                // Request blockchain sync
                self.request_blockchain_sync(peer_id).await?;
            }
            
            libp2p::swarm::SwarmEvent::ConnectionClosed { peer_id, .. } => {
                log::info!("👋 Disconnected from peer: {}", peer_id);
                self.peers.remove(&peer_id);
                self.stats.peer_count = self.peers.len();
            }
            
            _ => {}
        }
        
        Ok(())
    }
    
    async fn handle_gossip_message(&mut self, message: gossipsub::Message) -> Result<()> {
        let topic = message.topic.as_str();
        
        match topic {
            "qtc/blocks" => {
                self.stats.blocks_received += 1;
                self.stats.bytes_received += message.data.len() as u64;
                
                // Deserialize and process block
                if let Ok(block) = bincode::deserialize::<Block>(&message.data) {
                    log::info!("📦 Received block: height {}", block.header.height);
                    
                    let msg = Message::new(MessageType::Block(block));
                    let _ = self.event_sender.send(msg);
                } else {
                    log::warn!("⚠️ Failed to deserialize block");
                }
            }
            
            "qtc/transactions" => {
                self.stats.transactions_received += 1;
                self.stats.bytes_received += message.data.len() as u64;
                
                // Deserialize and process transaction
                if let Ok(tx) = bincode::deserialize::<Transaction>(&message.data) {
                    log::debug!("💰 Received transaction: {}", hex::encode(tx.hash().as_bytes()));
                    
                    let msg = Message::new(MessageType::Transaction(tx));
                    let _ = self.event_sender.send(msg);
                } else {
                    log::warn!("⚠️ Failed to deserialize transaction");
                }
            }
            
            _ => {
                log::debug!("📨 Received message on unknown topic: {}", topic);
            }
        }
        
        Ok(())
    }
    
    async fn handle_command(&mut self, command: P2PCommand) -> Result<()> {
        match command {
            P2PCommand::BroadcastBlock(block) => {
                self.broadcast_block(block).await?;
            }
            
            P2PCommand::BroadcastTransaction(tx) => {
                self.broadcast_transaction(tx).await?;
            }
            
            P2PCommand::RequestBlocks(start, end) => {
                self.request_blocks(start, end).await?;
            }
            
            P2PCommand::ConnectPeer(address) => {
                self.connect_peer(address).await?;
            }
            
            P2PCommand::DisconnectPeer(peer_id) => {
                self.disconnect_peer(peer_id).await?;
            }
            
            P2PCommand::GetPeers => {
                // This would typically send response back through a channel
                // For now, just log the peer count
                log::info!("📊 Currently connected to {} peers", self.peers.len());
            }
        }
        
        Ok(())
    }
    
    async fn broadcast_block(&mut self, block: Block) -> Result<()> {
        log::info!("📡 Broadcasting block: height {}", block.header.height);
        
        let data = bincode::serialize(&block)
            .map_err(|e| QtcError::Network(format!("Failed to serialize block: {}", e)))?;
        
        let topic = gossipsub::IdentTopic::new("qtc/blocks");
        
        self.swarm.behaviour_mut().gossipsub.publish(topic, data)
            .map_err(|e| QtcError::Network(format!("Failed to publish block: {}", e)))?;
        
        self.stats.blocks_sent += 1;
        self.stats.bytes_sent += block.size() as u64;
        
        Ok(())
    }
    
    async fn broadcast_transaction(&mut self, tx: Transaction) -> Result<()> {
        log::debug!("📡 Broadcasting transaction: {}", hex::encode(tx.hash().as_bytes()));
        
        let data = bincode::serialize(&tx)
            .map_err(|e| QtcError::Network(format!("Failed to serialize transaction: {}", e)))?;
        
        let topic = gossipsub::IdentTopic::new("qtc/transactions");
        
        self.swarm.behaviour_mut().gossipsub.publish(topic, data)
            .map_err(|e| QtcError::Network(format!("Failed to publish transaction: {}", e)))?;
        
        self.stats.transactions_sent += 1;
        self.stats.bytes_sent += tx.size() as u64;
        
        Ok(())
    }
    
    async fn request_blocks(&mut self, start_height: u64, end_height: u64) -> Result<()> {
        log::info!("📥 Requesting blocks {} to {}", start_height, end_height);
        
        // In a full implementation, this would send a specific request message
        // For now, we'll implement a simplified version
        
        for peer_id in self.peers.keys().cloned().collect::<Vec<_>>() {
            // Send block request to peer
            // This would use a custom protocol in production
            log::debug!("Requesting blocks from peer: {}", peer_id);
        }
        
        Ok(())
    }
    
    async fn connect_peer(&mut self, address: String) -> Result<()> {
        log::info!("🔗 Connecting to peer: {}", address);
        
        let multiaddr = address.parse()
            .map_err(|e| QtcError::Network(format!("Invalid address: {}", e)))?;
        
        self.swarm.dial(multiaddr)
            .map_err(|e| QtcError::Network(format!("Failed to dial peer: {}", e)))?;
        
        Ok(())
    }
    
    async fn disconnect_peer(&mut self, peer_id: PeerId) -> Result<()> {
        log::info!("✂️ Disconnecting from peer: {}", peer_id);
        
        // Disconnect from the peer
        self.swarm.disconnect_peer_id(peer_id)
            .map_err(|e| QtcError::Network(format!("Failed to disconnect: {}", e)))?;
        
        Ok(())
    }
    
    async fn request_blockchain_sync(&mut self, peer_id: PeerId) -> Result<()> {
        log::info!("🔄 Requesting blockchain sync from peer: {}", peer_id);
        
        // Get our current height
        let our_height = {
            let blockchain = self.blockchain.read().unwrap();
            blockchain.height
        };
        
        // In a full implementation, this would send a sync request message
        // For now, just log the sync request
        log::debug!("Our height: {}, requesting sync from peer", our_height);
        
        Ok(())
    }
    
    fn update_stats(&mut self) {
        self.stats.uptime_seconds = self.start_time.elapsed().as_secs();
        self.stats.connected_peers = self.peers.values().cloned().collect();
        self.stats.peer_count = self.peers.len();
    }
    
    async fn maintenance_tasks(&mut self) -> Result<()> {
        // Periodic maintenance tasks
        
        // Remove stale peers
        let now = chrono::Utc::now().timestamp() as u64;
        let stale_timeout = 300; // 5 minutes
        
        let stale_peers: Vec<PeerId> = self.peers
            .iter()
            .filter(|(_, info)| now - info.last_seen > stale_timeout)
            .map(|(peer_id, _)| *peer_id)
            .collect();
        
        for peer_id in stale_peers {
            log::warn!("🗑️ Removing stale peer: {}", peer_id);
            self.peers.remove(&peer_id);
        }
        
        // Bootstrap if we have too few peers
        if self.peers.len() < 3 {
            log::info!("🔄 Bootstrapping - too few peers connected");
            self.swarm.behaviour_mut().kademlia.bootstrap()
                .map_err(|e| QtcError::Network(format!("Bootstrap failed: {}", e)))?;
        }
        
        Ok(())
    }
    
    pub fn get_stats(&self) -> NetworkStats {
        self.stats.clone()
    }
    
    pub fn get_peer_count(&self) -> usize {
        self.peers.len()
    }
    
    pub fn get_peers(&self) -> Vec<PeerInfo> {
        self.peers.values().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::Database;
    use tempfile::TempDir;
    
    #[tokio::test]
    async fn test_p2p_node_creation() -> Result<()> {
        let temp_dir = TempDir::new().unwrap();
        let db = Arc::new(Database::new(temp_dir.path().join("test.db"))?);
        let blockchain = Arc::new(RwLock::new(Blockchain::new(db)?));
        
        let (mut node, _receiver, _sender) = P2PNode::new(
            blockchain,
            0, // Random port
            vec![],
        ).await?;
        
        assert_eq!(node.get_peer_count(), 0);
        
        Ok(())
    }
}
