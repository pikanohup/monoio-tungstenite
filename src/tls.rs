use monoio::io::{AsyncReadRent, AsyncWriteRent};

use crate::{
    client::{IntoClientRequest, client_with_config, uri_mode},
    error::Result,
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
    // TODO tls connector
}

mod encryption {
    pub mod plain {
        use monoio::io::{AsyncReadRent, AsyncWriteRent};

        use crate::{
            error::{Error, UrlError},
            stream::{MaybeTlsStream, Mode},
        };

        pub async fn wrap_stream<S>(socket: S, mode: Mode) -> Result<MaybeTlsStream<S>, Error>
        where
            S: AsyncReadRent + AsyncWriteRent,
        {
            match mode {
                Mode::Plain => Ok(MaybeTlsStream::Plain(socket)),
                Mode::Tls => Err(Error::Url(UrlError::TlsFeatureNotEnabled)),
            }
        }
    }
}

/// Creates a WebSocket handshake from a request and a stream,
/// upgrading the stream to TLS if required.
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
    let mode = uri_mode(request.uri())?;

    let stream = match connector {
        Some(conn) => match conn {
            Connector::Plain => self::encryption::plain::wrap_stream(stream, mode),
        },
        None => self::encryption::plain::wrap_stream(stream, mode),
    }
    .await?;

    client_with_config(request, stream, config).await
}
