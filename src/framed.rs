// This file is mostly copied from https://github.com/monoio-rs/monoio-codec/blob/fa8e401122b93c33515afbe3268d0a59730e42c1/src/framed.rs
//
// The original code is dual-licensed under MIT or Apache 2.0.
//
// Copyright (c) 2024 Monoio Contributors
//
// The full text of both licenses is available in the `NOTICE` file in the root of this repository.

use bytes::BytesMut;
use monoio::{
    BufResult,
    buf::{IoBufMut, IoVecBufMut},
    io::{AsyncReadRent, AsyncWriteRent, AsyncWriteRentExt, sink::Sink, stream::Stream},
};
use monoio_codec::{Decoder, Encoder, FramedRead};

#[derive(Debug, Clone)]
pub struct FramedWrite<IO, Codec> {
    io: IO,
    codec: Codec,
    buf: BytesMut,
    backpressure: usize,
}

impl<IO, Codec> FramedWrite<IO, Codec> {
    /// Creates a new `FrameEncoder`.
    pub fn new(io: IO, codec: Codec, backpressure: usize) -> Self {
        Self {
            io,
            codec,
            backpressure,
            buf: BytesMut::with_capacity(backpressure),
        }
    }

    /// Sets the backpressure limit for the write buffer.
    pub fn set_backpressure(&mut self, backpressure: usize) {
        self.backpressure = backpressure;
    }

    /// Consumes the `FramedWrite` and returns the underlying I/O stream.
    #[inline]
    pub fn into_inner(self) -> IO {
        self.io
    }

    /// Returns a reference to the underlying I/O stream.
    #[inline]
    pub fn get_ref(&self) -> &IO {
        &self.io
    }

    /// Returns a mutable reference to the underlying I/O stream.
    #[inline]
    pub fn get_mut(&mut self) -> &mut IO {
        &mut self.io
    }

    /// Returns a reference to the underlying encoder.
    pub fn encoder(&self) -> &Codec {
        &self.codec
    }

    /// Returns a mutable reference to the underlying encoder.
    pub fn encoder_mut(&mut self) -> &mut Codec {
        &mut self.codec
    }

    /// Returns a reference to the write buffer.
    pub fn write_buffer(&self) -> &BytesMut {
        &self.buf
    }

    /// Returns a mutable reference to the write buffer.
    pub fn write_buffer_mut(&mut self) -> &mut BytesMut {
        &mut self.buf
    }

    /// Equivalent to [`Sink::send`] but with custom codec.
    #[inline]
    pub async fn send_with<C: Encoder<Item>, Item>(
        &mut self,
        codec: &mut C,
        item: Item,
    ) -> Result<(), C::Error>
    where
        IO: AsyncWriteRent,
    {
        if self.buf.len() > self.backpressure {
            self.flush().await?;
        }

        codec.encode(item, &mut self.buf)?;
        Ok(())
    }

    #[inline]
    async fn flush(&mut self) -> std::io::Result<()>
    where
        IO: AsyncWriteRent,
    {
        if self.buf.is_empty() {
            return Ok(());
        }

        let buf = std::mem::replace(&mut self.buf, BytesMut::new());
        let (res, buf) = self.io.write_all(buf).await;
        self.buf = buf;
        res?;

        self.buf.clear();
        self.io.flush().await?;
        Ok(())
    }
}

impl<IO, Codec, Item> Sink<Item> for FramedWrite<IO, Codec>
where
    IO: AsyncWriteRent,
    Codec: Encoder<Item>,
{
    type Error = Codec::Error;

    async fn send(&mut self, item: Item) -> Result<(), Self::Error> {
        if self.buf.len() > self.backpressure {
            // return Err(Error::WriteBufferFull(Message::Frame(item)));
            Self::flush(self).await?;
        }

        self.codec.encode(item, &mut self.buf)?;
        Ok(())
    }

    async fn flush(&mut self) -> Result<(), Self::Error> {
        Self::flush(self).await?;
        Ok(())
    }

    async fn close(&mut self) -> Result<(), Self::Error> {
        Self::flush(self).await?;
        self.io.shutdown().await?;
        Ok(())
    }
}

impl<IO, Codec> AsyncReadRent for FramedWrite<IO, Codec>
where
    IO: AsyncReadRent,
{
    async fn read<T: IoBufMut>(&mut self, buf: T) -> BufResult<usize, T> {
        self.io.read(buf).await
    }

    async fn readv<T: IoVecBufMut>(&mut self, buf: T) -> BufResult<usize, T> {
        self.io.readv(buf).await
    }
}

#[derive(Debug)]
pub struct Framed<IO, Dec, Enc> {
    inner: FramedRead<FramedWrite<IO, Enc>, Dec>,
}

impl<IO, Dec, Enc> Framed<IO, Dec, Enc> {
    /// Creates a new `Framed`.
    pub fn new(io: IO, dec: Dec, enc: Enc, backpressure: usize) -> Self {
        let framed_write = FramedWrite::new(io, enc, backpressure);
        let inner = FramedRead::with_capacity(framed_write, dec, 8192);
        Self { inner }
    }

    /// Consumes the `Framed` and returns the underlying I/O stream.
    pub fn into_inner(self) -> IO {
        self.inner.into_inner().into_inner()
    }

    /// Returns a reference to the underlying decoder.
    pub fn decoder(&self) -> &Dec {
        self.inner.decoder()
    }

    /// Returns a mutable reference to the underlying decoder.
    pub fn decoder_mut(&mut self) -> &mut Dec {
        self.inner.decoder_mut()
    }

    /// Returns a reference to the underlying encoder.
    pub fn encoder(&self) -> &Enc {
        self.framed_write().encoder()
    }

    /// Returns a mutable reference to the underlying encoder.
    pub fn encoder_mut(&mut self) -> &mut Enc {
        self.framed_write_mut().encoder_mut()
    }

    /// Returns a reference to the underlying I/O stream.
    #[inline]
    pub fn get_ref(&self) -> &IO {
        self.framed_write().get_ref()
    }

    /// Returns a mutable reference to the underlying I/O stream.
    #[inline]
    pub fn get_mut(&mut self) -> &mut IO {
        self.framed_write_mut().get_mut()
    }

    /// Returns a reference to the inner `FramedWrite`.
    #[inline]
    pub fn framed_write(&self) -> &FramedWrite<IO, Enc> {
        self.inner.get_ref()
    }

    /// Returns a mutable reference to the inner `FramedWrite`.
    #[inline]
    pub fn framed_write_mut(&mut self) -> &mut FramedWrite<IO, Enc> {
        self.inner.get_mut()
    }

    /// Returns a reference to the read buffer.
    pub fn read_buffer(&self) -> &BytesMut {
        self.inner.read_buffer()
    }

    /// Returns a mutable reference to the read buffer.
    pub fn read_buffer_mut(&mut self) -> &mut BytesMut {
        self.inner.read_buffer_mut()
    }

    /// Equivalent to [`Stream::next`] but with custom codec.
    #[inline]
    pub async fn next_with<C: Decoder>(
        &mut self,
        codec: &mut C,
    ) -> Option<Result<C::Item, C::Error>>
    where
        IO: AsyncReadRent,
    {
        self.inner.next_with(codec).await
    }

    /// Equivalent to [`Sink::send`] but with custom codec.
    #[inline]
    pub async fn send_with<C, Item>(&mut self, codec: &mut C, item: Item) -> Result<(), C::Error>
    where
        IO: AsyncWriteRent,
        C: Encoder<Item>,
    {
        self.framed_write_mut().send_with(codec, item).await
    }
}

impl<IO, Dec, Enc> Stream for Framed<IO, Dec, Enc>
where
    IO: AsyncReadRent,
    Dec: Decoder,
{
    type Item = Result<Dec::Item, Dec::Error>;

    async fn next(&mut self) -> Option<Self::Item> {
        self.inner.next().await
    }
}

impl<IO, Dec, Enc, Item> Sink<Item> for Framed<IO, Dec, Enc>
where
    IO: AsyncWriteRent,
    Enc: Encoder<Item>,
{
    type Error = Enc::Error;

    async fn send(&mut self, item: Item) -> Result<(), Self::Error> {
        Sink::send(self.framed_write_mut(), item).await
    }

    async fn flush(&mut self) -> Result<(), Self::Error> {
        Sink::flush(self.framed_write_mut()).await
    }

    async fn close(&mut self) -> Result<(), Self::Error> {
        Sink::close(self.framed_write_mut()).await
    }
}
