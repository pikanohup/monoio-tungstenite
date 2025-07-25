//! Verifies that we can read data messages even if we have initiated a close handshake,
//! but before we got confirmation.

#![cfg(feature = "handshake")]

use monoio::{io::sink::SinkExt, net::TcpListener};
use monoio_tungstenite::{Error, Message, accept, connect};

#[monoio::test]
async fn test_receive_after_init_close() {
    let server = TcpListener::bind("localhost:3013").unwrap();

    let client_conn = monoio::spawn(async move {
        let (mut client, _) = connect("ws://localhost:3013/socket").await.unwrap();

        client
            .send_and_flush(Message::Text("Hello WebSocket".into()))
            .await
            .unwrap();

        let message = client.read().await.unwrap(); // receive close from server
        assert!(message.is_close());

        let err = client.read().await.unwrap_err(); // now we should get ConnectionClosed
        match err {
            Error::ConnectionClosed => {}
            _ => panic!("unexpected error: {err:?}"),
        }
    });

    let (client_handler, _) = server.accept().await.unwrap();
    let mut client_handler = accept(client_handler).await.unwrap();

    client_handler.close(None).await.unwrap(); // send close to client

    // This read should succeed even though we already initiated a close
    let message = client_handler.read().await.unwrap();
    assert_eq!(message.into_data(), b"Hello WebSocket"[..]);

    assert!(client_handler.read().await.unwrap().is_close()); // receive acknowledgement

    let err = client_handler.read().await.unwrap_err(); // now we should get ConnectionClosed
    match err {
        Error::ConnectionClosed => {}
        _ => panic!("unexpected error: {err:?}"),
    }

    drop(client_handler);

    client_conn.await;
}
