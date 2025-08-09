use bytes::BytesMut;
use monoio_codec::Encoder;

use crate::{
    error::Error,
    protocol::frame::{Frame, FrameHeader, frame::LengthFormat, mask::apply_mask},
};

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
    fn test_encode_frame() {
        let mut buf = BytesMut::new();
        let frame = Frame::ping(vec![0x01, 0x02]);
        FrameEncoder.encode(frame, &mut buf).unwrap();
        assert_eq!(buf, vec![0x89, 0x02, 0x01, 0x02]);
    }
}
