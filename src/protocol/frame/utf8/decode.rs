// UTF-8 parsing and validation for WebSocket messages, modified from https://github.com/SimonSapin/rust-utf8/blob/218fea2b57b0e4c3de9fa17a376fcc4a4c0d08f3/src/lib.rs

#[derive(Debug, Copy, Clone)]
pub(crate) struct Incomplete {
    buffer: [u8; 4],
    buffer_len: u8,
}

impl Incomplete {
    /// Creates a new `Incomplete`.
    pub const fn new() -> Self {
        Self {
            buffer: [0; 4],
            buffer_len: 0,
        }
    }

    /// Creates a new `Incomplete` from the given bytes.
    #[inline]
    pub fn from_bytes(bytes: &[u8]) -> Self {
        let mut buffer = [0, 0, 0, 0];
        let len = bytes.len();
        buffer[..len].copy_from_slice(bytes);

        Incomplete {
            buffer,
            buffer_len: len as u8,
        }
    }

    /// Returns the current buffer length.
    #[inline]
    pub fn len(&self) -> usize {
        self.buffer_len as usize
    }

    /// Returns true if the buffer length is zero.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.buffer_len == 0
    }

    /// * `None`: still incomplete, call `try_complete` again with more input. If no more input is
    ///   available, this is invalid byte sequence.
    /// * `Some((result, remaining_input))`: We’re done with this `Incomplete`. To keep decoding,
    ///   pass `remaining_input` to `decode()`.
    #[allow(clippy::type_complexity)]
    #[inline]
    pub fn try_complete<'input>(
        &mut self,
        input: &'input [u8],
    ) -> Option<(Result<&str, &[u8]>, &'input [u8])> {
        let (consumed, opt_result) = self.try_complete_offsets(input);
        let result = opt_result?;
        let remaining_input = &input[consumed..];
        let result_bytes = self.take_buffer();
        let result = match result {
            Ok(()) => Ok(unsafe { str::from_utf8_unchecked(result_bytes) }),
            Err(()) => Err(result_bytes),
        };
        Some((result, remaining_input))
    }

    #[inline]
    fn take_buffer(&mut self) -> &[u8] {
        let len = self.buffer_len as usize;
        self.buffer_len = 0;
        &self.buffer[..len]
    }

    /// (consumed_from_input, None): not enough input
    /// (consumed_from_input, Some(Err(()))): error bytes in buffer
    /// (consumed_from_input, Some(Ok(()))): UTF-8 string in buffer
    #[inline]
    fn try_complete_offsets(&mut self, input: &[u8]) -> (usize, Option<Result<(), ()>>) {
        let initial_buffer_len = self.buffer_len as usize;

        let copied_from_input;
        {
            let unwritten = &mut self.buffer[initial_buffer_len..];
            copied_from_input = input.len().min(unwritten.len());
            unwritten[..copied_from_input].copy_from_slice(&input[..copied_from_input]);
        }

        let spliced = &self.buffer[..initial_buffer_len + copied_from_input];
        match simdutf8::compat::from_utf8(spliced) {
            Ok(_) => {
                self.buffer_len = spliced.len() as u8;
                (copied_from_input, Some(Ok(())))
            }

            Err(error) => {
                let valid_up_to = error.valid_up_to();
                if valid_up_to > 0 {
                    let consumed = valid_up_to.checked_sub(initial_buffer_len).unwrap();
                    self.buffer_len = valid_up_to as u8;
                    (consumed, Some(Ok(())))
                } else {
                    match error.error_len() {
                        Some(invalid_sequence_length) => {
                            let consumed = invalid_sequence_length
                                .checked_sub(initial_buffer_len)
                                .unwrap();
                            self.buffer_len = invalid_sequence_length as u8;
                            (consumed, Some(Err(())))
                        }

                        None => {
                            self.buffer_len = spliced.len() as u8;
                            (copied_from_input, None)
                        }
                    }
                }
            }
        }
    }
}
