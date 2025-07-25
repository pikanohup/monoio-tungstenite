use monoio::io::{AsyncReadRent, AsyncWriteRent, sink::Sink, stream::Stream};

use crate::{
    error::{CapacityError, Error, ProtocolError, Result},
    protocol::{
        frame::{
            CloseFrame, Frame, Utf8Bytes,
            codec::{FrameCodec, FrameDecoder, FrameEncoder},
            coding::{CloseCode, Control as OpCtl, Data as OpData, OpCode},
        },
        message::{IncompleteMessage, IncompleteMessageType, Message},
    },
};

/// Indicates a Client or Server role of the websocket.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// This socket is a server.
    Server,
    /// This socket is a client.
    Client,
}

/// The configuration for WebSocket connection.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct WebSocketConfig {
    /// The initial capacity of the read buffer. This buffer is pre-allocated and can hold at least
    /// the specified number of bytes without requiring reallocation.
    ///
    /// The default value is 128 KiB.
    pub initial_read_capacity: usize,
    /// The target minimum size of the write buffer to reach before writing the data to the
    /// underlying stream.
    ///
    /// The default value is 128 KiB.
    ///
    /// If set to `0` each message will be eagerly written to the underlying stream. It is often
    /// more optimal to allow them to buffer a little, hence the default value.
    ///
    /// Note: [`flush`](crate::protocol::WebSocket::flush) will always fully write the buffer
    /// regardless.
    pub write_buffer_size: usize,
    /// The maximum size of an incoming message. `None` means no size limit.
    ///
    /// The default value is 64 MiB, which should be reasonably big for all normal use-cases but
    /// small enough to prevent memory eating by a malicious user.
    pub max_message_size: Option<usize>,
    /// The maximum size of a single incoming message frame.
    ///
    ///  `None` means no size limit. The limit is for frame payload NOT including the frame header.
    ///
    /// The default value is 16 MiB, which should be reasonably big for all normal use-cases but
    /// small enough to prevent memory eating by a malicious user.
    pub max_frame_size: Option<usize>,
    /// When set to `true`, the server will accept and handle unmasked frames
    /// from the client.
    ///
    /// According to the RFC 6455, the server must close the connection to the client in such
    /// cases, however it seems like there are some popular libraries that are sending unmasked
    /// frames, ignoring the RFC. By default this option is set to `false`, i.e. according to
    /// RFC 6455.
    pub accept_unmasked_frames: bool,
}

impl Default for WebSocketConfig {
    fn default() -> Self {
        Self {
            initial_read_capacity: 128 * 1024,
            write_buffer_size: 128 * 1024,
            max_message_size: Some(64 << 20),
            max_frame_size: Some(16 << 20),
            accept_unmasked_frames: false,
        }
    }
}

impl WebSocketConfig {
    /// Sets [`Self::initial_read_capacity`].
    pub fn initial_read_capacity(mut self, initial_read_capacity: usize) -> Self {
        self.initial_read_capacity = initial_read_capacity;
        self
    }

    /// Sets [`Self::write_buffer_size`].
    pub fn write_buffer_size(mut self, write_buffer_size: usize) -> Self {
        self.write_buffer_size = write_buffer_size;
        self
    }

    /// Sets [`Self::max_message_size`].
    pub fn max_message_size(mut self, max_message_size: Option<usize>) -> Self {
        self.max_message_size = max_message_size;
        self
    }

    /// Sets [`Self::max_frame_size`].
    pub fn max_frame_size(mut self, max_frame_size: Option<usize>) -> Self {
        self.max_frame_size = max_frame_size;
        self
    }

    /// Sets [`Self::accept_unmasked_frames`].
    pub fn accept_unmasked_frames(mut self, accept_unmasked_frames: bool) -> Self {
        self.accept_unmasked_frames = accept_unmasked_frames;
        self
    }
}

/// WebSocket input-output stream.
///
/// This is THE structure you want to create to be able to speak the WebSocket protocol.
/// It may be created by calling `connect`, `accept` or `client` functions.
///
/// Use [`WebSocket::read`], [`WebSocket::write`] to received and send messages.
///
/// This structure also implements [`Stream`] and [`Sink`].
#[derive(Debug)]
pub struct WebSocket<S>
where
    S: AsyncWriteRent + AsyncReadRent,
{
    /// Server or client?
    role: Role,
    /// encoder/decoder of frame.
    frame_codec: FrameCodec<S>,
    /// The state of processing, either "active" or "closing".
    state: WebSocketState,
    /// Receive: an incomplete message being processed.
    incomplete: Option<IncompleteMessage>,
    /// The configuration for the websocket session.
    config: WebSocketConfig,
}

impl<S> WebSocket<S>
where
    S: AsyncWriteRent + AsyncReadRent,
{
    /// Converts a raw socket into a WebSocket without performing a handshake.
    pub fn from_raw_socket(stream: S, role: Role, config: Option<WebSocketConfig>) -> Self {
        let config = config.unwrap_or_default();

        let frame_codec = FrameCodec::new(
            stream,
            FrameDecoder::new(
                config.max_frame_size,
                matches!(role, Role::Server),
                config.accept_unmasked_frames,
            ),
            FrameEncoder,
            config.initial_read_capacity,
            config.write_buffer_size,
        );

        Self {
            role,
            frame_codec,
            state: WebSocketState::Active,
            incomplete: None,
            config,
        }
    }

    /// Creates a [`WebSocket`] from an existing [`FrameCodec`].
    ///
    /// This is typically used after a successful handshake, allowing to
    /// reuse the frame codec that was already used for the handshake process.
    //
    // todo: pub api, +setup frame_codec
    #[cfg(feature = "handshake")]
    pub(crate) fn from_existing_frame_codec(
        frame_codec: FrameCodec<S>,
        role: Role,
        config: WebSocketConfig,
    ) -> Self {
        Self {
            role,
            frame_codec,
            state: WebSocketState::Active,
            incomplete: None,
            config,
        }
    }

    /// Changes the configuration.
    pub fn set_config(&mut self, set_func: impl FnOnce(&mut WebSocketConfig)) {
        set_func(&mut self.config);

        self.frame_codec
            .decoder_mut()
            .set_accept_unmasked(self.config.accept_unmasked_frames);
        self.frame_codec
            .decoder_mut()
            .set_max_frame_size(self.config.max_frame_size);
        self.frame_codec
            .framed_write_mut()
            .set_backpressure(self.config.write_buffer_size);
    }

    /// Reads the configuration.
    pub fn get_config(&self) -> &WebSocketConfig {
        &self.config
    }

    /// Checks if it is possible to read messages.
    ///
    /// Reading is impossible after receiving `Message::Close`. It is still possible after
    /// sending close frame since the peer still may send some data before confirming close.
    pub fn can_read(&self) -> bool {
        self.state.can_read()
    }

    /// Checks if it is possible to write messages.
    ///
    /// Writing gets impossible immediately after sending or receiving `Message::Close`.
    pub fn can_write(&self) -> bool {
        self.state.is_active()
    }

    /// Reads a message from the websocket.
    #[inline]
    pub async fn read(&mut self) -> Result<Message> {
        // Do not read from already closed connections.
        self.state.check_not_terminated()?;

        if self.role == Role::Server && !self.state.can_read() {
            self.state = WebSocketState::Terminated;
            return Err(Error::ConnectionClosed);
        }

        loop {
            let (msg, auto_send) = self.read_message_frame().await?;
            if let Some(auto_send) = auto_send {
                self.write_frame(auto_send).await?;
                // self.flush().await?;
                self.frame_codec.flush().await?;
            }

            if let Some(msg) = msg {
                return Ok(msg);
            }
        }
    }

    /// Writes a message into the websocket.
    ///
    /// Does **not** flush.
    #[inline]
    pub async fn write(&mut self, message: Message) -> Result<()> {
        // When terminated, return AlreadyClosed.
        self.state.check_not_terminated()?;

        // Do not write after sending a close frame.
        if !self.state.is_active() {
            return Err(Error::Protocol(ProtocolError::SendAfterClosing));
        }

        let frame = match message {
            Message::Text(data) => Frame::message(data, OpCode::Data(OpData::Text), true),
            Message::Binary(data) => Frame::message(data, OpCode::Data(OpData::Binary), true),
            Message::Ping(data) => Frame::ping(data),
            Message::Pong(data) => Frame::pong(data),
            Message::Close(code) => return self.close(code).await,
            Message::Frame(f) => f,
        };

        self.write_frame(frame).await?;
        Ok(())
    }

    /// Closes the connection.
    ///
    /// This function guarantees that the close frame will be queued.
    /// There is no need to call it again. Calling this function is
    /// the same as calling `send(Message::Close(..))`.
    pub async fn close(&mut self, code: Option<CloseFrame>) -> Result<()> {
        if let WebSocketState::Active = self.state {
            self.state = WebSocketState::ClosedByUs;
            let frame = Frame::close(code);
            self.write_frame(frame).await?;
        }

        self.flush().await
    }

    /// Writes a single frame into the write-buffer.
    async fn write_frame(&mut self, mut frame: Frame) -> Result<()> {
        if self.role == Role::Client {
            // 5. If the data is being sent by the client, the frame(s) MUST be
            // masked as defined in Section 5.3. (RFC 6455)
            frame.set_random_mask();
        }

        self.frame_codec
            .send(frame)
            .await
            .check_connection_reset(self.state)?;

        Ok(())
    }

    /// Flushes the writes.
    ///
    /// Ensures all messages previously passed to [`write`](Self::write) and automatically
    /// queued pong responses are written & flushed into the `stream`.
    #[inline]
    pub async fn flush(&mut self) -> Result<()> {
        // If we're closing and there is nothing to send anymore, we should close the connection.
        if self.role == Role::Server && !self.state.can_read() {
            // The underlying TCP connection, in most normal cases, SHOULD be closed
            // first by the server, so that it holds the TIME_WAIT state and not the
            // client (as this would prevent it from re-opening the connection for 2
            // maximum segment lifetimes (2MSL), while there is no corresponding
            // server impact as a TIME_WAIT connection is immediately reopened upon
            // a new SYN with a higher seq number). (RFC 6455)
            self.state = WebSocketState::Terminated;
            self.frame_codec.close().await?;
            return Err(Error::ConnectionClosed);
        }

        self.frame_codec.flush().await?;
        Ok(())
    }

    async fn read_message_frame(&mut self) -> Result<(Option<Message>, Option<Frame>)> {
        match self
            .frame_codec
            .next()
            .await
            .transpose()
            .check_connection_reset(self.state)?
        {
            Some(frame) => {
                if !self.state.can_read() {
                    return Err(Error::Protocol(ProtocolError::ReceivedAfterClosing));
                }

                // MUST be 0 unless an extension is negotiated that defines meanings
                // for non-zero values.  If a nonzero value is received and none of
                // the negotiated extensions defines the meaning of such a nonzero
                // value, the receiving endpoint MUST _Fail the WebSocket
                // Connection_.
                {
                    let hdr = frame.header();
                    if hdr.rsv1 || hdr.rsv2 || hdr.rsv3 {
                        return Err(Error::Protocol(ProtocolError::NonZeroReservedBits));
                    }
                }

                if self.role == Role::Client && frame.is_masked() {
                    // A client MUST close a connection if it detects a masked frame. (RFC 6455)
                    return Err(Error::Protocol(ProtocolError::MaskedFrameFromServer));
                }

                self.handle_frame(frame)
            }

            // Connection closed by peer
            None => match std::mem::replace(&mut self.state, WebSocketState::Terminated) {
                WebSocketState::ClosedByPeer | WebSocketState::CloseAcknowledged => {
                    Err(Error::ConnectionClosed)
                }
                _ => Err(Error::Protocol(ProtocolError::ResetWithoutClosingHandshake)),
            },
        }
    }

    fn handle_frame(&mut self, frame: Frame) -> Result<(Option<Message>, Option<Frame>)> {
        match frame.header().opcode {
            OpCode::Control(ctl) => {
                match ctl {
                    // All control frames MUST have a payload length of 125 bytes or less
                    // and MUST NOT be fragmented. (RFC 6455)
                    _ if !frame.header().is_final => {
                        Err(Error::Protocol(ProtocolError::FragmentedControlFrame))
                    }

                    _ if frame.payload().len() > 125 => {
                        Err(Error::Protocol(ProtocolError::ControlFrameTooBig))
                    }

                    OpCtl::Close => {
                        let (msg, reply) = self.do_close(frame.into_close()?);
                        Ok((msg.map(Message::Close), reply))
                    }

                    OpCtl::Reserved(i) => {
                        Err(Error::Protocol(ProtocolError::UnknownControlFrameType(i)))
                    }

                    OpCtl::Ping => {
                        let data = frame.into_payload();
                        // No ping processing after we sent a close frame.
                        let reply = self.state.is_active().then(|| Frame::pong(data.clone()));
                        Ok((Some(Message::Ping(data)), reply))
                    }

                    OpCtl::Pong => Ok((Some(Message::Pong(frame.into_payload())), None)),
                }
            }

            OpCode::Data(data) => {
                let fin = frame.header().is_final;

                let msg = match data {
                    OpData::Continue => {
                        if let Some(ref mut msg) = self.incomplete {
                            msg.extend(frame.into_payload(), self.config.max_message_size)?;
                        } else {
                            return Err(Error::Protocol(ProtocolError::UnexpectedContinueFrame));
                        }

                        if fin {
                            Ok(Some(self.incomplete.take().unwrap().complete()?))
                        } else {
                            Ok(None)
                        }
                    }

                    c if self.incomplete.is_some() => {
                        Err(Error::Protocol(ProtocolError::ExpectedFragment(c)))
                    }

                    OpData::Text if fin => {
                        check_max_size(frame.payload().len(), self.config.max_message_size)?;
                        Ok(Some(Message::Text(frame.into_text()?)))
                    }

                    OpData::Binary if fin => {
                        check_max_size(frame.payload().len(), self.config.max_message_size)?;
                        Ok(Some(Message::Binary(frame.into_payload())))
                    }

                    OpData::Text | OpData::Binary => {
                        let message_type = match data {
                            OpData::Text => IncompleteMessageType::Text,
                            OpData::Binary => IncompleteMessageType::Binary,
                            _ => panic!("Bug: message is not text nor binary"),
                        };

                        let mut incomplete = IncompleteMessage::new(message_type);
                        incomplete.extend(frame.into_payload(), self.config.max_message_size)?;
                        self.incomplete = Some(incomplete);

                        Ok(None)
                    }

                    OpData::Reserved(i) => {
                        Err(Error::Protocol(ProtocolError::UnknownDataFrameType(i)))
                    }
                }?;

                Ok((msg, None))
            }
        }
    }

    /// Handles the reception of a close frame and determines the appropriate response.
    ///
    /// Returns:
    /// - An optional close frame to be returned to the user.
    /// - An optional reply frame to be sent back to the peer.
    fn do_close(
        &mut self,
        close: Option<CloseFrame>,
    ) -> (Option<Option<CloseFrame>>, Option<Frame>) {
        match self.state {
            WebSocketState::Active => {
                self.state = WebSocketState::ClosedByPeer;

                let close = close.map(|frame| {
                    if !frame.code.is_allowed() {
                        CloseFrame {
                            code: CloseCode::Protocol,
                            reason: Utf8Bytes::from_static("Protocol violation"),
                        }
                    } else {
                        frame
                    }
                });

                let reply = Frame::close(close.clone());
                (Some(close), Some(reply))
            }

            WebSocketState::ClosedByPeer | WebSocketState::CloseAcknowledged => {
                // It is already closed, just ignore.
                (None, None)
            }

            WebSocketState::ClosedByUs => {
                // We received a reply.
                self.state = WebSocketState::CloseAcknowledged;
                (Some(close), None)
            }

            WebSocketState::Terminated => unreachable!(),
        }
    }
}

impl<S> Stream for WebSocket<S>
where
    S: AsyncWriteRent + AsyncReadRent,
{
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

impl<S> Sink<Message> for WebSocket<S>
where
    S: AsyncWriteRent + AsyncReadRent,
{
    type Error = Error;

    async fn send(&mut self, item: Message) -> Result<(), Self::Error> {
        self.write(item).await
    }

    async fn flush(&mut self) -> Result<(), Self::Error> {
        match self.flush().await {
            Ok(()) | Err(Error::ConnectionClosed) => Ok(()),
            Err(e) => Err(e),
        }
    }

    async fn close(&mut self) -> Result<(), Self::Error> {
        match self.close(None).await {
            Ok(()) | Err(Error::ConnectionClosed) => Ok(()),
            Err(e) => Err(e),
        }
    }
}

#[inline]
fn check_max_size(size: usize, max_size: Option<usize>) -> Result<()> {
    if let Some(max_size) = max_size
        && size > max_size
    {
        return Err(Error::Capacity(CapacityError::MessageTooLong {
            size,
            max_size,
        }));
    }

    Ok(())
}

/// Translates "Connection reset by peer" into `ConnectionClosed` if appropriate.
trait CheckConnectionReset {
    fn check_connection_reset(self, state: WebSocketState) -> Self;
}

impl<T> CheckConnectionReset for Result<T> {
    fn check_connection_reset(self, state: WebSocketState) -> Self {
        match self {
            Err(Error::Io(io_error)) => Err({
                if !state.can_read() && io_error.kind() == std::io::ErrorKind::ConnectionReset {
                    Error::ConnectionClosed
                } else {
                    Error::Io(io_error)
                }
            }),
            x => x,
        }
    }
}

/// The current connection state.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum WebSocketState {
    /// The connection is active.
    Active,
    /// We initiated a close handshake.
    ClosedByUs,
    /// The peer initiated a close handshake.
    ClosedByPeer,
    /// The peer replied to our close handshake.
    CloseAcknowledged,
    /// The connection does not exist anymore.
    Terminated,
}

impl WebSocketState {
    /// Tells if we're allowed to process normal messages.
    fn is_active(self) -> bool {
        matches!(self, WebSocketState::Active)
    }

    /// Tells if we should process incoming data. Note that if we send a close frame
    /// but the remote hasn't confirmed, they might have sent data before they receive our
    /// close frame, so we should still pass those to client code, hence ClosedByUs is valid.
    fn can_read(self) -> bool {
        matches!(self, WebSocketState::Active | WebSocketState::ClosedByUs)
    }

    /// Checks if the state is active, return error if not.
    fn check_not_terminated(self) -> Result<()> {
        match self {
            WebSocketState::Terminated => Err(Error::AlreadyClosed),
            _ => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use monoio::{
        BufResult,
        buf::{IoBuf, IoBufMut, IoVecBuf, IoVecBufMut},
    };

    use super::*;

    struct MockWrite<S>(S);

    impl<S> AsyncWriteRent for MockWrite<S> {
        async fn write<T: IoBuf>(&mut self, buf: T) -> BufResult<usize, T> {
            (Ok(buf.bytes_init()), buf)
        }

        async fn writev<T: IoVecBuf>(&mut self, buf_vec: T) -> BufResult<usize, T> {
            (Ok(buf_vec.read_iovec_len()), buf_vec)
        }

        async fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }

        async fn shutdown(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl<S: AsyncReadRent> AsyncReadRent for MockWrite<S> {
        async fn read<T: IoBufMut>(&mut self, buf: T) -> BufResult<usize, T> {
            self.0.read(buf).await
        }

        async fn readv<T: IoVecBufMut>(&mut self, buf: T) -> BufResult<usize, T> {
            self.0.readv(buf).await
        }
    }

    #[monoio::test]
    async fn receive_messages() {
        let incoming = [
            0x89, 0x02, 0x01, 0x02, 0x8a, 0x01, 0x03, 0x01, 0x07, 0x48, 0x65, 0x6c, 0x6c, 0x6f,
            0x2c, 0x20, 0x80, 0x06, 0x57, 0x6f, 0x72, 0x6c, 0x64, 0x21, 0x82, 0x03, 0x01, 0x02,
            0x03,
        ];
        let mut socket = WebSocket::from_raw_socket(MockWrite(&incoming[..]), Role::Client, None);

        assert_eq!(
            socket.read().await.unwrap(),
            Message::Ping(vec![1, 2].into())
        );
        assert_eq!(socket.read().await.unwrap(), Message::Pong(vec![3].into()));
        assert_eq!(
            socket.read().await.unwrap(),
            Message::Text("Hello, World!".into())
        );
        assert_eq!(
            socket.read().await.unwrap(),
            Message::Binary(vec![0x01, 0x02, 0x03].into())
        );
    }

    #[monoio::test]
    async fn size_limiting_text_fragmented() {
        let incoming = [
            0x01, 0x07, 0x48, 0x65, 0x6c, 0x6c, 0x6f, 0x2c, 0x20, 0x80, 0x06, 0x57, 0x6f, 0x72,
            0x6c, 0x64, 0x21,
        ];
        let limit = WebSocketConfig {
            max_message_size: Some(10),
            ..WebSocketConfig::default()
        };
        let mut socket =
            WebSocket::from_raw_socket(MockWrite(&incoming[..]), Role::Client, Some(limit));

        assert!(matches!(
            socket.read().await,
            Err(Error::Capacity(CapacityError::MessageTooLong {
                size: 13,
                max_size: 10
            }))
        ));
    }

    #[monoio::test]
    async fn size_limiting_binary() {
        let incoming = [0x82, 0x03, 0x01, 0x02, 0x03];
        let limit = WebSocketConfig {
            max_message_size: Some(2),
            ..WebSocketConfig::default()
        };
        let mut socket =
            WebSocket::from_raw_socket(MockWrite(&incoming[..]), Role::Client, Some(limit));

        assert!(matches!(
            socket.read().await,
            Err(Error::Capacity(CapacityError::MessageTooLong {
                size: 3,
                max_size: 2
            }))
        ));
    }
}
