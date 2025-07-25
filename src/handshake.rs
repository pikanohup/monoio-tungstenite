//! WebSocket handshake control.

pub mod client;
pub mod headers;
pub mod server;

use sha1::{Digest, Sha1};

/// Derives the `Sec-WebSocket-Accept` response header from a `Sec-WebSocket-Key` request header.
///
/// This function can be used to perform a handshake before passing a raw TCP stream to
/// [`WebSocket::from_raw_socket`][crate::protocol::WebSocket::from_raw_socket].
pub fn derive_accept_key(request_key: &[u8]) -> String {
    // ... field is constructed by concatenating /key/ ...
    // ... with the string "258EAFA5-E914-47DA-95CA-C5AB0DC85B11" (RFC 6455)
    const WS_GUID: &[u8] = b"258EAFA5-E914-47DA-95CA-C5AB0DC85B11";
    let mut sha1 = Sha1::default();
    sha1.update(request_key);
    sha1.update(WS_GUID);
    data_encoding::BASE64.encode(&sha1.finalize())
}

#[cfg(test)]
mod tests {
    use super::derive_accept_key;

    #[test]
    fn key_conversion() {
        // example from RFC 6455
        assert_eq!(
            derive_accept_key(b"dGhlIHNhbXBsZSBub25jZQ=="),
            "s3pPLMBiTxaQ9kYGzzhZRbK+xOo="
        );
    }
}
