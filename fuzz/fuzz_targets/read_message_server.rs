#![no_main]

use libfuzzer_sys::fuzz_target;
use monoio_tungstenite::{WebSocket, protocol::Role};

mod shared;
use shared::{MockWrite, RUNTIME};

fuzz_target!(|data: &[u8]| {
    RUNTIME.with(|runtime| {
        runtime.borrow_mut().block_on(async {
            let mut socket = WebSocket::from_raw_socket(MockWrite(data), Role::Server, None);
            socket.read().await.ok();
        });
    });
});
