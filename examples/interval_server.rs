use std::{net::SocketAddr, time::Duration};

use monoio::{
    io::{sink::SinkExt, stream::Stream},
    net::{TcpListener, TcpStream},
};
use monoio_tungstenite::{Error, Message, Result, accept};

async fn accept_connection(peer: SocketAddr, stream: TcpStream) {
    if let Err(e) = handle_connection(peer, stream).await {
        match e {
            Error::ConnectionClosed | Error::Protocol(_) | Error::Utf8(_) => (),
            err => eprintln!("Error processing connection: {err}"),
        }
    }
}

async fn handle_connection(peer: SocketAddr, stream: TcpStream) -> Result<()> {
    let mut ws_stream = accept(stream).await.expect("Failed to accept");
    println!("New WebSocket connection: {peer}");
    let mut interval = monoio::time::interval(Duration::from_millis(1000));

    // Echo incoming WebSocket messages and send a message periodically every second.

    loop {
        monoio::select! {
            msg = ws_stream.next() => {
                match msg {
                    Some(msg) => {
                        let msg = msg?;
                        if msg.is_text() || msg.is_binary() {
                            ws_stream.send_and_flush(msg).await?;
                        } else if msg.is_close() {
                            break;
                        }
                    }
                    None => break,
                }
            }

            _ = interval.tick() => {
                ws_stream.send_and_flush(Message::text("tick")).await?;
            }
        }
    }

    Ok(())
}

#[monoio::main(enable_timer = true)]
async fn main() {
    let addr = "127.0.0.1:9002";
    let listener = TcpListener::bind(addr).expect("Can't listen");
    println!("Listening on: {addr}");

    while let Ok((stream, _)) = listener.accept().await {
        let peer = stream
            .peer_addr()
            .expect("connected streams should have a peer address");
        println!("Peer address: {peer}");

        monoio::spawn(accept_connection(peer, stream));
    }
}
