//! API module for REST and WebSocket endpoints

pub mod rest;
pub mod websocket;

pub use rest::RestApi;
pub use websocket::WebSocketServer;
