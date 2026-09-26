//! Experimental loopback HTTP/1 adapter. No production authority or TLS provider.
//! Hyper owns the HTTP parser. The Go engine owns JSON-RPC interpretation.
#![forbid(unsafe_op_in_unsafe_fn)]

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

pub const PROFILE: &str = "dytallix-pqc-http-local-v1";
pub const ALLOWED_ORIGIN: &str = "http://127.0.0.1:4173";
pub const MAX_FRAME: usize = 2_097_152;
pub const MAX_REQUEST_BODY: usize = 1_048_576;
pub const MAX_RESPONSE_BODY: usize = 1_500_000;
pub const MAX_CONNECTIONS: usize = 32;
pub const MAX_HEADERS: usize = 64;
pub const MAX_HEADER_BYTES: usize = 65_536;
pub const DEADLINE: Duration = Duration::from_secs(10);
type HttpBody = Full<Bytes>;

#[derive(Debug)]
pub struct Config {
    pub home: PathBuf,
    pub listen: SocketAddr,
    pub socket: PathBuf,
}

impl Config {
    pub fn parse(args: impl IntoIterator<Item = String>) -> Result<Self, String> {
        let mut home = None;
        let mut listen = None;
        let mut profile = None;
        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--production" => return Err("NO_GO: production is prohibited".into()),
                "--home" if home.is_none() => home = args.next().map(PathBuf::from),
                "--listen" if listen.is_none() => {
                    listen = Some(
                        args.next()
                            .ok_or("Missing listen address")?
                            .parse::<SocketAddr>()
                            .map_err(|_| "Use a numeric IP:port")?,
                    );
                }
                "--profile" if profile.is_none() => profile = args.next(),
                _ => return Err("Unknown or duplicate argument".into()),
            }
        }
        if profile.as_deref() != Some(PROFILE) {
            return Err("Explicit supported experimental profile required".into());
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
        Ok(Self {
            home,
            listen,
            socket,
        })
    }
}

fn own_metadata(path: &Path) -> Result<std::fs::Metadata, String> {
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

fn decode_response(raw: &[u8]) -> Result<Response<HttpBody>, String> {
    let response: IpcResponse = serde_json::from_slice(raw).map_err(|_| "Invalid IPC response")?;
    if response.version != 1 || !(200..=599).contains(&response.status) {
        return Err("Unsupported IPC response version or final status".into());
    }
    let body = STANDARD
        .decode(&response.body_base64)
        .map_err(|_| "Invalid response encoding")?;
    if body.len() > MAX_RESPONSE_BODY || STANDARD.encode(&body) != response.body_base64 {
        return Err("Noncanonical or excessive response body".into());
    }
    let mut http = Response::builder().status(response.status);
    for (name, value) in [
        (header::CONTENT_TYPE, response.headers.content_type),
        (header::CACHE_CONTROL, response.headers.cache_control),
    ] {
        if let Some(value) = value {
            if value.len() > MAX_HEADER_BYTES {
                return Err("Excessive response header".into());
            }
            let parsed =
                header::HeaderValue::from_str(&value).map_err(|_| "Invalid IPC response header")?;
            http = http.header(name, parsed);
        }
    }
    http.body(Full::new(Bytes::from(body)))
        .map_err(|_| "Invalid IPC response".to_owned())
}

async fn forward(socket: &Path, frame: &[u8]) -> Result<Response<HttpBody>, String> {
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
    decode_response(&raw)
}

fn response(status: StatusCode, message: &'static str) -> Response<HttpBody> {
    // Fixed local errors contain no request, transaction or key material.
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Full::new(Bytes::from(
            serde_json::to_vec(&serde_json::json!({"error":message})).expect("fixed error JSON"),
        )))
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
fn valid_rpc_path(path: &str) -> bool {
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
            if chunk.len() > MAX_REQUEST_BODY - bytes.len() {
                return response(StatusCode::PAYLOAD_TOO_LARGE, "Request body exceeds limit");
            }
            bytes.extend_from_slice(&chunk);
        }
    }
    if parts.method == Method::GET && !bytes.is_empty() {
        return response(StatusCode::BAD_REQUEST, "GET body is unsupported");
    }
    let ipc = IpcRequest {
        version: 1,
        method: parts.method.as_str(),
        path: parts.uri.path(),
        query: parts.uri.query().unwrap_or(""),
        body_base64: STANDARD.encode(bytes),
        remote_addr: peer.to_string(),
    };
    let frame = match serde_json::to_vec(&ipc) {
        Ok(frame) if frame.len() <= MAX_FRAME => frame,
        _ => return response(StatusCode::PAYLOAD_TOO_LARGE, "Request frame exceeds limit"),
    };
    match timeout(DEADLINE, forward(socket, &frame)).await {
        Ok(Ok(result)) => result,
        Ok(Err(_)) => response(StatusCode::BAD_GATEWAY, "Engine RPC response failed"),
        Err(_) => response(StatusCode::GATEWAY_TIMEOUT, "Engine RPC deadline expired"),
    }
}
async fn handle(
    req: Request<Incoming>,
    peer: SocketAddr,
    socket: Arc<PathBuf>,
) -> Result<Response<HttpBody>, Infallible> {
    let cors = match origin_allowed(req.headers()) {
        Ok(cors) => cors,
        Err(_) => return Ok(response(StatusCode::FORBIDDEN, "Origin rejected")),
    };
    let mut result = handle_inner(req, peer, &socket, cors).await;
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
    println!(
        "{}",
        serde_json::json!({"status":"EXPERIMENTAL_LOCAL_ONLY","profile":PROFILE,"listen":actual.to_string(),"production_authorized":false,"launch_status":"NO_GO"})
    );
    let capacity = Arc::new(Semaphore::new(MAX_CONNECTIONS));
    let socket = Arc::new(config.socket);
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
            let deadline = Instant::now() + DEADLINE;
            let service = hyper::service::service_fn(move |req| handle(req, peer, socket.clone()));
            let mut builder = hyper::server::conn::http1::Builder::new();
            builder
                .timer(TokioTimer::new())
                .keep_alive(false)
                .max_headers(MAX_HEADERS)
                .max_buf_size(MAX_HEADER_BYTES)
                .header_read_timeout(DEADLINE);
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
        assert_eq!(decode_response(raw).unwrap().status(), StatusCode::OK);
        for text in [
            r#"{"version":1,"version":1,"status":200,"headers":{},"body_base64":""}"#,
            r#"{"version":2,"status":200,"headers":{},"body_base64":""}"#,
            r#"{"version":1,"status":101,"headers":{},"body_base64":""}"#,
            r#"{"version":1,"status":200,"headers":{"Access-Control-Allow-Origin":"*"},"body_base64":""}"#,
            r#"{"version":1,"status":200,"headers":{"Content-Type":"a","Content-Type":"b"},"body_base64":""}"#,
            r#"{"version":1,"status":200,"headers":{},"body_base64":"e30"}"#,
            r#"{"version":1,"status":200,"headers":{},"body_base64":"","extra":true}"#,
        ] {
            assert!(decode_response(text.as_bytes()).is_err());
        }
    }
}
