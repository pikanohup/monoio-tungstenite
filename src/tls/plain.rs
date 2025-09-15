use monoio::io::{AsyncReadRent, AsyncWriteRent};

use crate::{
    error::{Error, UrlError},
    stream::{MaybeTlsStream, Mode},
};

pub(super) async fn wrap_stream<S>(socket: S, mode: Mode) -> Result<MaybeTlsStream<S>, Error>
where
    S: AsyncReadRent + AsyncWriteRent,
{
    match mode {
        Mode::Plain => Ok(MaybeTlsStream::Plain(socket)),
        Mode::Tls => Err(Error::Url(UrlError::TlsFeatureNotEnabled)),
    }
}
