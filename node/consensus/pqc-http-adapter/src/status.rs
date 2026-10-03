//! The public status page (P01, 3 October 2026): a read-only plain-HTTP
//! listener on an endpoint, for a free uptime checker. `GET /status` answers
//! `{"chain_id","height","time"}` from the engine's local `/status` route.
//! It uses no cryptography and accepts no input, so it adds nothing to the
//! PQC-only boundary; it is unauthenticated, a liveness hint and never a
//! source of chain state. Every other request gets a fixed error.

use crate::{engine, Limits, Reply};
use bytes::Bytes;
use http_body_util::Full;
use hyper::{body::Incoming, header, Method, Request, Response, StatusCode};
use hyper_util::rt::{TokioIo, TokioTimer};
use std::{convert::Infallible, net::SocketAddr, path::PathBuf, sync::Arc, time::Duration};
use tokio::{net::TcpListener, sync::Semaphore, time::Instant};

/// Fixed limits: the page takes no request body and answers a few bytes.
pub const MAX_CONNECTIONS: usize = 8;
pub const DEADLINE: Duration = Duration::from_secs(2);
const MAX_HEADERS: usize = 16;
const MAX_HEADER_BYTES: usize = crate::MIN_HEADER_BYTES;
const MAX_ENGINE_RESPONSE: usize = 65_536;

/// Checks a status listener address: an explicit address and port, never the
/// adapter's loopback listener. A production build needs a routable address,
/// since the page exists for an outside checker.
pub fn check_listen(listen: SocketAddr, adapter: SocketAddr) -> Result<(), String> {
    let ip = listen.ip();
    if ip.is_unspecified() || ip.is_multicast() || listen.port() == 0 || listen == adapter {
        return Err("The status listener needs its own explicit address and port".into());
    }
    if cfg!(feature = "production") && (ip.is_loopback() || link_local(ip)) {
        return Err("A production status listener needs a routable address".into());
    }
    Ok(())
}

fn link_local(ip: std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(v4) => v4.is_link_local(),
        std::net::IpAddr::V6(v6) => (v6.segments()[0] & 0xffc0) == 0xfe80,
    }
}

fn fixed(status: StatusCode, body: serde_json::Value) -> Response<Full<Bytes>> {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::CACHE_CONTROL, "no-store")
        .body(Full::new(Bytes::from(body.to_string())))
        .expect("fixed status response")
}

/// The three public fields of the engine's `/status` answer, or None.
pub(crate) fn summarize(reply: &Reply) -> Option<serde_json::Value> {
    if reply.status != 200 {
        return None;
    }
    let value: serde_json::Value = serde_json::from_slice(&reply.body).ok()?;
    let result = value.get("result")?;
    let chain_id = result.get("node_info")?.get("network")?.as_str()?;
    let sync = result.get("sync_info")?;
    let height: u64 = sync.get("latest_block_height")?.as_str()?.parse().ok()?;
    let time = sync.get("latest_block_time")?.as_str()?;
    Some(serde_json::json!({"chain_id": chain_id, "height": height, "time": time}))
}

async fn handle(
    request: Request<Incoming>,
    peer: SocketAddr,
    socket: Arc<PathBuf>,
) -> Result<Response<Full<Bytes>>, Infallible> {
    if request.uri().path() != "/status" || request.uri().query().is_some() {
        return Ok(fixed(
            StatusCode::NOT_FOUND,
            serde_json::json!({"error": "Not found"}),
        ));
    }
    if request.method() != Method::GET {
        return Ok(fixed(
            StatusCode::METHOD_NOT_ALLOWED,
            serde_json::json!({"error": "Only GET"}),
        ));
    }
    let limits = Limits {
        max_connections: MAX_CONNECTIONS,
        max_request_body: 1,
        max_response_body: MAX_ENGINE_RESPONSE,
        max_headers: MAX_HEADERS,
        max_header_bytes: MAX_HEADER_BYTES,
        deadline: DEADLINE,
    };
    let reply = engine("GET", "/status", "", b"", peer, &socket, &limits).await;
    Ok(match summarize(&reply) {
        Some(body) => fixed(StatusCode::OK, body),
        None => fixed(
            StatusCode::SERVICE_UNAVAILABLE,
            serde_json::json!({"error": "Engine status unavailable"}),
        ),
    })
}

/// Serves the status page: one request per connection, at most
/// `MAX_CONNECTIONS` at once, each within `DEADLINE`.
pub async fn serve(listener: TcpListener, socket: Arc<PathBuf>) -> Result<(), String> {
    let capacity = Arc::new(Semaphore::new(MAX_CONNECTIONS));
    loop {
        let (stream, peer) = listener
            .accept()
            .await
            .map_err(|_| "Status listener failed")?;
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

    fn reply(status: u16, body: &str) -> Reply {
        Reply {
            status,
            content_type: Some("application/json".into()),
            cache_control: None,
            body: body.as_bytes().to_vec(),
        }
    }

    #[test]
    fn the_page_keeps_three_public_fields() {
        let engine = r#"{"jsonrpc":"2.0","id":-1,"result":{"node_info":{"network":"dytallix-staging-1","id":"x"},"sync_info":{"latest_block_height":"42","latest_block_time":"2027-01-07T14:00:05Z","latest_app_hash":"AB","catching_up":false},"validator_info":{"address":"y"}}}"#;
        assert_eq!(
            summarize(&reply(200, engine)).unwrap(),
            serde_json::json!({"chain_id":"dytallix-staging-1","height":42,"time":"2027-01-07T14:00:05Z"})
        );
        for bad in [
            reply(500, engine),
            reply(200, "not json"),
            reply(
                200,
                r#"{"result":{"node_info":{"network":"c"},"sync_info":{"latest_block_height":"x","latest_block_time":"t"}}}"#,
            ),
            reply(
                200,
                r#"{"result":{"sync_info":{"latest_block_height":"1","latest_block_time":"t"}}}"#,
            ),
        ] {
            assert!(summarize(&bad).is_none());
        }
    }

    /// A private home with `data/` and an engine stand-in on `data/rpc.sock`
    /// that answers `/status` like the engine, removed on drop.
    struct Home(PathBuf);
    impl Drop for Home {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn home(label: &str, with_engine: bool) -> Home {
        use std::os::unix::fs::PermissionsExt;
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let path = std::env::temp_dir().join(format!("dyt-st-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(path.join("data")).unwrap();
        let path = path.canonicalize().unwrap();
        for dir in [path.clone(), path.join("data")] {
            std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        if with_engine {
            let socket = path.join("data/rpc.sock");
            let listener = tokio::net::UnixListener::bind(&socket).unwrap();
            std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600)).unwrap();
            tokio::spawn(async move {
                loop {
                    let (mut stream, _) = listener.accept().await.unwrap();
                    let len = stream.read_u32().await.unwrap() as usize;
                    let mut frame = vec![0; len];
                    stream.read_exact(&mut frame).await.unwrap();
                    let request: serde_json::Value = serde_json::from_slice(&frame).unwrap();
                    assert_eq!(
                        (request["method"].as_str(), request["path"].as_str()),
                        (Some("GET"), Some("/status"))
                    );
                    let body = serde_json::json!({"jsonrpc":"2.0","id":-1,"result":{"node_info":{"network":"dytallix-staging-1"},"sync_info":{"latest_block_height":"7","latest_block_time":"2027-01-07T14:00:35Z","catching_up":false}}});
                    use base64::Engine;
                    let reply = serde_json::to_vec(&serde_json::json!({"version":1,"status":200,"headers":{"Content-Type":"application/json"},"body_base64":base64::engine::general_purpose::STANDARD.encode(body.to_string())})).unwrap();
                    stream.write_u32(reply.len() as u32).await.unwrap();
                    stream.write_all(&reply).await.unwrap();
                }
            });
        }
        Home(path)
    }

    async fn request(address: SocketAddr, line: &str) -> (u16, serde_json::Value) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let mut stream = tokio::net::TcpStream::connect(address).await.unwrap();
        stream
            .write_all(format!("{line}\r\nHost: checker\r\nConnection: close\r\n\r\n").as_bytes())
            .await
            .unwrap();
        let mut raw = Vec::new();
        stream.read_to_end(&mut raw).await.unwrap();
        let text = String::from_utf8(raw).unwrap();
        let status = text[9..12].parse().unwrap();
        let body = text.split("\r\n\r\n").nth(1).unwrap();
        (status, serde_json::from_str(body).unwrap())
    }

    #[tokio::test]
    async fn the_page_answers_only_get_status() {
        for (label, with_engine) in [("up", true), ("down", false)] {
            let home = home(label, with_engine);
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            tokio::spawn(serve(listener, Arc::new(home.0.join("data/rpc.sock"))));
            let (status, body) = request(address, "GET /status HTTP/1.1").await;
            if with_engine {
                assert_eq!(status, 200);
                assert_eq!(
                    body,
                    serde_json::json!({"chain_id":"dytallix-staging-1","height":7,"time":"2027-01-07T14:00:35Z"})
                );
            } else {
                assert_eq!(
                    (status, body),
                    (
                        503,
                        serde_json::json!({"error":"Engine status unavailable"})
                    )
                );
            }
            for (line, code) in [
                ("POST /status HTTP/1.1", 405),
                ("GET /status?x=1 HTTP/1.1", 404),
                ("GET /abci_query HTTP/1.1", 404),
                ("GET / HTTP/1.1", 404),
            ] {
                assert_eq!(request(address, line).await.0, code, "{line}");
            }
        }
    }

    #[test]
    fn the_listener_has_its_own_explicit_address() {
        let adapter: SocketAddr = "127.0.0.1:26658".parse().unwrap();
        for bad in [
            "0.0.0.0:8080",
            "[::]:8080",
            "203.0.113.11:0",
            "224.0.0.1:8080",
            "127.0.0.1:26658",
        ] {
            assert!(
                check_listen(bad.parse().unwrap(), adapter).is_err(),
                "{bad}"
            );
        }
        check_listen("203.0.113.11:8080".parse().unwrap(), adapter).unwrap();
        // Loopback serves local tests in a development build only.
        let local = check_listen("127.0.0.1:8080".parse().unwrap(), adapter);
        assert_eq!(local.is_ok(), !cfg!(feature = "production"));
        assert_eq!(
            check_listen("169.254.1.1:8080".parse().unwrap(), adapter).is_ok(),
            !cfg!(feature = "production")
        );
    }
}
