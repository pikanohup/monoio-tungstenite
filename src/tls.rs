//! TLS support for WebSocket connections.

use monoio::io::{AsyncReadRent, AsyncWriteRent};

#[cfg(feature = "native-tls")]
pub mod native_tls;
mod plain;
#[cfg(feature = "rustls-tls")]
pub mod rustls;

use crate::{
    client::{IntoClientRequest, client_with_config, uri_mode},
    error::{Error, Result, UrlError},
    handshake::client::Response,
    protocol::{WebSocket, WebSocketConfig},
    stream::MaybeTlsStream,
};

/// A connector that can be used when establishing connections, allowing to control whether
/// `native-tls` or `rustls` is used to create a TLS connection. Or TLS can be disabled with the
/// `Plain` variant.
#[non_exhaustive]
#[derive(Clone)]
pub enum Connector {
    /// Plain (non-TLS) connector.
    Plain,
    /// `native-tls` TLS connector.
    #[cfg(feature = "native-tls")]
    NativeTls(native_tls::TlsConnector),
    /// `rustls` TLS connector.
    #[cfg(feature = "rustls-tls")]
    Rustls(rustls::TlsConnector),
}

#[cfg(feature = "native-tls")]
impl From<native_tls::TlsConnector> for Connector {
    fn from(connector: native_tls::TlsConnector) -> Self {
        Connector::NativeTls(connector)
    }
}

#[cfg(feature = "rustls-tls")]
impl From<rustls::TlsConnector> for Connector {
    fn from(connector: rustls::TlsConnector) -> Self {
        Connector::Rustls(connector)
    }
}

/// Creates a WebSocket handshake from a request and a stream,
/// upgrading the stream to TLS if required.
#[cfg(any(feature = "native-tls", feature = "rustls-tls"))]
pub async fn client_tls<R, S>(
    request: R,
    stream: S,
) -> Result<(WebSocket<MaybeTlsStream<S>>, Response)>
where
    R: IntoClientRequest,
    S: AsyncReadRent + AsyncWriteRent,
{
    client_tls_with_config(request, stream, None, None).await
}

/// The same as [`client_tls()`] but one can specify a websocket configuration,
/// and an optional connector. If no connector is specified, a default one will
/// be created.
pub async fn client_tls_with_config<R, S>(
    request: R,
    stream: S,
    config: Option<WebSocketConfig>,
    connector: Option<Connector>,
) -> Result<(WebSocket<MaybeTlsStream<S>>, Response)>
where
    R: IntoClientRequest,
    S: AsyncReadRent + AsyncWriteRent,
{
    let request = request.into_client_request()?;

    #[cfg(any(feature = "native-tls", feature = "rustls-tls"))]
    let domain = match request.uri().host() {
        Some(d) => Ok(d.to_string()),
        None => Err(Error::Url(UrlError::NoHostName)),
    }?;

    let mode = uri_mode(request.uri())?;

    let stream = match connector {
        Some(conn) => match conn {
            Connector::Plain => plain::wrap_stream(stream, mode).await,
            #[cfg(feature = "native-tls")]
            Connector::NativeTls(conn) => {
                native_tls::wrap_stream(stream, &domain, mode, Some(conn)).await
            }
            #[cfg(feature = "rustls-tls")]
            Connector::Rustls(conn) => rustls::wrap_stream(stream, &domain, mode, Some(conn)).await,
        },
        None => {
            #[cfg(feature = "native-tls")]
            {
                native_tls::wrap_stream(stream, &domain, mode, None).await
            }
            #[cfg(all(feature = "rustls-tls", not(feature = "native-tls")))]
            {
                rustls::wrap_stream(stream, &domain, mode, None).await
            }
            #[cfg(not(any(feature = "native-tls", feature = "rustls-tls")))]
            {
                plain::wrap_stream(stream, mode).await
            }
        }
    }?;

    client_with_config(request, stream, config).await
}
