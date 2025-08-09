// This file is modified from https://github.com/monoio-rs/monoio-codec/blob/fa8e401122b93c33515afbe3268d0a59730e42c1/src/framed.rs
//
// The original code is dual-licensed under MIT or Apache 2.0.
//
// Copyright (c) 2024 Monoio Contributors
//
// The full text of both licenses is available in the `NOTICE` file in the root of this repository.

//! WebSocket frame codec implementation.

use bytes::BytesMut;
use monoio::io::{AsyncReadRent, AsyncWriteRent, AsyncWriteRentExt, sink::Sink, stream::Stream};
use monoio_codec::{Decoder, Encoder, FramedRead};

use crate::{error::Error, protocol::frame::Frame};

mod decode;
pub use decode::FrameDecoder;

mod encode;
pub use encode::FrameEncoder;

/// Codec for WebSocket frames.
#[derive(Debug)]
pub struct FrameCodec<IO> {
    inner: FramedRead<IO, FrameDecoder>,
    write_buf: BytesMut,
    write_limit: usize,
}

impl<IO> FrameCodec<IO> {
    /// Creates a new `FrameCodec`.
    pub fn new(io: IO, write_limit: usize) -> Self {
        Self {
            inner: FramedRead::new(io, FrameDecoder::default()),
            write_buf: BytesMut::with_capacity(write_limit),
            write_limit,
        }
    }

    /// Creates a new `FrameCodec` from an existing `FramedRead`, allowing for reuse of the read
    /// buffer.
    pub fn from_framed_read<C>(framed_read: FramedRead<IO, C>, write_limit: usize) -> Self {
        Self {
            inner: framed_read.map_decoder(|_| FrameDecoder::default()),
            write_buf: BytesMut::with_capacity(write_limit),
            write_limit,
        }
    }

    /// Returns the write limit for the codec.
    pub fn write_limit(&self) -> usize {
        self.write_limit
    }

    /// Sets the write limit for the codec.
    pub fn set_write_limit(&mut self, limit: usize) {
        self.write_limit = limit;
    }

    /// Returns a reference to the write buffer.
    pub fn write_buffer(&self) -> &BytesMut {
        &self.write_buf
    }

    /// Returns a mutable reference to the write buffer.
    pub fn write_buffer_mut(&mut self) -> &mut BytesMut {
        &mut self.write_buf
    }

    /// Returns a reference to the underlying `IO`.
    pub fn get_ref(&self) -> &IO {
        self.inner.get_ref()
    }

    /// Returns a mutable reference to the underlying `IO`.
    pub fn get_mut(&mut self) -> &mut IO {
        self.inner.get_mut()
    }

    /// Consumes the `FrameCodec` and returns the underlying `IO`.
    pub fn into_inner(self) -> IO {
        self.inner.into_inner()
    }

    /// Returns a mutable reference to the inner `FrameDecoder`.
    pub fn decoder_mut(&mut self) -> &mut FrameDecoder {
        self.inner.decoder_mut()
    }

    /// Equivalent to [`Sink::send`] but with custom codec.
    pub async fn send_with<C: Encoder<Item>, Item>(
        &mut self,
        codec: &mut C,
        item: Item,
    ) -> Result<(), C::Error>
    where
        IO: AsyncWriteRent,
    {
        if self.write_buf.len() > self.write_limit {
            self.flush().await?;
        }

        codec.encode(item, &mut self.write_buf)?;
        Ok(())
    }

    /// Equivalent to [`Stream::next`] but with custom codec.
    pub async fn next_with<C: Decoder>(
        &mut self,
        codec: &mut C,
    ) -> Option<Result<C::Item, C::Error>>
    where
        IO: AsyncReadRent,
    {
        self.inner.next_with(codec).await
    }

    #[inline]
    async fn flush(&mut self) -> std::io::Result<()>
    where
        IO: AsyncWriteRent,
    {
        if self.write_buf.is_empty() {
            return Ok(());
        }

        let buf = std::mem::replace(&mut self.write_buf, BytesMut::new());
        let (res, buf) = self.get_mut().write_all(buf).await;
        self.write_buf = buf;
        res?;

        self.write_buf.clear();
        self.get_mut().flush().await?;
        Ok(())
    }
}

impl<IO> Sink<Frame> for FrameCodec<IO>
where
    IO: AsyncWriteRent,
{
    type Error = Error;

    async fn send(&mut self, frame: Frame) -> Result<(), Self::Error> {
        if self.write_buf.len() > self.write_limit {
            // return Err(Error::WriteBufferFull(Message::Frame(item)));
            Self::flush(self).await?;
        }

        FrameEncoder.encode(frame, &mut self.write_buf)?;
        Ok(())
    }

    async fn flush(&mut self) -> Result<(), Self::Error> {
        Self::flush(self).await?;
        Ok(())
    }

    async fn close(&mut self) -> Result<(), Self::Error> {
        Self::flush(self).await?;
        self.get_mut().shutdown().await?;
        Ok(())
    }
}

impl<IO> Stream for FrameCodec<IO>
where
    IO: AsyncReadRent,
{
    type Item = Result<Frame, Error>;

    async fn next(&mut self) -> Option<Self::Item> {
        self.inner.next().await
    }
}
