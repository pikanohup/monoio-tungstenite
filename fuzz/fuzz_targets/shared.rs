use std::cell::{LazyCell, RefCell};

use monoio::{
    BufResult, FusionRuntime, IoUringDriver, LegacyDriver,
    buf::{IoBuf, IoBufMut, IoVecBuf, IoVecBufMut},
    io::{AsyncReadRent, AsyncWriteRent},
};

pub struct MockWrite<S>(pub S);

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

thread_local! {
    // Initialize the runtime statically, to ensure the intentional leak within
    // driver creation (`Box::leak` in `monoio::driver::uring::IoUringDriver::new_with_entries`)
    // is not repeatedly flagged by LeakSanitizer on each fuzz iteration.
    pub static RUNTIME: LazyCell<RefCell<FusionRuntime<IoUringDriver, LegacyDriver>>> = LazyCell::new(|| {
    RefCell::new(monoio::RuntimeBuilder::<monoio::FusionDriver>::new()
        .build()
        .expect("Failed to create runtime"))
 });
}
