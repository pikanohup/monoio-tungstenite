use monoio::{
    io::{sink::SinkExt, stream::Stream},
    net::{TcpListener, TcpStream},
};
use monoio_tungstenite::{
    accept_hdr,
    handshake::server::{Request, Response},
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
    let callback = |req: &Request, mut response: Response| {
        println!("Received a new ws handshake");
        println!("The request's path is: {}", req.uri().path());
        println!("The request's headers are:");
        for (header, _value) in req.headers() {
            println!("* {header}");
        }

        // Let's add an additional header to our response to the client.
        let headers = response.headers_mut();
        headers.append("MyCustomHeader", ":)".parse().unwrap());
        headers.append("SOME_TUNGSTENITE_HEADER", "header_value".parse().unwrap());

        Ok(response)
    };

    let mut ws = accept_hdr(stream, callback)
        .await
        .expect("Failed to accept WebSocket connection");
    println!("WebSocket connection established");

    while let Some(msg) = ws.next().await {
        match msg {
            Ok(msg) => {
                if msg.is_binary() || msg.is_text() {
                    ws.send_and_flush(msg).await.expect("Error writing message");
                }
            }

            Err(e) => eprintln!("Error receiving message: {e}"),
        }
    }
    println!("WebSocket connection closed");
}
