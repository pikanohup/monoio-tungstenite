//! Split WebSocket into independent read and write halves.
//!
//! This module provides [`WebSocketReadHalf`] and [`WebSocketWriteHalf`] types that allow
//! independent, concurrent reading and writing on a WebSocket connection.
//!
//! # Threading Model
//!
//! The split halves use `Rc<RefCell<>>` for shared state and are **not `Send`**. This is
//! consistent with monoio's single-threaded, thread-per-core architecture. Both halves must
//! be used on the same thread as the original [`WebSocket`](super::WebSocket).
//!
//! # Auto-Pong Behavior
//!
//! When using a non-split [`WebSocket`](super::WebSocket), received Ping frames automatically
//! trigger an immediate Pong reply. In split mode, the read half queues Pong replies into shared
//! state and the write half sends them on the next [`write`](WebSocketWriteHalf::write) or
//! [`flush`](WebSocketWriteHalf::flush) call. This may result in slightly delayed Pong responses
//! compared to the non-split variant.

use std::{cell::RefCell, collections::VecDeque, rc::Rc};

use bytes::BytesMut;
use monoio::io::{AsyncReadRent, AsyncWriteRent, AsyncWriteRentExt, sink::Sink, stream::Stream};
use monoio_codec::{Encoder, FramedRead};

use crate::{
    error::{Error, ProtocolError, Result},
    protocol::{
        frame::{
            CloseFrame, Frame,
            codec::{FrameDecoder, FrameEncoder},
        },
        message::{IncompleteMessage, Message},
        websocket::{
            CheckConnectionReset, Role, WebSocketConfig, WebSocketState, handle_eof, handle_frame,
            message_into_frame, validate_frame,
        },
    },
};

/// Shared state between the read and write halves of a split WebSocket.
#[derive(Debug)]
struct SharedState {
    state: WebSocketState,
    pending_pongs: VecDeque<Frame>,
}

/// The read half of a split [`WebSocket`](super::WebSocket) connection.
///
/// Created by [`WebSocket::into_split`](super::WebSocket::into_split).
///
/// This half can read messages from the WebSocket. When a Ping frame is received,
/// the corresponding Pong reply is queued and will be sent by the
/// [`WebSocketWriteHalf`] on its next write or flush operation.
#[derive(Debug)]
pub struct WebSocketReadHalf<R: AsyncReadRent> {
    role: Role,
    frame_reader: FramedRead<R, FrameDecoder>,
    incomplete: Option<IncompleteMessage>,
    config: WebSocketConfig,
    shared: Rc<RefCell<SharedState>>,
}

/// The write half of a split [`WebSocket`](super::WebSocket) connection.
///
/// Created by [`WebSocket::into_split`](super::WebSocket::into_split).
///
/// This half can write messages to the WebSocket. On each write or flush,
/// any pending Pong replies (queued by the [`WebSocketReadHalf`] in response to Pings)
/// are sent first.
#[derive(Debug)]
pub struct WebSocketWriteHalf<W: AsyncWriteRent> {
    role: Role,
    writer: W,
    write_buf: BytesMut,
    write_limit: usize,
    shared: Rc<RefCell<SharedState>>,
}

/// Creates the shared state and split halves from decomposed WebSocket parts.
pub(crate) fn split_inner<R: AsyncReadRent, W: AsyncWriteRent>(
    role: Role,
    config: WebSocketConfig,
    state: WebSocketState,
    incomplete: Option<IncompleteMessage>,
    frame_reader: FramedRead<R, FrameDecoder>,
    writer: W,
    write_buf: BytesMut,
    write_limit: usize,
) -> (WebSocketReadHalf<R>, WebSocketWriteHalf<W>) {
    let shared = Rc::new(RefCell::new(SharedState {
        state,
        pending_pongs: VecDeque::new(),
    }));

    let read_half = WebSocketReadHalf {
        role,
        frame_reader,
        incomplete,
        config,
        shared: shared.clone(),
    };

    let write_half = WebSocketWriteHalf {
        role,
        writer,
        write_buf,
        write_limit,
        shared,
    };

    (read_half, write_half)
}

// =============================================================================
// WebSocketReadHalf implementation
// =============================================================================

impl<R: AsyncReadRent> WebSocketReadHalf<R> {
    /// Checks if it is possible to read messages.
    ///
    /// Reading is impossible after receiving `Message::Close`. It is still possible after
    /// sending close frame since the peer still may send some data before confirming close.
    pub fn can_read(&self) -> bool {
        self.shared.borrow().state.can_read()
    }

    /// Reads a message from the websocket.
    ///
    /// When a Ping frame is received, the corresponding Pong reply is queued
    /// in the shared state and will be sent by the write half on its next operation.
    pub async fn read(&mut self) -> Result<Message> {
        {
            let shared = self.shared.borrow();
            shared.state.check_not_terminated()?;

            if self.role == Role::Server && !shared.state.can_read() {
                drop(shared);
                self.shared.borrow_mut().state = WebSocketState::Terminated;
                return Err(Error::ConnectionClosed);
            }
        }

        loop {
            let (msg, auto_send) = self.read_message_frame().await?;
            if let Some(pong_frame) = auto_send {
                // Queue the pong in the shared state for the write half to send.
                self.shared.borrow_mut().pending_pongs.push_back(pong_frame);
            }

            if let Some(msg) = msg {
                return Ok(msg);
            }
        }
    }

    async fn read_message_frame(&mut self) -> Result<(Option<Message>, Option<Frame>)> {
        let state = self.shared.borrow().state;
        match self
            .frame_reader
            .next()
            .await
            .transpose()
            .check_connection_reset(state)?
        {
            Some(frame) => {
                if !self.shared.borrow().state.can_read() {
                    return Err(Error::Protocol(ProtocolError::ReceivedAfterClosing));
                }

                validate_frame(self.role, &frame)?;

                let mut shared = self.shared.borrow_mut();
                handle_frame(
                    &mut shared.state,
                    &mut self.incomplete,
                    self.config.max_message_size,
                    frame,
                )
            }

            None => Err(handle_eof(&mut self.shared.borrow_mut().state)),
        }
    }
}

impl<R: AsyncReadRent> Stream for WebSocketReadHalf<R> {
    type Item = Result<Message>;

    #[inline]
    async fn next(&mut self) -> Option<Self::Item> {
        match self.read().await {
            Ok(msg) => Some(Ok(msg)),
            Err(Error::AlreadyClosed | Error::ConnectionClosed) => None,
            Err(e) => Some(Err(e)),
        }
    }
}

// =============================================================================
// WebSocketWriteHalf implementation
// =============================================================================

impl<W: AsyncWriteRent> WebSocketWriteHalf<W> {
    /// Checks if it is possible to write messages.
    ///
    /// Writing gets impossible immediately after sending or receiving `Message::Close`.
    pub fn can_write(&self) -> bool {
        self.shared.borrow().state.is_active()
    }

    /// Writes a message into the websocket.
    ///
    /// Any pending Pong replies (queued by the read half) are sent first.
    ///
    /// Does **not** flush.
    pub async fn write(&mut self, message: Message) -> Result<()> {
        // Send any queued pong frames first.
        self.send_pending_pongs().await?;

        {
            let shared = self.shared.borrow();
            shared.state.check_not_terminated()?;

            if !shared.state.is_active() {
                return Err(Error::Protocol(ProtocolError::SendAfterClosing));
            }
        }

        let frame = match message_into_frame(message) {
            Ok(frame) => frame,
            Err(code) => return self.close(code).await,
        };

        self.write_frame(frame).await?;
        Ok(())
    }

    /// Closes the connection.
    ///
    /// This function guarantees that the close frame will be queued.
    /// There is no need to call it again. Calling this function is
    /// the same as calling `write(Message::Close(..))`.
    pub async fn close(&mut self, code: Option<CloseFrame>) -> Result<()> {
        if self.shared.borrow().state == WebSocketState::Active {
            self.shared.borrow_mut().state = WebSocketState::ClosedByUs;
            let frame = Frame::close(code);
            self.write_frame(frame).await?;
        }

        self.flush().await
    }

    /// Flushes the writes.
    ///
    /// Ensures all messages previously passed to [`write`](Self::write) and automatically
    /// queued pong responses are written & flushed into the stream.
    pub async fn flush(&mut self) -> Result<()> {
        // Send any queued pong frames first.
        self.send_pending_pongs().await?;

        let state = self.shared.borrow().state;

        if self.role == Role::Server && !state.can_read() {
            self.shared.borrow_mut().state = WebSocketState::Terminated;
            self.flush_and_shutdown().await?;
            return Err(Error::ConnectionClosed);
        }

        self.flush_write_buf().await?;
        Ok(())
    }

    /// Sends any pending pong frames queued by the read half.
    async fn send_pending_pongs(&mut self) -> Result<()> {
        loop {
            let pong = self.shared.borrow_mut().pending_pongs.pop_front();
            match pong {
                Some(frame) => self.write_frame(frame).await?,
                None => break,
            }
        }
        Ok(())
    }

    async fn write_frame(&mut self, mut frame: Frame) -> Result<()> {
        let state = self.shared.borrow().state;
        if self.role == Role::Client {
            frame.set_random_mask();
        }

        if self.write_buf.len() > self.write_limit {
            self.flush_write_buf().await?;
        }

        FrameEncoder
            .encode(frame, &mut self.write_buf)
            .check_connection_reset(state)?;

        Ok(())
    }

    async fn flush_write_buf(&mut self) -> Result<()> {
        if self.write_buf.is_empty() {
            return Ok(());
        }

        let buf = std::mem::replace(&mut self.write_buf, BytesMut::new());
        let (res, buf) = self.writer.write_all(buf).await;
        self.write_buf = buf;
        res?;

        self.write_buf.clear();
        self.writer.flush().await?;
        Ok(())
    }

    async fn flush_and_shutdown(&mut self) -> Result<()> {
        self.flush_write_buf().await?;
        self.writer.shutdown().await?;
        Ok(())
    }
}

impl<W: AsyncWriteRent> Sink<Message> for WebSocketWriteHalf<W> {
    type Error = Error;

    async fn send(&mut self, item: Message) -> Result<(), Self::Error> {
        self.write(item).await
    }

    async fn flush(&mut self) -> Result<(), Self::Error> {
        match WebSocketWriteHalf::flush(self).await {
            Ok(()) | Err(Error::ConnectionClosed) => Ok(()),
            Err(e) => Err(e),
        }
    }

    async fn close(&mut self) -> Result<(), Self::Error> {
        match WebSocketWriteHalf::close(self, None).await {
            Ok(()) | Err(Error::ConnectionClosed) => Ok(()),
            Err(e) => Err(e),
        }
    }
}
