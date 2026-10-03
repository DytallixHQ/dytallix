//! Loopback HTTP/1 adapter, with no TLS provider. A development build serves
//! the local profile; a production build (feature `production`, production
//! activation v1) serves only the production profile.
//! Hyper owns the HTTP parser. The Go engine owns JSON-RPC interpretation.
//! An optional client channel listener (E04 gap 19) serves the same requests
//! to remote clients over the post-quantum channel, and an optional status
//! listener serves a read-only status page for an uptime checker.
#![forbid(unsafe_op_in_unsafe_fn)]

pub mod channel;
pub mod status;

use base64::{engine::general_purpose::STANDARD, Engine};
use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::{body::Incoming, header, Method, Request, Response, StatusCode};
use hyper_util::rt::{TokioIo, TokioTimer};
use serde::{Deserialize, Serialize};
use std::{
    convert::Infallible,
    net::SocketAddr,
    os::unix::fs::{FileTypeExt, MetadataExt},
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, UnixStream},
    sync::Semaphore,
    time::{timeout, Instant},
};

/// Each build serves only its own profile.
#[cfg(not(feature = "production"))]
pub const PROFILE: &str = "dytallix-pqc-http-local-v1";
#[cfg(feature = "production")]
pub const PROFILE: &str = "dytallix-pqc-http-production-v1";
pub const ALLOWED_ORIGIN: &str = "http://127.0.0.1:4173";
pub const MAX_FRAME: usize = 2_097_152;
pub const MAX_REQUEST_BODY: usize = 1_048_576;
pub const MAX_RESPONSE_BODY: usize = 1_500_000;
pub const MAX_CONNECTIONS: usize = 32;
pub const MAX_HEADERS: usize = 64;
pub const MAX_HEADER_BYTES: usize = 65_536;
pub const DEADLINE: Duration = Duration::from_secs(10);
/// Hyper's smallest read buffer; `max_header_bytes` cannot go below it.
pub const MIN_HEADER_BYTES: usize = 8192;
type HttpBody = Full<Bytes>;

/// Serving limits. Each starts at its compiled ceiling above; an operator
/// flag can lower it but never raise it (E04 gap 13, P01 28 September 2026).
/// The IPC frame limit is the engine's and stays fixed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    pub max_connections: usize,
    pub max_request_body: usize,
    pub max_response_body: usize,
    pub max_headers: usize,
    pub max_header_bytes: usize,
    pub deadline: Duration,
}
impl Limits {
    pub const CEILING: Limits = Limits {
        max_connections: MAX_CONNECTIONS,
        max_request_body: MAX_REQUEST_BODY,
        max_response_body: MAX_RESPONSE_BODY,
        max_headers: MAX_HEADERS,
        max_header_bytes: MAX_HEADER_BYTES,
        deadline: DEADLINE,
    };
    /// Lower one limit from its flag. A value above the ceiling or below the
    /// floor (1, or `MIN_HEADER_BYTES` for header bytes) fails.
    fn tighten(&mut self, flag: &str, value: Option<String>) -> Result<(), String> {
        let value: u64 = value
            .ok_or("Missing limit value")?
            .parse()
            .map_err(|_| "Limits are decimal integers")?;
        let (slot, floor, ceiling) = match flag {
            "--max-connections" => (&mut self.max_connections, 1, MAX_CONNECTIONS),
            "--max-request-body-bytes" => (&mut self.max_request_body, 1, MAX_REQUEST_BODY),
            "--max-response-body-bytes" => (&mut self.max_response_body, 1, MAX_RESPONSE_BODY),
            "--max-headers" => (&mut self.max_headers, 1, MAX_HEADERS),
            "--max-header-bytes" => (
                &mut self.max_header_bytes,
                MIN_HEADER_BYTES,
                MAX_HEADER_BYTES,
            ),
            "--deadline-ms" => {
                if !(1..=DEADLINE.as_millis() as u64).contains(&value) {
                    return Err("Deadline must be from 1 ms up to its ceiling".into());
                }
                self.deadline = Duration::from_millis(value);
                return Ok(());
            }
            _ => return Err("Unknown or duplicate argument".into()),
        };
        let value = usize::try_from(value).map_err(|_| "Limit exceeds its ceiling")?;
        if !(floor..=ceiling).contains(&value) {
            return Err("A limit can only be lowered from its ceiling".into());
        }
        *slot = value;
        Ok(())
    }
}

#[derive(Debug)]
pub struct Config {
    pub home: PathBuf,
    pub listen: SocketAddr,
    pub socket: PathBuf,
    pub limits: Limits,
    pub channel: Option<channel::ChannelConfig>,
    /// The public status page's address, if served (P01, 3 October 2026).
    pub status: Option<SocketAddr>,
}

impl Config {
    pub fn parse(args: impl IntoIterator<Item = String>) -> Result<Self, String> {
        let mut home = None;
        let mut listen = None;
        let mut status = None;
        let mut profile = None;
        let mut limits = Limits::CEILING;
        let mut channel = channel::ChannelArgs::default();
        let mut set = std::collections::BTreeSet::new();
        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--production" => return Err("NO_GO: production is prohibited".into()),
                flag if channel::ChannelArgs::FLAGS.contains(&flag)
                    && set.insert(flag.to_owned()) =>
                {
                    channel.set(flag, args.next())?
                }
                "--home" if home.is_none() => home = args.next().map(PathBuf::from),
                "--listen" if listen.is_none() => {
                    listen = Some(
                        args.next()
                            .ok_or("Missing listen address")?
                            .parse::<SocketAddr>()
                            .map_err(|_| "Use a numeric IP:port")?,
                    );
                }
                "--status-listen" if status.is_none() => {
                    status = Some(
                        args.next()
                            .ok_or("Missing status listen address")?
                            .parse::<SocketAddr>()
                            .map_err(|_| "Use a numeric IP:port")?,
                    );
                }
                "--profile" if profile.is_none() => profile = args.next(),
                flag if (flag.starts_with("--max-") || flag == "--deadline-ms")
                    && set.insert(flag.to_owned()) =>
                {
                    limits.tighten(flag, args.next())?
                }
                _ => return Err("Unknown or duplicate argument".into()),
            }
        }
        if profile.as_deref() != Some(PROFILE) {
            return Err("This build's adapter profile is required".into());
        }
        let home = home.ok_or("Explicit private home required")?;
        let listen = listen.ok_or("Explicit numeric loopback address required")?;
        if !listen.ip().is_loopback() {
            return Err("Only numeric loopback is permitted".into());
        }
        if !home.is_absolute() || home.canonicalize().map_err(|_| "Home is unavailable")? != home {
            return Err("Home must be an absolute canonical path".into());
        }
        private_dir(&home)?;
        private_dir(&home.join("data"))?;
        let socket = home.join("data/rpc.sock");
        validate_socket(&socket)?;
        let channel = channel.finish(&home)?;
        if let Some(address) = status {
            status::check_listen(address, listen)?;
            if channel.as_ref().is_some_and(|c| c.listen == address) {
                return Err("The status and channel listeners need their own ports".into());
            }
        }
        Ok(Self {
            home,
            listen,
            socket,
            limits,
            channel,
            status,
        })
    }
}

pub(crate) fn own_metadata(path: &Path) -> Result<std::fs::Metadata, String> {
    let meta =
        std::fs::symlink_metadata(path).map_err(|_| "Required private path is unavailable")?;
    // This reads the calling process identity. It neither accesses nor loads keys.
    let uid = unsafe { libc::geteuid() };
    if meta.uid() != uid || meta.file_type().is_symlink() {
        return Err("Path ownership or type is invalid".into());
    }
    Ok(meta)
}
fn private_dir(path: &Path) -> Result<(), String> {
    let meta = own_metadata(path)?;
    if !meta.is_dir() || meta.mode() & 0o777 != 0o700 {
        return Err("Private directories must have mode 0700".into());
    }
    Ok(())
}
fn validate_socket(path: &Path) -> Result<(), String> {
    let parent = path.parent().ok_or("Missing socket parent")?;
    private_dir(parent)?;
    let home = parent.parent().ok_or("Missing home")?;
    private_dir(home)?;
    if home.canonicalize().map_err(|_| "Home is unavailable")? != home {
        return Err("Home path changed".into());
    }
    let meta = own_metadata(path)?;
    if !meta.file_type().is_socket() || meta.mode() & 0o777 != 0o600 {
        return Err("RPC path must be a private mode-0600 Unix socket".into());
    }
    Ok(())
}

#[derive(Serialize)]
#[serde(deny_unknown_fields)]
struct IpcRequest<'a> {
    version: u8,
    method: &'a str,
    path: &'a str,
    query: &'a str,
    body_base64: String,
    remote_addr: String,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct IpcResponse {
    version: u8,
    status: u16,
    headers: IpcHeaders,
    body_base64: String,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct IpcHeaders {
    #[serde(rename = "Content-Type")]
    content_type: Option<String>,
    #[serde(rename = "Cache-Control")]
    cache_control: Option<String>,
}

/// An engine answer, or a fixed local error, before it takes the HTTP or the
/// channel form.
pub(crate) struct Reply {
    pub status: u16,
    pub content_type: Option<String>,
    pub cache_control: Option<String>,
    pub body: Vec<u8>,
}

impl Reply {
    /// Fixed local errors contain no request, transaction or key material.
    pub(crate) fn error(status: StatusCode, message: &'static str) -> Self {
        Reply {
            status: status.as_u16(),
            content_type: Some("application/json".into()),
            cache_control: None,
            body: serde_json::to_vec(&serde_json::json!({"error":message}))
                .expect("fixed error JSON"),
        }
    }

    fn into_http(self) -> Response<HttpBody> {
        let mut http = Response::builder().status(self.status);
        for (name, value) in [
            (header::CONTENT_TYPE, self.content_type),
            (header::CACHE_CONTROL, self.cache_control),
        ] {
            if let Some(value) = value {
                match header::HeaderValue::from_str(&value) {
                    Ok(parsed) => http = http.header(name, parsed),
                    Err(_) => {
                        return response(StatusCode::BAD_GATEWAY, "Engine RPC response failed")
                    }
                }
            }
        }
        http.body(Full::new(Bytes::from(self.body)))
            .unwrap_or_else(|_| response(StatusCode::BAD_GATEWAY, "Engine RPC response failed"))
    }
}

fn decode_reply(raw: &[u8], limits: &Limits) -> Result<Reply, String> {
    let response: IpcResponse = serde_json::from_slice(raw).map_err(|_| "Invalid IPC response")?;
    if response.version != 1 || !(200..=599).contains(&response.status) {
        return Err("Unsupported IPC response version or final status".into());
    }
    let body = STANDARD
        .decode(&response.body_base64)
        .map_err(|_| "Invalid response encoding")?;
    if body.len() > limits.max_response_body || STANDARD.encode(&body) != response.body_base64 {
        return Err("Noncanonical or excessive response body".into());
    }
    for value in [
        &response.headers.content_type,
        &response.headers.cache_control,
    ]
    .into_iter()
    .flatten()
    {
        if value.len() > limits.max_header_bytes {
            return Err("Excessive response header".into());
        }
        header::HeaderValue::from_str(value).map_err(|_| "Invalid IPC response header")?;
    }
    Ok(Reply {
        status: response.status,
        content_type: response.headers.content_type,
        cache_control: response.headers.cache_control,
        body,
    })
}

async fn forward(socket: &Path, frame: &[u8], limits: &Limits) -> Result<Reply, String> {
    if frame.is_empty() || frame.len() > MAX_FRAME {
        return Err("Excessive request frame".into());
    }
    validate_socket(socket)?;
    let mut stream = UnixStream::connect(socket)
        .await
        .map_err(|_| "Engine RPC unavailable")?;
    stream
        .write_all(&(frame.len() as u32).to_be_bytes())
        .await
        .map_err(|_| "IPC write failed")?;
    stream
        .write_all(frame)
        .await
        .map_err(|_| "IPC write failed")?;
    // Keep the write side open. Closing it signals caller cancellation to Go.
    let length = stream
        .read_u32()
        .await
        .map_err(|_| "IPC response unavailable")? as usize;
    if length == 0 || length > MAX_FRAME {
        return Err("IPC response frame exceeds limit".into());
    }
    let mut raw = vec![0; length];
    stream
        .read_exact(&mut raw)
        .await
        .map_err(|_| "Incomplete IPC response")?;
    decode_reply(&raw, limits)
}

/// Sends one checked request to the engine's client socket within the
/// deadline. Both listeners end here.
pub(crate) async fn engine(
    method: &str,
    path: &str,
    query: &str,
    body: &[u8],
    peer: SocketAddr,
    socket: &Path,
    limits: &Limits,
) -> Reply {
    let ipc = IpcRequest {
        version: 1,
        method,
        path,
        query,
        body_base64: STANDARD.encode(body),
        remote_addr: peer.to_string(),
    };
    let frame = match serde_json::to_vec(&ipc) {
        Ok(frame) if frame.len() <= MAX_FRAME => frame,
        _ => return Reply::error(StatusCode::PAYLOAD_TOO_LARGE, "Request frame exceeds limit"),
    };
    match timeout(limits.deadline, forward(socket, &frame, limits)).await {
        Ok(Ok(reply)) => reply,
        Ok(Err(_)) => Reply::error(StatusCode::BAD_GATEWAY, "Engine RPC response failed"),
        Err(_) => Reply::error(StatusCode::GATEWAY_TIMEOUT, "Engine RPC deadline expired"),
    }
}

fn response(status: StatusCode, message: &'static str) -> Response<HttpBody> {
    let reply = Reply::error(status, message);
    Response::builder()
        .status(reply.status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Full::new(Bytes::from(reply.body)))
        .expect("fixed HTTP response")
}
fn add_cors(response: &mut Response<HttpBody>, permitted: bool) {
    if permitted {
        response.headers_mut().insert(
            header::ACCESS_CONTROL_ALLOW_ORIGIN,
            header::HeaderValue::from_static(ALLOWED_ORIGIN),
        );
        response
            .headers_mut()
            .insert(header::VARY, header::HeaderValue::from_static("Origin"));
    }
}
fn origin_allowed(headers: &header::HeaderMap) -> Result<bool, ()> {
    if headers.get_all(header::ORIGIN).iter().count() > 1 {
        return Err(());
    }
    match headers.get(header::ORIGIN) {
        None => Ok(false),
        Some(value) if value.as_bytes() == ALLOWED_ORIGIN.as_bytes() => Ok(true),
        _ => Err(()),
    }
}
fn preflight(headers: &header::HeaderMap, cors: bool) -> Response<HttpBody> {
    let method = headers
        .get(header::ACCESS_CONTROL_REQUEST_METHOD)
        .and_then(|v| v.to_str().ok());
    if !cors
        || headers
            .get_all(header::ACCESS_CONTROL_REQUEST_METHOD)
            .iter()
            .count()
            != 1
        || !matches!(method, Some("GET" | "POST"))
    {
        return response(StatusCode::FORBIDDEN, "Preflight rejected");
    }
    let mut names = headers
        .get_all(header::ACCESS_CONTROL_REQUEST_HEADERS)
        .iter();
    if let Some(value) = names.next() {
        let valid = value
            .to_str()
            .ok()
            .map(|value| {
                value
                    .split(',')
                    .all(|name| name.trim().eq_ignore_ascii_case("content-type"))
            })
            .unwrap_or(false);
        if !valid || names.next().is_some() {
            return response(StatusCode::FORBIDDEN, "Preflight headers rejected");
        }
    }
    Response::builder()
        .status(StatusCode::NO_CONTENT)
        .header(header::ACCESS_CONTROL_ALLOW_METHODS, "GET, POST, OPTIONS")
        .header(header::ACCESS_CONTROL_ALLOW_HEADERS, "content-type")
        .body(Full::new(Bytes::new()))
        .expect("fixed preflight")
}
pub(crate) fn valid_rpc_path(path: &str) -> bool {
    path.len() <= 128
        && path.starts_with('/')
        && path.as_bytes()[1..]
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || *b == b'_')
}

async fn handle_inner(
    req: Request<Incoming>,
    peer: SocketAddr,
    socket: &Path,
    cors: bool,
    limits: &Limits,
) -> Response<HttpBody> {
    if req.method() == Method::OPTIONS {
        return preflight(req.headers(), cors);
    }
    if req.headers().contains_key(header::UPGRADE) || req.uri().path() == "/websocket" {
        return response(
            StatusCode::NOT_IMPLEMENTED,
            "WebSocket and upgrades are unsupported",
        );
    }
    if req.method() != Method::POST && req.method() != Method::GET {
        let mut result = response(StatusCode::METHOD_NOT_ALLOWED, "Unsupported method");
        result.headers_mut().insert(
            header::ALLOW,
            header::HeaderValue::from_static("GET, POST, OPTIONS"),
        );
        return result;
    }
    if req.uri().scheme().is_some()
        || req.uri().authority().is_some()
        || !valid_rpc_path(req.uri().path())
        || req.uri().query().unwrap_or("").len() > 16_384
    {
        return response(StatusCode::BAD_REQUEST, "Unsupported request target");
    }
    if req.method() == Method::GET && req.uri().path() == "/"
        || req.method() == Method::POST && req.uri().path() != "/"
    {
        return response(
            StatusCode::NOT_IMPLEMENTED,
            "RPC route is unsupported in this profile",
        );
    }
    if req.headers().contains_key(header::CONTENT_ENCODING) {
        return response(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "Encoded request bodies are unsupported",
        );
    }
    if req.method() == Method::POST {
        let ct = req
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok());
        if req.headers().get_all(header::CONTENT_TYPE).iter().count() != 1
            || !ct
                .map(|v| {
                    v.split(';')
                        .next()
                        .unwrap_or("")
                        .trim()
                        .eq_ignore_ascii_case("application/json")
                })
                .unwrap_or(false)
        {
            return response(StatusCode::NOT_IMPLEMENTED, "Only JSON POST is supported");
        }
    }
    let (parts, mut body) = req.into_parts();
    let mut bytes = Vec::new();
    while let Some(frame) = body.frame().await {
        let frame = match frame {
            Ok(frame) => frame,
            Err(_) => return response(StatusCode::BAD_REQUEST, "HTTP body failed"),
        };
        if let Ok(chunk) = frame.into_data() {
            if chunk.len() > limits.max_request_body - bytes.len() {
                return response(StatusCode::PAYLOAD_TOO_LARGE, "Request body exceeds limit");
            }
            bytes.extend_from_slice(&chunk);
        }
    }
    if parts.method == Method::GET && !bytes.is_empty() {
        return response(StatusCode::BAD_REQUEST, "GET body is unsupported");
    }
    engine(
        parts.method.as_str(),
        parts.uri.path(),
        parts.uri.query().unwrap_or(""),
        &bytes,
        peer,
        socket,
        limits,
    )
    .await
    .into_http()
}
async fn handle(
    req: Request<Incoming>,
    peer: SocketAddr,
    socket: Arc<PathBuf>,
    limits: Limits,
) -> Result<Response<HttpBody>, Infallible> {
    let cors = match origin_allowed(req.headers()) {
        Ok(cors) => cors,
        Err(_) => return Ok(response(StatusCode::FORBIDDEN, "Origin rejected")),
    };
    let mut result = handle_inner(req, peer, &socket, cors, &limits).await;
    add_cors(&mut result, cors);
    Ok(result)
}

pub async fn serve(config: Config) -> Result<(), String> {
    validate_socket(&config.socket)?;
    let listener = TcpListener::bind(config.listen)
        .await
        .map_err(|_| "Cannot bind loopback listener")?;
    let actual = listener
        .local_addr()
        .map_err(|_| "Cannot inspect listener")?;
    let channel = match config.channel {
        Some(channel) => {
            let listener = TcpListener::bind(channel.listen)
                .await
                .map_err(|_| "Cannot bind channel listener")?;
            Some((listener, channel))
        }
        None => None,
    };
    let status = match config.status {
        Some(address) => Some(
            TcpListener::bind(address)
                .await
                .map_err(|_| "Cannot bind status listener")?,
        ),
        None => None,
    };
    let mut ready = if cfg!(feature = "production") {
        serde_json::json!({"status":"READY","profile":PROFILE,"listen":actual.to_string(),"build":"production"})
    } else {
        serde_json::json!({"status":"EXPERIMENTAL_LOCAL_ONLY","profile":PROFILE,"listen":actual.to_string(),"production_authorized":false,"launch_status":"NO_GO"})
    };
    if let Some(listener) = &status {
        let bound = listener
            .local_addr()
            .map_err(|_| "Cannot inspect status listener")?;
        ready["status_listen"] = bound.to_string().into();
    }
    if let Some((listener, channel)) = &channel {
        let bound = listener
            .local_addr()
            .map_err(|_| "Cannot inspect channel listener")?;
        ready["channel_listen"] = bound.to_string().into();
        ready["channel_key_sha256"] = channel.fingerprint().into();
    }
    println!("{ready}");
    let limits = config.limits;
    let socket = Arc::new(config.socket);
    // An absent optional listener never finishes.
    let channel_socket = socket.clone();
    let channel_task = async move {
        match channel {
            Some((listener, channel)) => {
                channel::serve(listener, channel, channel_socket, limits).await
            }
            None => std::future::pending().await,
        }
    };
    let status_socket = socket.clone();
    let status_task = async move {
        match status {
            Some(listener) => status::serve(listener, status_socket).await,
            None => std::future::pending().await,
        }
    };
    tokio::select! {
        result = serve_http(listener, socket, limits) => result,
        result = channel_task => result,
        result = status_task => result,
    }
}

async fn serve_http(
    listener: TcpListener,
    socket: Arc<PathBuf>,
    limits: Limits,
) -> Result<(), String> {
    let capacity = Arc::new(Semaphore::new(limits.max_connections));
    loop {
        let (stream, peer) = listener.accept().await.map_err(|_| "Listener failed")?;
        if !peer.ip().is_loopback() {
            continue;
        }
        let permit = match capacity.clone().try_acquire_owned() {
            Ok(permit) => permit,
            Err(_) => continue,
        };
        let socket = socket.clone();
        tokio::spawn(async move {
            let _permit = permit;
            let deadline = Instant::now() + limits.deadline;
            let service =
                hyper::service::service_fn(move |req| handle(req, peer, socket.clone(), limits));
            let mut builder = hyper::server::conn::http1::Builder::new();
            builder
                .timer(TokioTimer::new())
                .keep_alive(false)
                .max_headers(limits.max_headers)
                .max_buf_size(limits.max_header_bytes)
                .header_read_timeout(limits.deadline);
            // One HTTP request per accepted connection. Dropping this future
            // closes the local IPC stream and cancels the engine request.
            let _ = tokio::time::timeout_at(
                deadline,
                builder.serve_connection(TokioIo::new(stream), service),
            )
            .await;
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn limits_can_only_be_lowered_once_each() {
        let mut limits = Limits::CEILING;
        for (flag, value) in [
            ("--max-connections", "8"),
            ("--max-request-body-bytes", "65536"),
            ("--max-response-body-bytes", "100000"),
            ("--max-headers", "16"),
            ("--max-header-bytes", "8192"),
            ("--deadline-ms", "2500"),
        ] {
            limits.tighten(flag, Some(value.into())).unwrap();
        }
        assert_eq!(
            limits,
            Limits {
                max_connections: 8,
                max_request_body: 65536,
                max_response_body: 100_000,
                max_headers: 16,
                max_header_bytes: 8192,
                deadline: Duration::from_millis(2500),
            }
        );
        for (flag, value) in [
            ("--max-connections", "33"),
            ("--max-connections", "0"),
            ("--max-connections", "-1"),
            ("--max-connections", "1e3"),
            ("--max-request-body-bytes", "1048577"),
            ("--max-response-body-bytes", "1500001"),
            ("--max-headers", "65"),
            ("--max-header-bytes", "8191"),
            ("--max-header-bytes", "65537"),
            ("--deadline-ms", "0"),
            ("--deadline-ms", "10001"),
            ("--max-sockets", "1"),
        ] {
            let mut limits = Limits::CEILING;
            assert!(
                limits.tighten(flag, Some(value.into())).is_err(),
                "{flag} {value}"
            );
            assert_eq!(limits, Limits::CEILING);
        }
        let mut limits = Limits::CEILING;
        assert!(limits.tighten("--max-connections", None).is_err());
        // A repeated flag fails before any path is read, even at the ceiling.
        assert!(Config::parse(
            ["--max-connections", "32", "--max-connections", "8"].map(String::from)
        )
        .unwrap_err()
        .contains("duplicate"));
    }
    #[test]
    fn a_lowered_response_limit_refuses_larger_bodies() {
        let body = STANDARD.encode(vec![b'x'; 101]);
        let raw = format!(r#"{{"version":1,"status":200,"headers":{{}},"body_base64":"{body}"}}"#);
        let mut limits = Limits::CEILING;
        assert!(decode_reply(raw.as_bytes(), &limits).is_ok());
        limits
            .tighten("--max-response-body-bytes", Some("100".into()))
            .unwrap();
        assert!(decode_reply(raw.as_bytes(), &limits).is_err());
    }
    #[test]
    fn production_and_unknown_profiles_refuse_before_paths() {
        assert!(Config::parse(["--production".to_owned()])
            .unwrap_err()
            .contains("NO_GO"));
        assert!(Config::parse(["--profile".to_owned(), "production".to_owned()]).is_err());
        assert!(Config::parse([
            "--profile".to_owned(),
            PROFILE.to_owned(),
            "--listen".to_owned(),
            "localhost:1".to_owned()
        ])
        .is_err());
    }
    #[test]
    fn each_build_serves_only_its_own_profile() {
        let other = if cfg!(feature = "production") {
            "dytallix-pqc-http-local-v1"
        } else {
            "dytallix-pqc-http-production-v1"
        };
        assert_ne!(other, PROFILE);
        assert!(Config::parse(["--profile", other].map(String::from))
            .unwrap_err()
            .contains("adapter profile"));
        let status = [
            "--status-listen",
            "203.0.113.1:80",
            "--status-listen",
            "203.0.113.1:81",
        ];
        assert!(Config::parse(status.map(String::from))
            .unwrap_err()
            .contains("duplicate"));
        assert!(
            Config::parse(["--status-listen", "status.example:80"].map(String::from))
                .unwrap_err()
                .contains("numeric")
        );
    }
    #[test]
    fn canonical_rpc_paths() {
        for path in ["/", "/abci_query", "/broadcast_tx_sync"] {
            assert!(valid_rpc_path(path));
        }
        for path in ["", "abc", "//", "/../x", "/a%2fb", "/a/b", "/é"] {
            assert!(!valid_rpc_path(path));
        }
        assert!(!valid_rpc_path(&format!("/{}", "x".repeat(128))));
    }
    #[test]
    fn strict_origin_and_preflight() {
        let mut h = header::HeaderMap::new();
        assert_eq!(origin_allowed(&h), Ok(false));
        h.insert(
            header::ORIGIN,
            header::HeaderValue::from_static(ALLOWED_ORIGIN),
        );
        assert_eq!(origin_allowed(&h), Ok(true));
        h.insert(
            header::ACCESS_CONTROL_REQUEST_METHOD,
            header::HeaderValue::from_static("POST"),
        );
        h.insert(
            header::ACCESS_CONTROL_REQUEST_HEADERS,
            header::HeaderValue::from_static("content-type"),
        );
        assert_eq!(preflight(&h, true).status(), StatusCode::NO_CONTENT);
        h.insert(
            header::ACCESS_CONTROL_REQUEST_HEADERS,
            header::HeaderValue::from_static("authorization"),
        );
        assert_eq!(preflight(&h, true).status(), StatusCode::FORBIDDEN);
        h.append(
            header::ORIGIN,
            header::HeaderValue::from_static(ALLOWED_ORIGIN),
        );
        assert!(origin_allowed(&h).is_err());
    }
    #[test]
    fn exact_response_contract() {
        let raw=br#"{"version":1,"status":200,"headers":{"Content-Type":"application/json"},"body_base64":"e30="}"#;
        assert_eq!(decode_reply(raw, &Limits::CEILING).unwrap().status, 200);
        for text in [
            r#"{"version":1,"version":1,"status":200,"headers":{},"body_base64":""}"#,
            r#"{"version":2,"status":200,"headers":{},"body_base64":""}"#,
            r#"{"version":1,"status":101,"headers":{},"body_base64":""}"#,
            r#"{"version":1,"status":200,"headers":{"Access-Control-Allow-Origin":"*"},"body_base64":""}"#,
            r#"{"version":1,"status":200,"headers":{"Content-Type":"a","Content-Type":"b"},"body_base64":""}"#,
            r#"{"version":1,"status":200,"headers":{},"body_base64":"e30"}"#,
            r#"{"version":1,"status":200,"headers":{},"body_base64":"","extra":true}"#,
        ] {
            assert!(decode_reply(text.as_bytes(), &Limits::CEILING).is_err());
        }
    }
}
