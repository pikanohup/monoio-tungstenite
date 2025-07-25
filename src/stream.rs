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
pub enum MaybeTlsStream<S> {
    /// Unencrypted socket stream.
    Plain(S),
    // TODO: TLS<S>,
}

impl<S> MaybeTlsStream<S> {
    /// Returns a shared reference to the inner stream.
    pub fn get_ref(&self) -> &S {
        match self {
            MaybeTlsStream::Plain(s) => s,
        }
    }
}

impl<S: AsyncReadRent> AsyncReadRent for MaybeTlsStream<S> {
    async fn read<T: IoBufMut>(&mut self, buf: T) -> BufResult<usize, T> {
        match self {
            MaybeTlsStream::Plain(s) => s.read(buf).await,
        }
    }

    async fn readv<T: IoVecBufMut>(&mut self, buf: T) -> BufResult<usize, T> {
        match self {
            MaybeTlsStream::Plain(s) => s.readv(buf).await,
        }
    }
}

impl<S: AsyncWriteRent> AsyncWriteRent for MaybeTlsStream<S> {
    async fn write<T: IoBuf>(&mut self, buf: T) -> BufResult<usize, T> {
        match self {
            MaybeTlsStream::Plain(s) => s.write(buf).await,
        }
    }

    async fn writev<T: IoVecBuf>(&mut self, buf: T) -> BufResult<usize, T> {
        match self {
            MaybeTlsStream::Plain(s) => s.writev(buf).await,
        }
    }

    async fn flush(&mut self) -> std::io::Result<()> {
        match self {
            MaybeTlsStream::Plain(s) => s.flush().await,
        }
    }

    async fn shutdown(&mut self) -> std::io::Result<()> {
        match self {
            MaybeTlsStream::Plain(s) => s.shutdown().await,
        }
    }
}
