//! TLS support using native-tls.

use monoio::io::{AsyncReadRent, AsyncWriteRent};
// re-export `TlsConnector` for users to create their own TLS connectors.
pub use monoio_native_tls::TlsConnector;

use crate::{
    Error, Result,
    error::TlsError,
    stream::{MaybeTlsStream, Mode},
};

pub(super) async fn wrap_stream<S>(
    socket: S,
    domain: &str,
    mode: Mode,
    tls_connector: Option<TlsConnector>,
) -> Result<MaybeTlsStream<S>>
where
    S: AsyncReadRent + AsyncWriteRent,
{
    match mode {
        Mode::Plain => Ok(MaybeTlsStream::Plain(socket)),
        Mode::Tls => {
            let connector = match tls_connector {
                Some(connector) => connector,
                None => default_connector()?,
            };

            match connector.connect(domain, socket).await {
                Err(e) => Err(Error::Tls(e.into())),
                Ok(s) => Ok(MaybeTlsStream::NativeTls(s)),
            }
        }
    }
}

/// Creates a default native-tls `TlsConnector`.
pub fn default_connector() -> Result<TlsConnector> {
    let connector =
        native_tls::TlsConnector::new().map_err(|e| TlsError::Native(Box::new(e.into())))?;
    Ok(TlsConnector::from(connector))
}
