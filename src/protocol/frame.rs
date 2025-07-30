//! Utilities to work with raw WebSocket frames.

pub mod coding;

pub mod codec;
#[allow(clippy::module_inception)]
mod frame;
mod mask;
pub(crate) mod utf8;

pub use frame::{CloseFrame, Frame, FrameHeader};
pub use utf8::Utf8Bytes;
