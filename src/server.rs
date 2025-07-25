//! Methods to accept an incoming WebSocket connection on a server.

use monoio::io::{AsyncReadRent, AsyncWriteRent};

use crate::{
    error::Result,
    handshake::server::{Callback, NoCallback, server_handshake},
    protocol::{WebSocket, WebSocketConfig},
};

/// Accepts a new WebSocket connection with the provided stream.
///
/// This is typically used after a socket has been accepted from a
/// `TcpListener`. That socket is then passed to this function to perform
/// the server half of the accepting a client's websocket connection.
pub async fn accept<S>(stream: S) -> Result<WebSocket<S>>
where
    S: AsyncReadRent + AsyncWriteRent,
{
    accept_with_config(stream, None).await
}

/// The same as [`accept`] but the one can specify a websocket configuration.
pub async fn accept_with_config<S>(
    stream: S,
    config: Option<WebSocketConfig>,
) -> Result<WebSocket<S>>
where
    S: AsyncReadRent + AsyncWriteRent,
{
    accept_hdr_with_config(stream, NoCallback, config).await
}

/// Accepts the given Stream as a WebSocket.
///
/// This function does the same as [`accept`] but accepts an extra callback
/// for header processing. The callback receives headers of the incoming
/// requests and is able to add extra headers to the reply.
pub async fn accept_hdr<S, C>(stream: S, callback: C) -> Result<WebSocket<S>>
where
    S: AsyncReadRent + AsyncWriteRent,
    C: Callback,
{
    accept_hdr_with_config(stream, callback, None).await
}

/// The same as [`accept_hdr`] but the one can specify a websocket configuration.
pub async fn accept_hdr_with_config<S, C>(
    stream: S,
    callback: C,
    config: Option<WebSocketConfig>,
) -> Result<WebSocket<S>>
where
    S: AsyncReadRent + AsyncWriteRent,
    C: Callback,
{
    server_handshake(callback, stream, config).await
}
