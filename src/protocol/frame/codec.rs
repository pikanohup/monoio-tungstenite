//! WebSocket frame codec implementation using [`monoio_codec`](https://crates.io/crates/monoio-codec).

use std::io::Cursor;

use bytes::{Buf, BytesMut};
use monoio_codec::{Decoded, Decoder, Encoder};

use crate::{
    error::{CapacityError, Error, ProtocolError},
    framed::Framed,
    protocol::frame::{Frame, FrameHeader, frame::LengthFormat, mask::apply_mask},
};

/// Codec for WebSocket frames.
pub type FrameCodec<IO> = Framed<IO, FrameDecoder, FrameEncoder>;

/// Decoder for WebSocket frames.
#[derive(Debug, Clone, Default)]
pub struct FrameDecoder {
    header: Option<(FrameHeader, usize)>,
    max_frame_size: Option<usize>,
    unmask: bool,
    accept_unmasked: bool,
}

impl FrameDecoder {
    /// Creates a new `FrameDecoder`.
    pub fn new(max_size: Option<usize>, unmask: bool, accept_unmasked: bool) -> Self {
        Self {
            header: None,
            max_frame_size: max_size,
            unmask,
            accept_unmasked,
        }
    }

    /// Sets whether to unmask frames.
    pub fn set_unmask(&mut self, unmask: bool) {
        self.unmask = unmask;
    }

    /// Sets the maximum frame size.
    pub fn set_max_frame_size(&mut self, max_size: Option<usize>) {
        self.max_frame_size = max_size;
    }

    /// Sets whether to accept unmasked frames.
    pub fn set_accept_unmasked(&mut self, accept: bool) {
        self.accept_unmasked = accept;
    }
}

impl Decoder for FrameDecoder {
    type Item = Frame;
    type Error = Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Decoded<Self::Item>, Self::Error> {
        if self.header.is_none() {
            let mut cursor = Cursor::new(&mut *src);

            match FrameHeader::parse(&mut cursor)? {
                Some((header, len)) => {
                    let advanced = cursor.position();
                    src.advance(advanced as _);

                    let len = len as usize;
                    // Enforce frame size limit early
                    if let Some(max_size) = self.max_frame_size
                        && len > max_size
                    {
                        return Err(Error::Capacity(CapacityError::MessageTooLong {
                            size: len,
                            max_size,
                        }));
                    }

                    src.reserve(len);
                    self.header = Some((header, len));
                }

                None => {
                    src.reserve(FrameHeader::MAX_SIZE);
                    return Ok(Decoded::Insufficient);
                }
            };
        }

        let (_, len) = self.header.as_ref().unwrap();
        let len = *len;
        if src.len() < len {
            return Ok(Decoded::InsufficientAtLeast(len));
        }
        let mut payload = src.split_to(len);

        let (mut header, len) = self.header.take().unwrap();
        debug_assert_eq!(payload.len(), len);

        if self.unmask {
            if let Some(mask) = header.mask.take() {
                // A server MUST remove masking for data frames received from a client
                // as described in Section 5.3. (RFC 6455)
                apply_mask(&mut payload, mask);
            } else if !self.accept_unmasked {
                // The server MUST close the connection upon receiving a
                // frame that is not masked. (RFC 6455)
                // The only exception here is if the user explicitly accepts given
                // stream by setting WebSocketConfig.accept_unmasked_frames to true
                return Err(Error::Protocol(ProtocolError::UnmaskedFrameFromClient));
            }
        }

        let frame = Frame::from_payload(header, payload.freeze());
        Ok(Decoded::Some(frame))
    }
}

/// Encoder for WebSocket frames.
#[derive(Debug, Clone, Copy, Default)]
pub struct FrameEncoder;

impl FrameEncoder {
    #[inline]
    fn write_header(header: &FrameHeader, length: u64, dst: &mut BytesMut) {
        let code: u8 = header.opcode.into();

        let one = {
            code | if header.is_final { 0x80 } else { 0 }
                | if header.rsv1 { 0x40 } else { 0 }
                | if header.rsv2 { 0x20 } else { 0 }
                | if header.rsv3 { 0x10 } else { 0 }
        };

        let lenfmt = LengthFormat::for_length(length);

        let two = { lenfmt.length_byte() | if header.mask.is_some() { 0x80 } else { 0 } };

        dst.extend_from_slice(&[one, two]);
        match lenfmt {
            LengthFormat::U8(_) => (),
            LengthFormat::U16 => {
                dst.extend_from_slice(&(length as u16).to_be_bytes());
            }
            LengthFormat::U64 => {
                dst.extend_from_slice(&length.to_be_bytes());
            }
        }

        if let Some(ref mask) = header.mask {
            dst.extend_from_slice(mask);
        }
    }
}

impl Encoder<Frame> for FrameEncoder {
    type Error = Error;

    fn encode(&mut self, mut frame: Frame, dst: &mut BytesMut) -> Result<(), Self::Error> {
        dst.reserve(frame.len());

        FrameEncoder::write_header(&frame.header, frame.payload.len() as u64, dst);

        let len = dst.len();
        dst.extend_from_slice(&frame.payload);
        if let Some(mask) = frame.header.mask.take() {
            apply_mask(&mut dst[len..], mask);
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decode_frame() {
        let mut data = BytesMut::from(&[0x82, 0x07, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07][..]);
        let mut decoder = FrameDecoder::default();
        let frame = decoder.decode(&mut data).unwrap().unwrap();
        assert!(frame.header().is_final);
        assert_eq!(
            frame.into_payload(),
            &[0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07][..]
        );
        assert!(data.is_empty());
    }

    #[test]
    fn test_decode_frame_with_max_size() {
        let mut data = BytesMut::from(&[0x81, 0x05, 0x48, 0x65, 0x6c, 0x6c, 0x6f][..]);
        let mut decoder = FrameDecoder {
            max_frame_size: Some(1),
            ..Default::default()
        };
        let res = decoder.decode(&mut data);
        assert!(matches!(
            res.unwrap_err(),
            Error::Capacity(CapacityError::MessageTooLong {
                size: 5,
                max_size: 1
            })
        ));
    }

    #[test]
    fn test_decode_frame_insufficient() {
        let mut data = BytesMut::from(&[0x81][..]);
        let mut decoder = FrameDecoder::default();
        let res = decoder.decode(&mut data);
        assert!(matches!(res.unwrap(), Decoded::Insufficient));
        assert_eq!(data.len(), 1);

        let mut data = BytesMut::from(&[0x81, 0x05, 0x48, 0x65][..]);
        let mut decoder = FrameDecoder::default();
        let res = decoder.decode(&mut data);
        assert!(matches!(res.unwrap(), Decoded::InsufficientAtLeast(5)));
        assert_eq!(data.len(), 2);
    }

    #[test]
    fn test_encode_frame() {
        let mut buf = BytesMut::new();
        let frame = Frame::ping(vec![0x01, 0x02]);
        FrameEncoder.encode(frame, &mut buf).unwrap();
        assert_eq!(buf, vec![0x89, 0x02, 0x01, 0x02]);
    }
}
