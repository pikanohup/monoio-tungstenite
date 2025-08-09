//! Server handshake.

use std::io::Write as _;

use bytes::BytesMut;
use http::{
    HeaderMap, Request as HttpRequest, Response as HttpResponse, StatusCode, response::Builder,
};
use httparse::Status;
use monoio::io::{AsyncReadRent, AsyncWriteRent, AsyncWriteRentExt, stream::Stream};
use monoio_codec::{Decoded, Decoder, FramedRead};

use super::{
    derive_accept_key,
    headers::{FromHttparse, MAX_HEADERS},
};
use crate::{
    error::{Error, ProtocolError, Result},
    protocol::{Role, WebSocket, WebSocketConfig},
};

/// Server request type.
pub type Request = HttpRequest<()>;

/// Server response type.
pub type Response = HttpResponse<()>;

/// Server error response type.
pub type ErrorResponse = HttpResponse<Option<String>>;

/// Performs a server handshake.
pub async fn server_handshake<S, C>(
    callback: C,
    stream: S,
    config: Option<WebSocketConfig>,
) -> Result<WebSocket<S>>
where
    S: AsyncReadRent + AsyncWriteRent,
    C: Callback,
{
    let mut framed = FramedRead::new(stream, RequestDecoder);

    match framed.next().await {
        Some(Ok((size, req))) => {
            if framed.read_buffer().len() != size {
                return Err(Error::Protocol(ProtocolError::JunkAfterRequest));
            }

            let resp = create_response(&req)?;
            match callback.on_request(&req, resp) {
                Ok(resp) => {
                    let buf = generate_response(&resp)?;
                    let (res, _) = framed.get_mut().write_all(buf).await;
                    res?;
                    framed.get_mut().flush().await?;

                    framed.read_buffer_mut().clear();
                    Ok(WebSocket::from_framed_read(framed, Role::Server, config))
                }

                Err(resp) => {
                    if resp.status().is_success() {
                        return Err(Error::Protocol(ProtocolError::CustomResponseSuccessful));
                    }

                    let buf = generate_response(&resp)?;
                    let (res, _) = framed.get_mut().write_all(buf).await;
                    res?;

                    if let Some(body) = resp.body() {
                        let (res, _) = framed.get_mut().write_all(body.as_bytes().to_vec()).await;
                        res?;
                    }

                    framed.get_mut().flush().await?;

                    let (parts, body) = resp.into_parts();
                    let body = body.map(|b| b.as_bytes().to_vec());
                    Err(Error::Http(Box::new(http::Response::from_parts(
                        parts, body,
                    ))))
                }
            }
        }

        Some(Err(e)) => Err(e),

        None => Err(Error::Protocol(ProtocolError::HandshakeIncomplete)),
    }
}

fn create_parts<T>(request: &HttpRequest<T>) -> Result<Builder> {
    if request.method() != http::Method::GET {
        return Err(Error::Protocol(ProtocolError::WrongHttpMethod));
    }

    if request.version() < http::Version::HTTP_11 {
        return Err(Error::Protocol(ProtocolError::WrongHttpVersion));
    }

    if !request
        .headers()
        .get("Connection")
        .and_then(|h| h.to_str().ok())
        .map(|h| {
            h.split([' ', ','])
                .any(|p| p.eq_ignore_ascii_case("Upgrade"))
        })
        .unwrap_or(false)
    {
        return Err(Error::Protocol(
            ProtocolError::MissingConnectionUpgradeHeader,
        ));
    }

    if !request
        .headers()
        .get("Upgrade")
        .and_then(|h| h.to_str().ok())
        .map(|h| h.eq_ignore_ascii_case("websocket"))
        .unwrap_or(false)
    {
        return Err(Error::Protocol(
            ProtocolError::MissingUpgradeWebSocketHeader,
        ));
    }

    if !request
        .headers()
        .get("Sec-WebSocket-Version")
        .map(|h| h == "13")
        .unwrap_or(false)
    {
        return Err(Error::Protocol(
            ProtocolError::MissingSecWebSocketVersionHeader,
        ));
    }

    let key = request
        .headers()
        .get("Sec-WebSocket-Key")
        .ok_or(Error::Protocol(ProtocolError::MissingSecWebSocketKey))?;

    let builder = Response::builder()
        .status(StatusCode::SWITCHING_PROTOCOLS)
        .version(request.version())
        .header("Connection", "Upgrade")
        .header("Upgrade", "websocket")
        .header("Sec-WebSocket-Accept", derive_accept_key(key.as_bytes()));

    Ok(builder)
}

/// Creates a response for the request.
pub fn create_response(request: &Request) -> Result<Response> {
    Ok(create_parts(request)?.body(())?)
}

/// Creates a response for the request with a custom body.
pub fn create_response_with_body<T1, T2>(
    request: &HttpRequest<T1>,
    generate_body: impl FnOnce() -> T2,
) -> Result<HttpResponse<T2>> {
    Ok(create_parts(request)?.body(generate_body())?)
}

fn generate_response<T>(resp: &HttpResponse<T>) -> Result<Vec<u8>> {
    let mut buf = Vec::new();

    write!(
        buf,
        "{version:?} {status}\r\n",
        version = resp.version(),
        status = resp.status()
    )
    .unwrap();

    for (k, v) in resp.headers() {
        buf.extend_from_slice(k.as_ref());
        buf.extend_from_slice(b": ");
        buf.extend_from_slice(v.as_ref());
        buf.extend_from_slice(b"\r\n");
    }

    buf.extend_from_slice(b"\r\n");
    Ok(buf)
}

/// Decoder for Request.
#[derive(Debug, Clone, Copy, Default)]
pub struct RequestDecoder;

impl Decoder for RequestDecoder {
    type Item = (usize, Request);
    type Error = Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Decoded<Self::Item>, Self::Error> {
        let mut hbuffer = [httparse::EMPTY_HEADER; MAX_HEADERS];
        let mut req = httparse::Request::new(&mut hbuffer);

        Ok(match req.parse(src)? {
            Status::Partial => Decoded::Insufficient,
            Status::Complete(size) => Decoded::Some((size, Request::from_httparse(req)?)),
        })
    }
}

impl<'h, 'b: 'h> FromHttparse<httparse::Request<'h, 'b>> for Request {
    fn from_httparse(raw: httparse::Request<'h, 'b>) -> Result<Self> {
        if raw.method.expect("Bug: no method in header") != "GET" {
            return Err(Error::Protocol(ProtocolError::WrongHttpMethod));
        }

        if raw.version.expect("Bug: no HTTP version") < /*1.*/1 {
            return Err(Error::Protocol(ProtocolError::WrongHttpVersion));
        }

        let headers = HeaderMap::from_httparse(raw.headers)?;

        let mut request = Request::new(());
        *request.method_mut() = http::Method::GET;
        *request.headers_mut() = headers;
        *request.uri_mut() = raw.path.expect("Bug: no path in header").parse()?;
        // TODO: httparse only supports HTTP 0.9/1.0/1.1 but not HTTP 2.0
        // so the only valid value we could get in the response would be 1.1.
        *request.version_mut() = http::Version::HTTP_11;

        Ok(request)
    }
}

/// The callback trait.
///
/// The callback is called when the server receives an incoming WebSocket
/// handshake request from the client. Specifying a callback allows you to analyze incoming headers
/// and add additional headers to the response that server sends to the client and/or reject the
/// connection based on the incoming headers.
pub trait Callback: Sized {
    /// Called whenever the server read the request from the client and is ready to reply to it.
    /// May return additional reply headers.
    /// Returning an error resulting in rejecting the incoming connection.
    fn on_request(self, req: &Request, resp: Response) -> Result<Response, Box<ErrorResponse>>;
}

impl<F> Callback for F
where
    F: FnOnce(&Request, Response) -> Result<Response, Box<ErrorResponse>>,
{
    fn on_request(self, req: &Request, resp: Response) -> Result<Response, Box<ErrorResponse>> {
        self(req, resp)
    }
}

/// Stub for callback that does nothing.
#[derive(Clone, Copy, Debug)]
pub struct NoCallback;

impl Callback for NoCallback {
    fn on_request(self, _req: &Request, resp: Response) -> Result<Response, Box<ErrorResponse>> {
        Ok(resp)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_parsing() {
        const DATA: &[u8] = b"GET /script.ws HTTP/1.1\r\nHost: foo.com\r\n\r\n";
        let (_, req) = RequestDecoder
            .decode(&mut BytesMut::from(DATA))
            .unwrap()
            .unwrap();
        assert_eq!(req.uri().path(), "/script.ws");
        assert_eq!(req.headers().get("Host").unwrap(), &b"foo.com"[..]);
    }

    #[test]
    fn request_replying() {
        const DATA: &[u8] = b"\
            GET /script.ws HTTP/1.1\r\n\
            Host: foo.com\r\n\
            Connection: upgrade\r\n\
            Upgrade: websocket\r\n\
            Sec-WebSocket-Version: 13\r\n\
            Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\
            \r\n";
        let (_, req) = RequestDecoder
            .decode(&mut BytesMut::from(DATA))
            .unwrap()
            .unwrap();
        let response = create_response(&req).unwrap();

        assert_eq!(
            response.headers().get("Sec-WebSocket-Accept").unwrap(),
            b"s3pPLMBiTxaQ9kYGzzhZRbK+xOo=".as_ref()
        );
    }
}
