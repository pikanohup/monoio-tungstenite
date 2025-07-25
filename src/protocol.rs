//! WebSocket protocol implementation.

pub mod frame;

mod message;

pub use frame::CloseFrame;
pub use message::Message;

mod websocket;
pub use websocket::{Role, WebSocket, WebSocketConfig};
