//! Lightweight, asynchronous WebSocket implementation for [`monoio`](https://github.com/bytedance/monoio) runtime,
//! adapted from [`tungstenite-rs`](https://github.com/snapview/tungstenite-rs).

#![deny(
    missing_docs,
    unused_must_use,
    unused_mut,
    unused_imports,
    unused_import_braces
)]

pub mod error;
pub use error::{Error, Result};

pub mod protocol;
pub mod stream;

#[cfg(feature = "handshake")]
pub mod client;
#[cfg(feature = "handshake")]
pub mod handshake;
#[cfg(feature = "handshake")]
pub mod server;

// mod tls;
mod framed;

// re-export bytes since used in `Message` API.
pub use bytes::Bytes;
#[cfg(feature = "handshake")]
pub use http;

pub use crate::protocol::{Message, WebSocket, frame::Utf8Bytes};
#[cfg(feature = "handshake")]
pub use crate::{
    client::{ClientRequestBuilder, client, connect},
    handshake::client::client_handshake,
    handshake::server::server_handshake,
    server::{accept, accept_hdr, accept_hdr_with_config, accept_with_config},
};
