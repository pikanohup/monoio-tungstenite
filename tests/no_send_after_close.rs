//! Verifies that we can read data messages even if we have initiated a close handshake,
//! but before we got confirmation.

#![cfg(feature = "handshake")]

use std::time::Duration;

use monoio::{net::TcpListener, time::sleep};
use monoio_tungstenite::{Error, Message, accept, connect, error::ProtocolError};

#[monoio::test(timer_enabled = true)]
async fn test_no_send_after_close() {
    let server = TcpListener::bind("localhost:3013").unwrap();

    let client_conn = monoio::spawn(async move {
        sleep(Duration::from_secs(1)).await;
        let (mut client, _) = connect("ws://localhost:3013/socket").await.unwrap();

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

    let err = client_handler
        .write(Message::Text("Hello WebSocket".into()))
        .await
        .unwrap_err();
    match err {
        Error::Protocol(s) => assert_eq!(s, ProtocolError::SendAfterClosing),
        e => panic!("unexpected error: {e:?}"),
    }

    drop(client_handler);

    client_conn.await;
}
