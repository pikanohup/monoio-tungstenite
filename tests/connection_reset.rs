//! Verifies that the server returns a `ConnectionClosed` error when the connection
//! is closed from the server's point of view and drop the underlying tcp socket.

#![cfg(feature = "handshake")]

use monoio::{io::sink::SinkExt, net::TcpListener};
use monoio_tungstenite::{Error, Message, accept, connect};

#[monoio::test]
async fn test_server_close() {
    let server =
        TcpListener::bind(("127.0.0.1", 3012)).expect("Can't listen, is port already in use?");

    let client_conn = monoio::spawn(async move {
        let (mut client, _) = connect("ws://127.0.0.1:3012").await.unwrap();

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

    let message = client_handler.read().await.unwrap();
    assert_eq!(message.into_data(), b"Hello WebSocket"[..]);

    client_handler.close(None).await.unwrap(); // send close to client

    let message = client_handler.read().await.unwrap(); // receive acknowledgement
    assert!(message.is_close());

    let err = client_handler.read().await.unwrap_err(); // now we should get ConnectionClosed
    match err {
        Error::ConnectionClosed => {}
        _ => panic!("unexpected error: {err:?}"),
    }

    drop(client_handler);

    client_conn.await;
}

#[monoio::test]
async fn test_client_close() {
    let server =
        TcpListener::bind(("127.0.0.1", 3014)).expect("Can't listen, is port already in use?");

    let client_conn = monoio::spawn(async move {
        let (mut client, _) = connect("ws://127.0.0.1:3014").await.unwrap();

        client
            .send_and_flush(Message::Text("Hello WebSocket".into()))
            .await
            .unwrap();

        let message = client.read().await.unwrap(); // receive answer from server
        assert_eq!(message.into_data(), b"From Server"[..]);

        client.close(None).await.unwrap(); // send close to server

        let message = client.read().await.unwrap(); // receive acknowledgement from server
        assert!(message.is_close());

        let err = client.read().await.unwrap_err(); // now we should get ConnectionClosed
        match err {
            Error::ConnectionClosed => {}
            _ => panic!("unexpected error: {err:?}"),
        }
    });

    let (client_handler, _) = server.accept().await.unwrap();
    let mut client_handler = accept(client_handler).await.unwrap();

    let message = client_handler.read().await.unwrap();
    assert_eq!(message.into_data(), b"Hello WebSocket"[..]);

    client_handler
        .send_and_flush(Message::Text("From Server".into()))
        .await
        .unwrap();

    let message = client_handler.read().await.unwrap(); // receive close from client
    assert!(message.is_close());

    let err = client_handler.read().await.unwrap_err(); // now we should get ConnectionClosed
    match err {
        Error::ConnectionClosed => {}
        _ => panic!("unexpected error: {err:?}"),
    }

    drop(client_handler);

    client_conn.await;
}
