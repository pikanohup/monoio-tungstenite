//! TLS support using rustls.

use monoio::io::{AsyncReadRent, AsyncWriteRent};
// re-export `TlsConnector` for users to create their own TLS connectors.
pub use monoio_rustls::TlsConnector;
use rustls::{ClientConfig, RootCertStore};
use rustls_pki_types::ServerName;

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

            let domain = ServerName::try_from(domain)
                .map_err(|_| TlsError::InvalidDnsName)?
                .to_owned();

            match connector.connect(domain, socket).await {
                Err(e) => Err(Error::Tls(e.into())),
                Ok(s) => Ok(MaybeTlsStream::Rustls(s)),
            }
        }
    }
}

/// Creates a default rustls `TlsConnector` using system or webpki root certificates,
/// depending on enabled features.
pub fn default_connector() -> Result<TlsConnector> {
    #[allow(unused_mut)]
    let mut root_store = RootCertStore::empty();
    #[cfg(feature = "rustls-tls-native-roots")]
    {
        #[allow(unused)]
        let rustls_native_certs::CertificateResult { certs, errors, .. } =
            rustls_native_certs::load_native_certs();

        // Not finding any native root CA certificates is not fatal if the
        // "rustls-tls-webpki-roots" feature is enabled.
        #[cfg(not(feature = "rustls-tls-webpki-roots"))]
        if certs.is_empty() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("no native root CA certificates found (errors: {errors:?})"),
            )
            .into());
        }

        let (_number_added, _number_ignored) = root_store.add_parsable_certificates(certs);
    }

    #[cfg(feature = "rustls-tls-webpki-roots")]
    {
        root_store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    }

    let connector = TlsConnector::from(
        ClientConfig::builder()
            .with_root_certificates(root_store)
            .with_no_client_auth(),
    );

    Ok(connector)
}
