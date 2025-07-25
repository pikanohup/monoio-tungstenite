//! Convenience wrapper for streams to switch between plain TCP and TLS at runtime.
//!  There is no dependency on actual TLS implementations. Everything like
//! `native_tls` or `openssl` will work as long as there is a TLS stream supporting
//! `AsyncReadRent + AsyncWriteRent` traits.

use monoio::{
    BufResult,
    buf::{IoBuf, IoBufMut, IoVecBuf, IoVecBufMut},
    io::{AsyncReadRent, AsyncWriteRent},
};

/// Stream mode, either plain TCP or TLS.
#[derive(Clone, Copy, Debug)]
pub enum Mode {
    /// Plain mode (`ws://` URL).
    Plain,
    /// TLS mode (`wss://` URL).
    Tls,
}

/// A stream that might be protected with TLS.
#[non_exhaustive]
#[derive(Debug)]
#[allow(clippy::large_enum_variant)]
pub enum MaybeTlsStream<S> {
    /// Unencrypted socket stream.
    Plain(S),
    /// Encrypted socket stream using `native-tls`.
    #[cfg(feature = "native-tls")]
    NativeTls(monoio_native_tls::TlsStream<S>),
    /// Encrypted socket stream using `rustls`.
    #[cfg(feature = "rustls-tls")]
    Rustls(monoio_rustls::ClientTlsStream<S>),
}

impl<S: AsyncReadRent + AsyncWriteRent> AsyncReadRent for MaybeTlsStream<S> {
    async fn read<T: IoBufMut>(&mut self, buf: T) -> BufResult<usize, T> {
        match self {
            MaybeTlsStream::Plain(s) => s.read(buf).await,
            #[cfg(feature = "native-tls")]
            MaybeTlsStream::NativeTls(s) => s.read(buf).await,
            #[cfg(feature = "rustls-tls")]
            MaybeTlsStream::Rustls(s) => s.read(buf).await,
        }
    }

    async fn readv<T: IoVecBufMut>(&mut self, buf: T) -> BufResult<usize, T> {
        match self {
            MaybeTlsStream::Plain(s) => s.readv(buf).await,
            #[cfg(feature = "native-tls")]
            MaybeTlsStream::NativeTls(s) => s.readv(buf).await,
            #[cfg(feature = "rustls-tls")]
            MaybeTlsStream::Rustls(s) => s.readv(buf).await,
        }
    }
}

impl<S: AsyncReadRent + AsyncWriteRent> AsyncWriteRent for MaybeTlsStream<S> {
    async fn write<T: IoBuf>(&mut self, buf: T) -> BufResult<usize, T> {
        match self {
            MaybeTlsStream::Plain(s) => s.write(buf).await,
            #[cfg(feature = "native-tls")]
            MaybeTlsStream::NativeTls(s) => s.write(buf).await,
            #[cfg(feature = "rustls-tls")]
            MaybeTlsStream::Rustls(s) => s.write(buf).await,
        }
    }

    async fn writev<T: IoVecBuf>(&mut self, buf: T) -> BufResult<usize, T> {
        match self {
            MaybeTlsStream::Plain(s) => s.writev(buf).await,
            #[cfg(feature = "native-tls")]
            MaybeTlsStream::NativeTls(s) => s.writev(buf).await,
            #[cfg(feature = "rustls-tls")]
            MaybeTlsStream::Rustls(s) => s.writev(buf).await,
        }
    }

    async fn flush(&mut self) -> std::io::Result<()> {
        match self {
            MaybeTlsStream::Plain(s) => s.flush().await,
            #[cfg(feature = "native-tls")]
            MaybeTlsStream::NativeTls(s) => s.flush().await,
            #[cfg(feature = "rustls-tls")]
            MaybeTlsStream::Rustls(s) => s.flush().await,
        }
    }

    async fn shutdown(&mut self) -> std::io::Result<()> {
        match self {
            MaybeTlsStream::Plain(s) => s.shutdown().await,
            #[cfg(feature = "native-tls")]
            MaybeTlsStream::NativeTls(s) => s.shutdown().await,
            #[cfg(feature = "rustls-tls")]
            MaybeTlsStream::Rustls(s) => s.shutdown().await,
        }
    }
}
