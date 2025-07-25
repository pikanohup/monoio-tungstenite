#![cfg(all(feature = "handshake", feature = "url"))]

use monoio::net::TcpListener;
use monoio_tungstenite::{
    accept_hdr, connect,
    handshake::server::{Request, Response},
};

/// Test for write buffering and flushing behaviour.
#[monoio::test]
async fn test_with_url() {
    let server = TcpListener::bind("localhost:3013").unwrap();

    // notice the use of url::Url instead of a string
    // notice the feature url is activated
    let url = url::Url::parse("ws://localhost:3013").unwrap();
    let client_conn = monoio::spawn(async {
        let conn = connect(url).await;
        assert!(conn.is_ok());
    });

    let (client_handler, _) = server.accept().await.unwrap();
    let mut client_handler = accept_hdr(client_handler, |_: &Request, r: Response| Ok(r))
        .await
        .unwrap();

    let closing = client_handler.close(None).await;
    assert!(closing.is_ok());

    client_conn.await;
}
