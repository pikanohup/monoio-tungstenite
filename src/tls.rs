use monoio::io::{AsyncReadRent, AsyncWriteRent};

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
    Rustls(std::sync::Arc<rustls::ClientConfig>),
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

    #[cfg(feature = "native-tls")]
    pub mod native_tls {
        use monoio::io::{AsyncReadRent, AsyncWriteRent};
        use monoio_native_tls::TlsConnector as MonoioTlsConnector;
        use native_tls::TlsConnector;

        use crate::{
            Error, Result,
            error::TlsError,
            stream::{MaybeTlsStream, Mode},
        };

        pub async fn wrap_stream<S>(
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
                    let connector = tls_connector
                        .map_or_else(TlsConnector::new, Ok)
                        .map_err(|e| TlsError::Native(Box::new(e.into())))?;
                    let connector = MonoioTlsConnector::from(connector);

                    match connector.connect(domain, socket).await {
                        Err(e) => Err(Error::Tls(e.into())),
                        Ok(s) => Ok(MaybeTlsStream::NativeTls(s)),
                    }
                }
            }
        }
    }

    #[cfg(feature = "rustls-tls")]
    pub mod rustls {
        use std::sync::Arc;

        use monoio::io::{AsyncReadRent, AsyncWriteRent};
        use monoio_rustls::TlsConnector as MonoioTlsConnector;
        use rustls::{ClientConfig, RootCertStore};
        use rustls_pki_types::ServerName;

        use crate::{
            Error, Result,
            error::TlsError,
            stream::{MaybeTlsStream, Mode},
        };

        pub async fn wrap_stream<S>(
            socket: S,
            domain: &str,
            mode: Mode,
            tls_connector: Option<Arc<ClientConfig>>,
        ) -> Result<MaybeTlsStream<S>>
        where
            S: AsyncReadRent + AsyncWriteRent,
        {
            match mode {
                Mode::Plain => Ok(MaybeTlsStream::Plain(socket)),
                Mode::Tls => {
                    let config = match tls_connector {
                        Some(config) => config,
                        None => {
                            #[allow(unused_mut)]
                            let mut root_store = RootCertStore::empty();
                            #[cfg(feature = "rustls-tls-native-roots")]
                            {
                                #[allow(unused)]
                                let rustls_native_certs::CertificateResult {
                                    certs, errors, ..
                                } = rustls_native_certs::load_native_certs();

                                // Not finding any native root CA certificates is not fatal if the
                                // "rustls-tls-webpki-roots" feature is enabled.
                                #[cfg(not(feature = "rustls-tls-webpki-roots"))]
                                if certs.is_empty() {
                                    return Err(std::io::Error::new(std::io::ErrorKind::NotFound, format!("no native root CA certificates found (errors: {errors:?})")).into());
                                }

                                let (_number_added, _number_ignored) =
                                    root_store.add_parsable_certificates(certs);
                            }

                            #[cfg(feature = "rustls-tls-webpki-roots")]
                            {
                                root_store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
                            }

                            Arc::new(
                                ClientConfig::builder()
                                    .with_root_certificates(root_store)
                                    .with_no_client_auth(),
                            )
                        }
                    };

                    let domain = ServerName::try_from(domain)
                        .map_err(|_| TlsError::InvalidDnsName)?
                        .to_owned();
                    let connector = MonoioTlsConnector::from(config);

                    match connector.connect(domain, socket).await {
                        Err(e) => Err(Error::Tls(e.into())),
                        Ok(s) => Ok(MaybeTlsStream::Rustls(s)),
                    }
                }
            }
        }
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
            Connector::Plain => encryption::plain::wrap_stream(stream, mode).await,
            #[cfg(feature = "native-tls")]
            Connector::NativeTls(conn) => {
                encryption::native_tls::wrap_stream(stream, &domain, mode, Some(conn)).await
            }
            #[cfg(feature = "rustls-tls")]
            Connector::Rustls(conn) => {
                encryption::rustls::wrap_stream(stream, &domain, mode, Some(conn)).await
            }
        },
        None => {
            #[cfg(feature = "native-tls")]
            {
                encryption::native_tls::wrap_stream(stream, &domain, mode, None).await
            }
            #[cfg(all(feature = "rustls-tls", not(feature = "native-tls")))]
            {
                encryption::rustls::wrap_stream(stream, &domain, mode, None).await
            }
            #[cfg(not(any(feature = "native-tls", feature = "rustls-tls")))]
            {
                encryption::plain::wrap_stream(stream, mode).await
            }
        }
    }?;

    client_with_config(request, stream, config).await
}
