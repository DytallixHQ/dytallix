//! The public status page (P01, 3 October 2026): a read-only plain-HTTP
//! listener on an endpoint, for a free uptime checker. `GET /status` answers
//! `{"chain_id","height","time"}` from the engine's local `/status` route:
//! 200 while the newest block is at most the configured age old, and 503
//! with the same fields once it is older, so a halted chain or a stuck
//! endpoint shows in the status code alone (monitoring v1; P01, 10 October
//! 2026).
//! It uses no cryptography and accepts no input, so it adds nothing to the
//! PQC-only boundary; it is unauthenticated, a liveness hint and never a
//! source of chain state. Every other request gets a fixed error.

use crate::{engine, Limits, Reply, LOCAL_CALLER};
use bytes::Bytes;
use http_body_util::Full;
use hyper::{body::Incoming, header, Method, Request, Response, StatusCode};
use hyper_util::rt::{TokioIo, TokioTimer};
use std::{
    convert::Infallible,
    net::SocketAddr,
    path::PathBuf,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::{net::TcpListener, sync::Semaphore, time::Instant};

/// Fixed limits: the page takes no request body and answers a few bytes.
pub const MAX_CONNECTIONS: usize = 8;
pub const DEADLINE: Duration = Duration::from_secs(2);
const MAX_HEADERS: usize = 16;
const MAX_HEADER_BYTES: usize = crate::MIN_HEADER_BYTES;
const MAX_ENGINE_RESPONSE: usize = 65_536;
/// Bounds of the head age a stale head is judged by.
pub const MAX_HEAD_AGE: std::ops::RangeInclusive<u64> = 1..=3600;

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

/// Seconds since the Unix epoch of an RFC 3339 UTC time as the engine
/// writes it (`2027-01-07T14:00:05.123456789Z`), or None.
pub(crate) fn unix_seconds(time: &str) -> Option<i64> {
    let (date, rest) = time.split_once('T')?;
    let rest = rest.strip_suffix('Z')?;
    let clock = match rest.split_once('.') {
        Some((whole, fraction))
            if !fraction.is_empty() && fraction.bytes().all(|b| b.is_ascii_digit()) =>
        {
            whole
        }
        Some(_) => return None,
        None => rest,
    };
    let number = |text: &str, digits: usize| -> Option<i64> {
        if text.len() == digits && text.bytes().all(|b| b.is_ascii_digit()) {
            text.parse().ok()
        } else {
            None
        }
    };
    let date: Vec<&str> = date.split('-').collect();
    let clock: Vec<&str> = clock.split(':').collect();
    let ([year, month, day], [hour, minute, second]) = (date.as_slice(), clock.as_slice()) else {
        return None;
    };
    let (year, month, day) = (number(year, 4)?, number(month, 2)?, number(day, 2)?);
    let (hour, minute, second) = (number(hour, 2)?, number(minute, 2)?, number(second, 2)?);
    if !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || hour > 23
        || minute > 59
        || second > 60
    {
        return None;
    }
    // Days from the civil date (H. Hinnant's days_from_civil).
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let year_of_era = y - era * 400;
    let day_of_year = (153 * ((month + 9) % 12) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    let days = era * 146_097 + day_of_era - 719_468;
    Some(days * 86_400 + hour * 3_600 + minute * 60 + second)
}

/// The page's answer at `now` (Unix seconds): 200 with the three fields
/// while the newest block is at most `max_age` old, 503 with the same fields
/// once it is older or its time is unreadable, and 503 when the engine does
/// not answer. A block time ahead of the clock counts as fresh.
pub(crate) fn answer(
    reply: &Reply,
    now: i64,
    max_age: Duration,
) -> (StatusCode, serde_json::Value) {
    let Some(body) = summarize(reply) else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            serde_json::json!({"error": "Engine status unavailable"}),
        );
    };
    let max_age = i64::try_from(max_age.as_secs()).unwrap_or(i64::MAX);
    let fresh = body["time"]
        .as_str()
        .and_then(unix_seconds)
        .is_some_and(|time| now.saturating_sub(time) <= max_age);
    let status = if fresh {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };
    (status, body)
}

async fn handle(
    request: Request<Incoming>,
    socket: Arc<PathBuf>,
    max_age: Duration,
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
    // The engine sees the adapter as a local caller.
    let reply = engine("GET", "/status", "", b"", LOCAL_CALLER, &socket, &limits).await;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| {
            i64::try_from(since.as_secs()).unwrap_or(i64::MAX)
        });
    let (status, body) = answer(&reply, now, max_age);
    Ok(fixed(status, body))
}

/// Serves the status page: one request per connection, at most
/// `MAX_CONNECTIONS` at once, each within `DEADLINE`. A head older than
/// `max_age` answers 503.
pub async fn serve(
    listener: TcpListener,
    socket: Arc<PathBuf>,
    max_age: Duration,
) -> Result<(), String> {
    let capacity = Arc::new(Semaphore::new(MAX_CONNECTIONS));
    loop {
        let (stream, _) = listener
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
            let service =
                hyper::service::service_fn(move |req| handle(req, socket.clone(), max_age));
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

    #[test]
    fn the_engines_block_time_reads_as_unix_seconds() {
        for (time, seconds) in [
            ("1970-01-01T00:00:00Z", 0),
            ("2027-01-07T14:00:05Z", 1_799_330_405),
            ("2027-01-07T14:00:05.123456789Z", 1_799_330_405),
            ("2000-02-29T23:59:59Z", 951_868_799),
        ] {
            assert_eq!(unix_seconds(time), Some(seconds), "{time}");
        }
        for bad in [
            "",
            "t",
            "2027-01-07 14:00:05Z",
            "2027-01-07T14:00:05",
            "2027-01-07T14:00:05+00:00",
            "2027-01-07T14:00:05.Z",
            "2027-1-07T14:00:05Z",
            "2027-13-07T14:00:05Z",
            "2027-01-07T24:00:05Z",
            "2027-01-07T14:00Z",
            "2027-01-07-01T14:00:05Z",
        ] {
            assert_eq!(unix_seconds(bad), None, "{bad}");
        }
    }

    #[test]
    fn a_stale_head_answers_503_with_the_same_fields() {
        let engine = |time: &str| {
            reply(
                200,
                &serde_json::json!({"result":{"node_info":{"network":"c"},"sync_info":{
                    "latest_block_height":"9","latest_block_time":time}}})
                .to_string(),
            )
        };
        let block = 1_799_330_405;
        let fields = serde_json::json!({"chain_id":"c","height":9,"time":"2027-01-07T14:00:05Z"});
        let age = Duration::from_secs(60);
        let fresh = engine("2027-01-07T14:00:05Z");
        for (now, status) in [
            (block - 30, StatusCode::OK), // a block time ahead of the clock
            (block, StatusCode::OK),
            (block + 60, StatusCode::OK),
            (block + 61, StatusCode::SERVICE_UNAVAILABLE),
            (block + 86_400, StatusCode::SERVICE_UNAVAILABLE),
        ] {
            assert_eq!(answer(&fresh, now, age), (status, fields.clone()), "{now}");
        }
        // An unreadable block time is not fresh.
        let (status, _) = answer(&engine("soon"), block, age);
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(
            answer(&reply(502, "{}"), block, age),
            (
                StatusCode::SERVICE_UNAVAILABLE,
                serde_json::json!({"error": "Engine status unavailable"})
            )
        );
    }

    /// A private home with `data/` and an engine stand-in on `data/rpc.sock`
    /// that answers `/status` like the engine, removed on drop.
    struct Home(PathBuf);
    impl Drop for Home {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn home(label: &str, engine_time: Option<&'static str>) -> Home {
        use std::os::unix::fs::PermissionsExt;
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let path = std::env::temp_dir().join(format!("dyt-st-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(path.join("data")).unwrap();
        let path = path.canonicalize().unwrap();
        for dir in [path.clone(), path.join("data")] {
            std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        if let Some(time) = engine_time {
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
                    // A local caller, never the checker's own address.
                    assert_eq!(request["remote_addr"], "127.0.0.1:1");
                    let body = serde_json::json!({"jsonrpc":"2.0","id":-1,"result":{"node_info":{"network":"dytallix-staging-1"},"sync_info":{"latest_block_height":"7","latest_block_time":time,"catching_up":false}}});
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
        // A head ahead of the clock is fresh; one from 2000 is stale.
        for (label, engine_time) in [
            ("up", Some("2027-01-07T14:00:35Z")),
            ("stale", Some("2000-02-29T23:59:59Z")),
            ("down", None),
        ] {
            let home = home(label, engine_time);
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let socket = Arc::new(home.0.join("data/rpc.sock"));
            tokio::spawn(serve(listener, socket, Duration::from_secs(60)));
            let (status, body) = request(address, "GET /status HTTP/1.1").await;
            if let Some(time) = engine_time {
                assert_eq!(status, if label == "up" { 200 } else { 503 });
                assert_eq!(
                    body,
                    serde_json::json!({"chain_id":"dytallix-staging-1","height":7,"time":time})
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
