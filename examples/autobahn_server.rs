use monoio::{
    io::{sink::SinkExt, stream::Stream},
    net::{TcpListener, TcpStream},
};
use monoio_tungstenite::{Error, Result, accept};

async fn accept_connection(stream: TcpStream) {
    if let Err(e) = handle_connection(stream).await {
        match e {
            Error::ConnectionClosed | Error::Protocol(_) | Error::Utf8(_) => (),
            err => eprintln!("Error processing connection: {err}"),
        }
    }
}

async fn handle_connection(stream: TcpStream) -> Result<()> {
    let mut ws_stream = accept(stream).await.expect("Failed to accept");

    while let Some(msg) = ws_stream.next().await {
        let msg = msg?;
        if msg.is_text() || msg.is_binary() {
            ws_stream.send_and_flush(msg).await?;
        }
    }

    Ok(())
}

#[monoio::main]
async fn main() {
    let listener = TcpListener::bind("127.0.0.1:9002").expect("Can't listen");

    while let Ok((stream, _)) = listener.accept().await {
        monoio::spawn(accept_connection(stream));
    }
}
