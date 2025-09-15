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

This crate offers a native WebSocket implementation for `monoio`, based on the widely-used and reliable `tungstenite-rs`. Instead of relying on [`IntoPollIo`](https://github.com/bytedance/monoio/blob/master/docs/en/poll-io.md) to simply wrap and reuse `tokio-tungstenite` or other poll-based libraries, it is built directly on `monoio`'s native IO model (`AsyncReadRent`/`AsyncWriteRent`), fully utilizing `io_uring`'s capabilities while ensuring seamless ecosystem integration.

## Features

`monoio-tungstenite` provides a complete implementation of the WebSocket specification. TLS is supported on all platforms using [`native-tls`](https://crates.io/crates/native-tls) or [`rustls`](https://crates.io/crates/rustls) . The following features are available:

* `native-tls`: Enables TLS support using the `native-tls` crate.
* `native-tls-vendored`: Same as `native-tls` but vendors OpenSSL for Linux builds.
* `rustls-tls`: Enables TLS support using the `rustls` crate.
* `rustls-tls-native-roots`: Enables `rustls` with native root certificates.
* `rustls-tls-webpki-roots`: Enables `rustls` with root certificates from the [`webpki-roots`](https://crates.io/crates/webpki-roots) crate.
* `rustls-tls-unsafe-io`: Exposes the `unsafe_io` feature in `monoio-rustls` to work around a known `io-cancellation` issue. **Please read the ["Known Issues" section](#known-issues) carefully for crucial details before using this feature**.

Choose the one that is appropriate for your needs.

By default **no TLS feature is activated**, so make sure you use one of the TLS features, otherwise you won't be able to communicate with the TLS endpoints.

Please note that `permessage-deflate` is not supported at this time.

## Testing

`monoio-tungstenite` passes the [Autobahn Testsuite](https://github.com/crossbario/autobahn-testsuite). It is also covered by internal unit tests as well as possible.

## Known Issues

### Concurrency with TLS and Cancellation Safety

When using `select!` for concurrent reads and writes on a `WebSocket<Stream>` with any TLS backend (`rustls` or `native-tls`), you may encounter a runtime panic. This is due to an upstream issue in `monoio-io-wrapper`, a crate used by both `monoio-rustls` and `monoio-native-tls`, where its default `SafeRead` is not cancellation-safe. Concurrent operations in `select!` can lead to one operation being cancelled, which triggers the panic. 

For more details and the original discussion, please see [this issue](https://github.com/pikanohup/monoio-tungstenite/issues/1).

#### Workaround

For users of the `rustls` backend, a workaround is available. It requires enabling the `rustls-tls-unsafe-io` feature in `monoio-tungstenite`, which in turn allows you to use the `unsafe_io()` method on a `TlsConnector` to create a connector configured for the alternative IO mode. For example:

```rust
use monoio_tungstenite::client::connect_tls_with_config;
use monoio_tungstenite::tls::rustls::default_connector;

...

let connector = default_connector()?;
let connector = unsafe { connector.unsafe_io(true) };
let (mut ws, _) = connect_tls_with_config("wss://example.com", None, false, Some(connector.into())).await?;

```

> [!WARNING]
> While it resolves the immediate panic, the `unsafe` designation implies there may be potential side effects or subtle bugs that have not been fully investigated within the context of this library.
> By calling the unsafe method, you are opting into this mode and acknowledging these potential unknown risks. Please use this feature with caution. The recommended long-term solution remains a proper fix in the upstream dependency.

Currently, this workaround is **not available for the `native-tls` backend** as the underlying `monoio-native-tls` crate does not expose the `unsafe_io` option.

## License

This project is dual-licensed, allowing you to choose between either the [MIT License](LICENSE-MIT) or the [Apache-2.0 License](LICENSE-APACHE) at your option.

For details on third-party library attributions, please see the [NOTICE](NOTICE) file.
