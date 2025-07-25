#![cfg(feature = "handshake")]

use local_sync::oneshot;
use monoio::{net::TcpListener, task::JoinHandle};
use monoio_tungstenite::{
    accept_hdr, connect,
    error::{Error, ProtocolError, SubProtocolError},
    handshake::{
        client::generate_key,
        server::{Request, Response},
    },
};

fn create_http_request(uri: &str, subprotocols: Option<Vec<String>>) -> http::Request<()> {
    let uri = uri.parse::<http::Uri>().unwrap();

    let authority = uri.authority().unwrap().as_str();
    let host = authority
        .find('@')
        .map(|idx| authority.split_at(idx + 1).1)
        .unwrap_or_else(|| authority);

    if host.is_empty() {
        panic!("Empty host name");
    }

    let mut builder = http::Request::builder()
        .method("GET")
        .header("Host", host)
        .header("Connection", "Upgrade")
        .header("Upgrade", "websocket")
        .header("Sec-WebSocket-Version", "13")
        .header("Sec-WebSocket-Key", generate_key());

    if let Some(subprotocols) = subprotocols {
        builder = builder.header("Sec-WebSocket-Protocol", subprotocols.join(", "));
    }

    builder.uri(uri).body(()).unwrap()
}

fn spawn_server(
    port: u16,
    server_subprotocols: Option<Vec<String>>,
    tx: oneshot::Sender<()>,
) -> JoinHandle<()> {
    monoio::spawn(async move {
        let server = TcpListener::bind(("localhost", port))
            .expect("Can't listen, is this port already in use?");
        tx.send(()).unwrap();

        let callback = |_request: &Request, mut response: Response| {
            if let Some(subprotocols) = server_subprotocols {
                let headers = response.headers_mut();
                headers.append(
                    "Sec-WebSocket-Protocol",
                    subprotocols.join(",").parse().unwrap(),
                );
            }
            Ok(response)
        };

        let (stream, _) = server.accept().await.expect("Failed to accept connection");
        let _ws = accept_hdr(stream, callback).await.unwrap();
    })
}

#[monoio::test]
async fn test_server_send_no_subprotocol() {
    let (tx, rx) = oneshot::channel();
    let h = spawn_server(3012, None, tx);
    rx.await.expect("Failed to wait for server to be ready");

    let err = connect(create_http_request(
        "ws://localhost:3012",
        Some(vec!["my-sub-protocol".into()]),
    ))
    .await
    .unwrap_err();

    assert!(matches!(
        err,
        Error::Protocol(ProtocolError::SecWebSocketSubProtocolError(
            SubProtocolError::NoSubProtocol
        ))
    ));

    h.await;
}

#[monoio::test]
async fn test_server_sent_subprotocol_none_requested() {
    let (tx, rx) = oneshot::channel();
    let h = spawn_server(3013, Some(vec!["my-sub-protocol".to_string()]), tx);
    rx.await.expect("Failed to wait for server to be ready");

    let err = connect(create_http_request("ws://localhost:3013", None))
        .await
        .unwrap_err();

    assert!(matches!(
        err,
        Error::Protocol(ProtocolError::SecWebSocketSubProtocolError(
            SubProtocolError::ServerSentSubProtocolNoneRequested
        ))
    ));

    h.await;
}

#[monoio::test]
async fn test_invalid_subprotocol() {
    let (tx, rx) = oneshot::channel();
    let h = spawn_server(3014, Some(vec!["invalid-sub-protocol".to_string()]), tx);
    rx.await.expect("Failed to wait for server to be ready");

    let err = connect(create_http_request(
        "ws://localhost:3014",
        Some(vec!["my-sub-protocol".to_string()]),
    ))
    .await
    .unwrap_err();

    assert!(matches!(
        err,
        Error::Protocol(ProtocolError::SecWebSocketSubProtocolError(
            SubProtocolError::InvalidSubProtocol
        ))
    ));

    h.await;
}

#[monoio::test]
async fn test_request_multiple_subprotocols() {
    let (tx, rx) = oneshot::channel();
    let h = spawn_server(3015, Some(vec!["my-sub-protocol".to_string()]), tx);
    rx.await.expect("Failed to wait for server to be ready");

    let (_, response) = connect(create_http_request(
        "ws://localhost:3015",
        Some(vec![
            "my-sub-protocol".to_string(),
            "my-sub-protocol-1".to_string(),
            "my-sub-protocol-2".to_string(),
        ]),
    ))
    .await
    .unwrap();

    assert_eq!(
        response.headers().get("Sec-WebSocket-Protocol").unwrap(),
        "my-sub-protocol".parse::<http::HeaderValue>().unwrap()
    );

    h.await;
}

#[monoio::test]
async fn test_request_multiple_subprotocols_with_initial_unknown() {
    let (tx, rx) = oneshot::channel();
    let h = spawn_server(3016, Some(vec!["my-sub-protocol".to_string()]), tx);
    rx.await.expect("Failed to wait for server to be ready");

    let (_, response) = connect(create_http_request(
        "ws://localhost:3016",
        Some(vec![
            "protocol-unknown-to-server".to_string(),
            "my-sub-protocol".to_string(),
        ]),
    ))
    .await
    .unwrap();

    assert_eq!(
        response.headers().get("Sec-WebSocket-Protocol").unwrap(),
        "my-sub-protocol".parse::<http::HeaderValue>().unwrap()
    );

    h.await;
}

#[monoio::test]
async fn test_request_single_subprotocol() {
    let (tx, rx) = oneshot::channel();
    let h = spawn_server(3017, Some(vec!["my-sub-protocol".to_string()]), tx);
    rx.await.expect("Failed to wait for server to be ready");

    let (_, response) = connect(create_http_request(
        "ws://localhost:3017",
        Some(vec!["my-sub-protocol".to_string()]),
    ))
    .await
    .unwrap();

    assert_eq!(
        response.headers().get("Sec-WebSocket-Protocol").unwrap(),
        "my-sub-protocol".parse::<http::HeaderValue>().unwrap()
    );

    h.await;
}
