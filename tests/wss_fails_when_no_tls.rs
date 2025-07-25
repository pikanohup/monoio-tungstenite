#![cfg(all(
    feature = "handshake",
    not(any(feature = "native-tls", feature = "rustls-tls"))
))]

use monoio_tungstenite::{Error, connect, error::UrlError};

#[monoio::test]
async fn wss_url_fails_when_no_tls_support() {
    let ws = connect("wss://127.0.0.1/ws").await;
    eprintln!("{ws:?}");
    assert!(matches!(
        ws,
        Err(Error::Url(UrlError::TlsFeatureNotEnabled))
    ));
}
