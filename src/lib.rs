//! Lightweight, asynchronous WebSocket implementation for [`monoio`](https://github.com/bytedance/monoio) runtime,
//! adapted from [`tungstenite-rs`](https://github.com/snapview/tungstenite-rs).

#![deny(
    missing_docs,
    unused_must_use,
    unused_mut,
    unused_imports,
    unused_import_braces
)]

#[cfg(feature = "handshake")]
pub mod client;
pub mod error;
mod framed;
#[cfg(feature = "handshake")]
pub mod handshake;
pub mod protocol;
#[cfg(feature = "handshake")]
pub mod server;
pub mod stream;
#[cfg(all(
    any(feature = "native-tls", feature = "rustls-tls"),
    feature = "handshake"
))]
mod tls;

// re-export bytes since used in `Message` API.
pub use bytes::Bytes;
pub use error::{Error, Result};
#[cfg(feature = "handshake")]
pub use http;
pub use protocol::{Message, WebSocket, frame::Utf8Bytes};
pub use stream::MaybeTlsStream;
#[cfg(all(
    any(feature = "native-tls", feature = "rustls-tls"),
    feature = "handshake"
))]
pub use tls::{Connector, client_tls, client_tls_with_config};

#[cfg(feature = "handshake")]
pub use crate::{
    client::{ClientRequestBuilder, client, connect},
    handshake::client::client_handshake,
    handshake::server::server_handshake,
    server::{accept, accept_hdr, accept_hdr_with_config, accept_with_config},
};
