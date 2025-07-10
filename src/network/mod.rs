//! Networking module for P2P communication

pub mod p2p;
pub mod protocol;

pub use p2p::{P2PNode, PeerInfo, NetworkStats};
pub use protocol::{Message, MessageType, ProtocolHandler};
