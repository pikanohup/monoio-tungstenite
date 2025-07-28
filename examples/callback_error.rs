use monoio::net::{TcpListener, TcpStream};
use monoio_tungstenite::{
    accept_hdr,
    handshake::server::{Request, Response},
    http::StatusCode,
};

#[monoio::main]
async fn main() {
    let server = TcpListener::bind("localhost:3012").expect("Failed to bind");

    while let Ok((stream, addr)) = server.accept().await {
        println!("New connection from: {addr}");
        monoio::spawn(handle_connection(stream));
    }
}

async fn handle_connection(stream: TcpStream) {
    let callback = |_req: &Request, _resp| {
        let resp = Response::builder()
            .status(StatusCode::FORBIDDEN)
            .body(Some("Access denied".into()))
            .unwrap();
        Err(Box::new(resp))
    };

    accept_hdr(stream, callback).await.unwrap_err();
    println!("WebSocket connection refused");
}
