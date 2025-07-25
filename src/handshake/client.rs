//! Client handshake.

use std::fmt::Write;

use bytes::{Buf, BytesMut};
use http::{
    HeaderMap, Request as HttpRequest, Response as HttpResponse, StatusCode, header::HeaderName,
};
use httparse::Status;
use monoio::io::{AsyncReadRent, AsyncWriteRent, sink::Sink};
use monoio_codec::{Decoded, Decoder, Encoder};

use super::{
    derive_accept_key,
    headers::{FromHttparse, MAX_HEADERS},
};
use crate::{
    client::uri_mode,
    error::{Error, ProtocolError, Result, SubProtocolError, UrlError},
    protocol::{
        Role, WebSocket, WebSocketConfig,
        frame::codec::{FrameCodec, FrameDecoder, FrameEncoder},
    },
};

/// Client request type.
pub type Request = HttpRequest<()>;

/// Client response type.
pub type Response = HttpResponse<Option<Vec<u8>>>;

/// Performs a client handshake.
pub async fn client_handshake<S>(
    request: Request,
    config: Option<WebSocketConfig>,
    stream: S,
) -> Result<(WebSocket<S>, Response)>
where
    S: AsyncReadRent + AsyncWriteRent,
{
    if request.method() != http::Method::GET {
        return Err(Error::Protocol(ProtocolError::WrongHttpMethod));
    }

    if request.version() < http::Version::HTTP_11 {
        return Err(Error::Protocol(ProtocolError::WrongHttpVersion));
    }

    // Check the URI scheme: only ws or wss are supported
    let _ = uri_mode(request.uri())?;

    let subprotocols = extract_subprotocols_from_request(&request)?;
    let key = get_websocket_key(&request)?;
    let accept_key = derive_accept_key(key.as_ref());
    let verify_data = VerifyData {
        accept_key,
        subprotocols,
    };

    let config = config.unwrap_or_default();
    let mut framed = FrameCodec::new(
        stream,
        FrameDecoder::new(config.max_frame_size, false, config.accept_unmasked_frames),
        FrameEncoder,
        config.initial_read_capacity,
        config.write_buffer_size,
    );

    framed.send_with(&mut RequestEncoder, request).await?;
    framed.flush().await?;

    match framed.next_with(&mut ResponseDecoder).await {
        Some(Ok((size, resp))) => {
            framed.read_buffer_mut().advance(size);
            let tail = framed.read_buffer_mut().to_vec();

            let resp = match verify_data.verify_response(resp) {
                Ok(r) => r,
                Err(Error::Http(mut e)) => {
                    *e.body_mut() = Some(tail);
                    return Err(Error::Http(e));
                }
                Err(e) => return Err(e),
            };

            let ws = WebSocket::from_existing_frame_codec(framed, Role::Client, config);
            Ok((ws, resp))
        }

        Some(Err(e)) => Err(e),

        None => Err(Error::Protocol(ProtocolError::HandshakeIncomplete)),
    }
}

// Headers that must be present in a correct request.
const KEY_HEADERNAME: &str = "Sec-WebSocket-Key";
const WEBSOCKET_HEADERS: [&str; 5] = [
    "Host",
    "Connection",
    "Upgrade",
    "Sec-WebSocket-Version",
    KEY_HEADERNAME,
];

/// Encoder for client request.
#[derive(Debug, Clone, Copy, Default)]
pub struct RequestEncoder;

impl Encoder<Request> for RequestEncoder {
    type Error = Error;

    fn encode(&mut self, mut req: Request, dst: &mut BytesMut) -> Result<(), Self::Error> {
        // XXX
        dst.reserve(256 + req.headers().len() * 35);

        write!(
            dst,
            "GET {path} {version:?}\r\n",
            path = req
                .uri()
                .path_and_query()
                .ok_or(Error::Url(UrlError::NoPathOrQuery))?
                .as_str(),
            version = req.version()
        )
        .unwrap();

        // We must check that all necessary headers for a valid request are present. Note that we
        // have to deal with the fact that some apps seem to have a case-sensitive check for
        // headers which is not correct and should not considered the correct behavior, but
        // it seems like some apps ignore it. `http` by default writes all headers in
        // lower-case which is fine (and does not violate the RFC) but some servers seem to
        // be poorely written and ignore RFC.
        //
        // See similar problem in `hyper`: https://github.com/hyperium/hyper/issues/1492
        let headers = req.headers_mut();
        for &header in &WEBSOCKET_HEADERS {
            let value = headers.remove(header).ok_or_else(|| {
                Error::Protocol(ProtocolError::InvalidHeader(
                    HeaderName::from_bytes(header.as_bytes()).unwrap(),
                ))
            })?;

            let value = value.to_str().map_err(|err| {
                Error::Utf8(format!(
                    "{err} for header name '{header}' with value: {value:?}"
                ))
            })?;

            dst.extend_from_slice(header.as_ref());
            dst.extend_from_slice(b": ");
            dst.extend_from_slice(value.as_bytes());
            dst.extend_from_slice(b"\r\n");
        }

        // Now we must ensure that the headers that we've written once are not anymore present in
        // the map. If they do, then the request is invalid (some headers are duplicated
        // there for some reason).
        let insensitive: Vec<String> = WEBSOCKET_HEADERS
            .iter()
            .map(|h| h.to_ascii_lowercase())
            .collect();

        for (k, v) in headers {
            let mut name = k.as_str();

            // We have already written the necessary headers once (above) and removed them from the
            // map. If we encounter them again, then the request is considered invalid
            // and error is returned. Note that we can't use `.contains()`, since `&str`
            // does not coerce to `&String` in Rust.
            if insensitive.iter().any(|x| x == name) {
                return Err(Error::Protocol(ProtocolError::InvalidHeader(k.clone())));
            }

            // Relates to the issue of some servers treating headers in a case-sensitive way, please
            // see: https://github.com/snapview/tungstenite-rs/pull/119 (original fix of the problem)
            if name == "sec-websocket-protocol" {
                name = "Sec-WebSocket-Protocol";
            }

            if name == "origin" {
                name = "Origin";
            }

            let value = v.to_str().map_err(|err| {
                Error::Utf8(format!("{err} for header name '{name}' with value: {v:?}"))
            })?;

            dst.extend_from_slice(name.as_ref());
            dst.extend_from_slice(b": ");
            dst.extend_from_slice(value.as_bytes());
            dst.extend_from_slice(b"\r\n");
        }

        dst.extend_from_slice(b"\r\n");
        Ok(())
    }
}

fn get_websocket_key(req: &Request) -> Result<&str> {
    let key = req
        .headers()
        .get(KEY_HEADERNAME)
        .ok_or_else(|| {
            Error::Protocol(ProtocolError::InvalidHeader(
                HeaderName::from_bytes(KEY_HEADERNAME.as_bytes()).unwrap(),
            ))
        })?
        .to_str()?;
    Ok(key)
}

fn extract_subprotocols_from_request(request: &Request) -> Result<Option<Vec<String>>> {
    if let Some(subprotocols) = request.headers().get("Sec-WebSocket-Protocol") {
        Ok(Some(
            subprotocols
                .to_str()?
                .split(',')
                .map(|s| s.trim().to_string())
                .collect(),
        ))
    } else {
        Ok(None)
    }
}

/// Decoder for response.
#[derive(Debug, Clone, Copy, Default)]
pub struct ResponseDecoder;

impl Decoder for ResponseDecoder {
    type Item = (usize, Response);
    type Error = Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Decoded<Self::Item>, Self::Error> {
        let mut hbuffer = [httparse::EMPTY_HEADER; MAX_HEADERS];
        let mut req = httparse::Response::new(&mut hbuffer);

        Ok(match req.parse(src)? {
            Status::Partial => Decoded::Insufficient,
            Status::Complete(size) => Decoded::Some((size, Response::from_httparse(req)?)),
        })
    }
}

impl<'h, 'b: 'h> FromHttparse<httparse::Response<'h, 'b>> for Response {
    fn from_httparse(raw: httparse::Response<'h, 'b>) -> Result<Self> {
        if raw.version.expect("Bug: no HTTP version") < /*1.*/1 {
            return Err(Error::Protocol(ProtocolError::WrongHttpVersion));
        }

        let headers = HeaderMap::from_httparse(raw.headers)?;

        let mut response = Response::new(None);
        *response.status_mut() = StatusCode::from_u16(raw.code.expect("Bug: no HTTP status code"))?;
        *response.headers_mut() = headers;
        // TODO: httparse only supports HTTP 0.9/1.0/1.1 but not HTTP 2.0
        // so the only valid value we could get in the response would be 1.1.
        *response.version_mut() = http::Version::HTTP_11;

        Ok(response)
    }
}

/// Generates a random key for the `Sec-WebSocket-Key` header.
pub fn generate_key() -> String {
    // a base64-encoded (see Section 4 of [RFC4648]) value that,
    // when decoded, is 16 bytes in length (RFC 6455)
    let r: [u8; 16] = rand::random();
    data_encoding::BASE64.encode(&r)
}

/// Information for handshake verification.
#[derive(Debug, Clone)]
struct VerifyData {
    /// Accepted server key.
    accept_key: String,

    /// Accepted subprotocols
    subprotocols: Option<Vec<String>>,
}

impl VerifyData {
    /// Verifies the response from the server against the expected values.
    pub fn verify_response(&self, resp: Response) -> Result<Response> {
        // 1. If the status code received from the server is not 101, the
        // client handles the response per HTTP [RFC2616] procedures. (RFC 6455)
        if resp.status() != StatusCode::SWITCHING_PROTOCOLS {
            return Err(Error::Http(Box::new(resp)));
        }

        let headers = resp.headers();

        // 2. If the response lacks an |Upgrade| header field or the |Upgrade|
        // header field contains a value that is not an ASCII case-
        // insensitive match for the value "websocket", the client MUST
        // _Fail the WebSocket Connection_. (RFC 6455)
        if !headers
            .get("Upgrade")
            .and_then(|h| h.to_str().ok())
            .map(|h| h.eq_ignore_ascii_case("websocket"))
            .unwrap_or(false)
        {
            return Err(Error::Protocol(
                ProtocolError::MissingUpgradeWebSocketHeader,
            ));
        }

        // 3. If the response lacks a |Connection| header field or the
        // |Connection| header field doesn't contain a token that is an
        // ASCII case-insensitive match for the value "Upgrade", the client
        // MUST _Fail the WebSocket Connection_. (RFC 6455)
        if !headers
            .get("Connection")
            .and_then(|h| h.to_str().ok())
            .map(|h| h.eq_ignore_ascii_case("Upgrade"))
            .unwrap_or(false)
        {
            return Err(Error::Protocol(
                ProtocolError::MissingConnectionUpgradeHeader,
            ));
        }

        // 4. If the response lacks a |Sec-WebSocket-Accept| header field or
        // the |Sec-WebSocket-Accept| contains a value other than the
        // base64-encoded SHA-1 of ... the client MUST _Fail the WebSocket
        // Connection_. (RFC 6455)
        if !headers
            .get("Sec-WebSocket-Accept")
            .map(|h| h == &self.accept_key)
            .unwrap_or(false)
        {
            return Err(Error::Protocol(
                ProtocolError::SecWebSocketAcceptKeyMismatch,
            ));
        }

        // 5. If the response includes a |Sec-WebSocket-Extensions| header
        // field and this header field indicates the use of an extension
        // that was not present in the client's handshake (the server has
        // indicated an extension not requested by the client), the client
        // MUST _Fail the WebSocket Connection_. (RFC 6455)
        // TODO

        // 6. If the response includes a |Sec-WebSocket-Protocol| header field
        // and this header field indicates the use of a subprotocol that was
        // not present in the client's handshake (the server has indicated a
        // subprotocol not requested by the client), the client MUST _Fail
        // the WebSocket Connection_. (RFC 6455)
        if headers.get("Sec-WebSocket-Protocol").is_none() && self.subprotocols.is_some() {
            return Err(Error::Protocol(
                ProtocolError::SecWebSocketSubProtocolError(SubProtocolError::NoSubProtocol),
            ));
        }

        if headers.get("Sec-WebSocket-Protocol").is_some() && self.subprotocols.is_none() {
            return Err(Error::Protocol(
                ProtocolError::SecWebSocketSubProtocolError(
                    SubProtocolError::ServerSentSubProtocolNoneRequested,
                ),
            ));
        }

        if let Some(returned_subprotocol) = headers.get("Sec-WebSocket-Protocol") {
            if let Some(accepted_subprotocols) = &self.subprotocols {
                if !accepted_subprotocols.contains(&returned_subprotocol.to_str()?.to_string()) {
                    return Err(Error::Protocol(
                        ProtocolError::SecWebSocketSubProtocolError(
                            SubProtocolError::InvalidSubProtocol,
                        ),
                    ));
                }
            }
        }

        Ok(resp)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::IntoClientRequest;

    #[test]
    fn random_keys() {
        let k1 = generate_key();
        println!("Generated random key 1: {k1}");
        let k2 = generate_key();
        println!("Generated random key 2: {k2}");
        assert_ne!(k1, k2);
        assert_eq!(k1.len(), k2.len());
        assert_eq!(k1.len(), 24);
        assert_eq!(k2.len(), 24);
        assert!(k1.ends_with("=="));
        assert!(k2.ends_with("=="));
        assert!(k1[..22].find('=').is_none());
        assert!(k2[..22].find('=').is_none());
    }

    fn construct_expected(host: &str, key: &str) -> Vec<u8> {
        format!(
            "\
            GET /getCaseCount HTTP/1.1\r\n\
            Host: {host}\r\n\
            Connection: Upgrade\r\n\
            Upgrade: websocket\r\n\
            Sec-WebSocket-Version: 13\r\n\
            Sec-WebSocket-Key: {key}\r\n\
            \r\n"
        )
        .into_bytes()
    }

    fn generate_request(request: Request) -> Result<(BytesMut, String)> {
        let key = get_websocket_key(&request)?.to_owned();
        let mut buf = BytesMut::new();
        RequestEncoder.encode(request, &mut buf)?;
        Ok((buf, key))
    }

    #[test]
    fn request_formatting() {
        let request = "ws://localhost/getCaseCount".into_client_request().unwrap();
        let (request, key) = generate_request(request).unwrap();
        let correct = construct_expected("localhost", &key);
        assert_eq!(&request[..], &correct[..]);
    }

    #[test]
    fn request_formatting_with_host() {
        let request = "wss://localhost:9001/getCaseCount"
            .into_client_request()
            .unwrap();
        let (request, key) = generate_request(request).unwrap();
        let correct = construct_expected("localhost:9001", &key);
        assert_eq!(&request[..], &correct[..]);
    }

    #[test]
    fn request_formatting_with_at() {
        let request = "wss://user:pass@localhost:9001/getCaseCount"
            .into_client_request()
            .unwrap();
        let (request, key) = generate_request(request).unwrap();
        let correct = construct_expected("localhost:9001", &key);
        assert_eq!(&request[..], &correct[..]);
    }

    #[test]
    fn response_parsing() {
        const DATA: &[u8] = b"HTTP/1.1 200 OK\r\nContent-Type: text/html\r\n\r\n";
        let (_, resp) = ResponseDecoder
            .decode(&mut BytesMut::from(DATA))
            .unwrap()
            .unwrap();
        assert_eq!(resp.status(), http::StatusCode::OK);
        assert_eq!(
            resp.headers().get("Content-Type").unwrap(),
            &b"text/html"[..],
        );
    }

    #[test]
    fn invalid_custom_request() {
        let request = http::Request::builder().method("GET").body(()).unwrap();
        assert!(generate_request(request).is_err());
    }
}
