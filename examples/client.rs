use monoio::io::sink::SinkExt;
use monoio_tungstenite::{Message, connect};

#[monoio::main]
async fn main() {
    let (mut socket, response) = connect("ws://localhost:3012/")
        .await
        .expect("Failed to connect");

    println!("Connected to the server");
    println!("Response HTTP code: {}", response.status());
    println!("Response contains the following headers:");
    for (header, _value) in response.headers() {
        println!("* {header}");
    }

    socket
        .send_and_flush(Message::Text("Hello WebSocket".into()))
        .await
        .expect("Error writing message");

    let msg = socket.read().await.expect("Error reading message");
    println!("Received: {msg}");

    socket.close(None).await.expect("Error closing socket");
}
