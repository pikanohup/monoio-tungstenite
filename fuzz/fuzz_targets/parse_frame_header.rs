#![no_main]

use std::io::Cursor;

use libfuzzer_sys::fuzz_target;
use monoio_tungstenite::protocol::frame::FrameHeader;

fuzz_target!(|data: &[u8]| {
    let mut cursor = Cursor::new(data);
    FrameHeader::parse(&mut cursor).ok();
});
