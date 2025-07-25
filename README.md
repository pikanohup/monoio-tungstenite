# monoio-tungstenite

Lightweight, asynchronous WebSocket implementation for [`monoio`](https://github.com/bytedance/monoio) runtime, adapted from [`tungstenite-rs`](https://github.com/snapview/tungstenite-rs).

```rust
use monoio::{
    io::{sink::SinkExt, stream::Stream},
    net::TcpListener,
};
use monoio_tungstenite::accept;

/// A WebSocket echo server.
#[monoio::main]
async fn main() {
    let server = TcpListener::bind("127.0.0.1:9001").unwrap();

    while let Ok((stream, _)) = server.accept().await {
        monoio::spawn(async move {
            let mut websocket = accept(stream).await.unwrap();

            while let Some(Ok(msg)) = websocket.next().await {
                // We do not want to send back ping/pong messages.
                if msg.is_binary() || msg.is_text() {
                    websocket.send_and_flush(msg).await.unwrap();
                }
            }
        });
    }
}
```

For more examples, please refer to the `examples/` directory.

> [!IMPORTANT]
> This project was initially developed for personal use and has not been battle-tested in large-scale production environments. Please use it with caution, especially in production systems.

## Introduction

This crate offers a native WebSocket implementation for `monoio`, based on the widely-used and reliable `tungstenite-rs`. Instead of relying on [`IntoPollIo`](https://github.com/bytedance/monoio/blob/master/docs/en/poll-io.md) to simply wrap and reuse `tokio-tungstenite` or other poll-based libraries, it is built directly on `monoio`'s native IO model (`AsyncReadRent`/`AsyncWriteRent`), with the help of the `monoio-codec` crate.

## Features

TODO: Add TLS support

## Testing

`monoio-tungstenite` passes the [Autobahn Testsuite](https://github.com/crossbario/autobahn-testsuite). It is also covered by internal unit tests as well as possible.

## License

This project is dual-licensed, allowing you to choose between either the [MIT License](LICENSE-MIT) or the [Apache-2.0 License](LICENSE-APACHE) at your option.

For details on third-party library attributions, please see the [NOTICE](NOTICE) file.
