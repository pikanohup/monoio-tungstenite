use clap::{Parser, ValueEnum};
use monoio::{
    io::{sink::SinkExt, stream::Stream},
    net::{TcpListener, TcpStream},
};
use monoio_tungstenite::accept;

/// arguments for the WebSocket server
#[derive(Parser, Debug)]
struct Args {
    /// listening host
    #[arg(short, long, default_value = "127.0.0.1")]
    host: String,

    /// listening port
    #[arg(short, long, default_value = "3012")]
    port: u16,

    /// driver to use
    #[arg(value_enum, default_value = "IoUring")]
    driver: Driver,
}

#[derive(ValueEnum, Debug, Clone, Copy)]
enum Driver {
    Legacy,
    IoUring,
}

fn main() {
    let args = Args::parse();
    println!("Parsed args: {args:?}");

    let Args { host, port, driver } = args;
    match driver {
        Driver::Legacy => monoio::start::<monoio::LegacyDriver, _>(serv(host, port)),
        Driver::IoUring => monoio::start::<monoio::IoUringDriver, _>(serv(host, port)),
    }
}

async fn serv(host: String, port: u16) {
    let server = TcpListener::bind((host, port)).expect("Failed to bind");

    while let Ok((stream, _)) = server.accept().await {
        monoio::spawn(handle_connection(stream));
    }
}

async fn handle_connection(stream: TcpStream) {
    let mut ws = accept(stream)
        .await
        .expect("Failed to accept WebSocket connection");

    while let Some(Ok(msg)) = ws.next().await {
        if msg.is_binary() || msg.is_text() {
            ws.send_and_flush(msg).await.expect("Error writing message");
        }
    }
}
