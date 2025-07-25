//! Methods to connect to a WebSocket as a client.

use http::{HeaderName, Uri};
use monoio::{
    io::{AsyncReadRent, AsyncWriteRent},
    net::TcpStream,
};

use crate::{
    error::{Error, Result, UrlError},
    handshake::client::{Request, Response, client_handshake, generate_key},
    protocol::{WebSocket, WebSocketConfig},
    stream::{MaybeTlsStream, Mode},
};

/// Connects to the given WebSocket.
///
/// Accepts any request that implements [`IntoClientRequest`], which is often just `&str`, but can
/// be a variety of types such as [`httparse::Request`] or [`http::Request`] for more
/// complex uses.
pub async fn connect<R: IntoClientRequest>(
    request: R,
) -> Result<(WebSocket<MaybeTlsStream<TcpStream>>, Response)> {
    connect_with_config(request, None, false).await
}

/// The same as [`connect`] but the one can specify a websocket configuration. `disable_nagle`
/// specifies if the Nagle's algorithm must be disabled, i.e. `set_nodelay(true)`. If you don't know
/// what the Nagle's algorithm is, better leave it set to `false`.
pub async fn connect_with_config<R: IntoClientRequest>(
    request: R,
    config: Option<WebSocketConfig>,
    disable_nagle: bool,
) -> Result<(WebSocket<MaybeTlsStream<TcpStream>>, Response)> {
    let request = request.into_client_request()?;
    let uri = request.uri();

    let host = match uri.host() {
        Some(d) if d.starts_with('[') && d.ends_with(']') => d[1..d.len() - 1].to_string(),
        Some(d) => d.to_string(),
        None => return Err(Error::Url(UrlError::NoHostName)),
    };

    let port = uri.port_u16().unwrap_or(match uri_mode(uri)? {
        Mode::Plain => 80,
        Mode::Tls => 443,
    });

    let stream = TcpStream::connect((host, port)).await?;
    if disable_nagle {
        stream.set_nodelay(true)?;
    }

    client_with_config(request, MaybeTlsStream::Plain(stream), config).await
}

/// Gets the mode of the given URL.
pub fn uri_mode(uri: &Uri) -> Result<Mode> {
    match uri.scheme_str() {
        Some("ws") => Ok(Mode::Plain),
        Some("wss") => Ok(Mode::Tls),
        _ => Err(Error::Url(UrlError::UnsupportedUrlScheme)),
    }
}

/// Performs the client handshake over the given stream given a web socket configuration. Passing
/// `None` as configuration is equal to calling [`client`] function.
pub async fn client_with_config<S, R>(
    request: R,
    stream: S,
    config: Option<WebSocketConfig>,
) -> Result<(WebSocket<S>, Response)>
where
    S: AsyncReadRent + AsyncWriteRent,
    R: IntoClientRequest,
{
    client_handshake(request.into_client_request()?, config, stream).await
}

/// Performs the client handshake over the given stream.
pub async fn client<S, R>(request: R, stream: S) -> Result<(WebSocket<S>, Response)>
where
    S: AsyncReadRent + AsyncWriteRent,
    R: IntoClientRequest,
{
    client_with_config(request, stream, None).await
}

/// Trait for converting various types into HTTP requests used for a client connection.
///
/// This trait is implemented by default for string slices, strings, [`http::Uri`] and
/// [`http::Request<()>`]. Note that the implementation for [`http::Request<()>`] is trivial and
/// will simply take your request and pass it as is further without altering any headers or URLs, so
/// be aware of this.
pub trait IntoClientRequest {
    /// Convert into a `Request` that can be used for a client connection.
    fn into_client_request(self) -> Result<Request>;
}

impl IntoClientRequest for &str {
    fn into_client_request(self) -> Result<Request> {
        self.parse::<Uri>()?.into_client_request()
    }
}

impl IntoClientRequest for &String {
    fn into_client_request(self) -> Result<Request> {
        <&str as IntoClientRequest>::into_client_request(self)
    }
}

impl IntoClientRequest for String {
    fn into_client_request(self) -> Result<Request> {
        <&str as IntoClientRequest>::into_client_request(&self)
    }
}

impl IntoClientRequest for &Uri {
    fn into_client_request(self) -> Result<Request> {
        self.clone().into_client_request()
    }
}

impl IntoClientRequest for Uri {
    fn into_client_request(self) -> Result<Request> {
        let authority = self
            .authority()
            .ok_or(Error::Url(UrlError::NoHostName))?
            .as_str();
        let host = authority
            .find('@')
            .map(|idx| authority.split_at(idx + 1).1)
            .unwrap_or_else(|| authority);

        if host.is_empty() {
            return Err(Error::Url(UrlError::EmptyHostName));
        }

        let req = Request::builder()
            .method("GET")
            .header("Host", host)
            .header("Connection", "Upgrade")
            .header("Upgrade", "websocket")
            .header("Sec-WebSocket-Version", "13")
            .header("Sec-WebSocket-Key", generate_key())
            .uri(self)
            .body(())?;
        Ok(req)
    }
}

#[cfg(feature = "url")]
impl IntoClientRequest for &url::Url {
    fn into_client_request(self) -> Result<Request> {
        self.as_str().into_client_request()
    }
}

#[cfg(feature = "url")]
impl IntoClientRequest for url::Url {
    fn into_client_request(self) -> Result<Request> {
        self.as_str().into_client_request()
    }
}

impl IntoClientRequest for Request {
    fn into_client_request(self) -> Result<Request> {
        Ok(self)
    }
}

impl IntoClientRequest for httparse::Request<'_, '_> {
    fn into_client_request(self) -> Result<Request> {
        use crate::handshake::headers::FromHttparse;

        Request::from_httparse(self)
    }
}

/// Builder for a custom [`IntoClientRequest`] with options to add
/// custom additional headers and sub protocols.
#[derive(Debug, Clone)]
pub struct ClientRequestBuilder {
    uri: Uri,
    /// Additional [`Request`] handshake headers
    additional_headers: Vec<(String, String)>,
    /// Handshake subprotocols
    subprotocols: Vec<String>,
}

impl ClientRequestBuilder {
    /// Initializes an empty request builder
    #[must_use]
    pub const fn new(uri: Uri) -> Self {
        Self {
            uri,
            additional_headers: Vec::new(),
            subprotocols: Vec::new(),
        }
    }

    /// Adds (`key`, `value`) as an additional header to the handshake request
    pub fn with_header<K, V>(mut self, key: K, value: V) -> Self
    where
        K: Into<String>,
        V: Into<String>,
    {
        self.additional_headers.push((key.into(), value.into()));
        self
    }

    /// Adds `protocol` to the handshake request subprotocols (`Sec-WebSocket-Protocol`)
    pub fn with_sub_protocol<P>(mut self, protocol: P) -> Self
    where
        P: Into<String>,
    {
        self.subprotocols.push(protocol.into());
        self
    }
}

impl IntoClientRequest for ClientRequestBuilder {
    fn into_client_request(self) -> Result<Request> {
        let mut request = self.uri.into_client_request()?;
        let headers = request.headers_mut();
        for (k, v) in self.additional_headers {
            let key = HeaderName::try_from(k)?;
            let value = v.parse()?;
            headers.append(key, value);
        }
        if !self.subprotocols.is_empty() {
            let protocols = self.subprotocols.join(", ").parse()?;
            headers.append("Sec-WebSocket-Protocol", protocols);
        }
        Ok(request)
    }
}
