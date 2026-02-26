//! WebSocket protocol implementation.

pub mod frame;

mod message;

pub use frame::CloseFrame;
pub use message::Message;

pub(crate) mod split;
pub use split::{WebSocketReadHalf, WebSocketWriteHalf};

mod websocket;
pub use websocket::{FramedRead, Role, WebSocket, WebSocketConfig};
