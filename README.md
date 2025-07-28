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

[![Crates.io](https://img.shields.io/crates/v/monoio-tungstenite)](https://crates.io/crates/monoio-tungstenite)
[![Docs.rs](https://img.shields.io/docsrs/monoio-tungstenite)](https://docs.rs/monoio-tungstenite)
[![License](https://img.shields.io/crates/l/monoio-tungstenite)](LICENSE-MIT)
[![Build Status](https://github.com/pikanohup/monoio-tungstenite/actions/workflows/ci.yml/badge.svg)](https://github.com/pikanohup/monoio-tungstenite/actions)


## Introduction

This crate offers a native WebSocket implementation for `monoio`, based on the widely-used and reliable `tungstenite-rs`. Instead of relying on [`IntoPollIo`](https://github.com/bytedance/monoio/blob/master/docs/en/poll-io.md) to simply wrap and reuse `tokio-tungstenite` or other poll-based libraries, it is built directly on `monoio`'s native IO model (`AsyncReadRent`/`AsyncWriteRent`), with the help of the `monoio-codec` crate.

## Features

`monoio-tungstenite` provides a complete implementation of the WebSocket specification. TLS is supported on all platforms using [`native-tls`](https://github.com/sfackler/rust-native-tls) or [`rustls`](https://github.com/ctz/rustls) . The following features are available:

* `native-tls`
* `native-tls-vendored`
* `rustls-tls-native-roots`
* `rustls-tls-webpki-roots`

Choose the one that is appropriate for your needs.

By default **no TLS feature is activated**, so make sure you use one of the TLS features, otherwise you won't be able to communicate with the TLS endpoints.

Please note that `permessage-deflate` is not supported at this time.

## Testing

`monoio-tungstenite` passes the [Autobahn Testsuite](https://github.com/crossbario/autobahn-testsuite). It is also covered by internal unit tests as well as possible.

## License

This project is dual-licensed, allowing you to choose between either the [MIT License](LICENSE-MIT) or the [Apache-2.0 License](LICENSE-APACHE) at your option.

For details on third-party library attributions, please see the [NOTICE](NOTICE) file.
