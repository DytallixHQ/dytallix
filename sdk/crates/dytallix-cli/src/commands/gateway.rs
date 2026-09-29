//! The local browser companion (E04 gap 19, C-d; the node's
//! `docs/architecture/client-channel-v1.md`, "Browser companion").
//!
//! Browsers cannot use post-quantum certificates, so there is no public HTTPS
//! endpoint. `dytallix gateway serve` runs on the user's machine at a literal
//! loopback address, which browsers treat as a secure context. It relays the
//! page's JSON-RPC to the pinned chain's endpoint, over the client channel or
//! loopback HTTP. It can serve a wallet bundle whose file manifest the user
//! pinned by digest; the files are read once, at startup.
use std::collections::BTreeMap;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, ensure, Context, Result};
use clap::{Args, Subcommand};
use dytallix_sdk::ordinary_client::CometClient;
use http_body_util::{BodyExt, Full, Limited};
use hyper::body::{Bytes, Incoming};
use hyper::header::{self, HeaderMap, HeaderValue};
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::{TokioIo, TokioTimer};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tokio::net::TcpListener;
use tokio::sync::Semaphore;

use super::consensus::ChainConfig;

/// Bounds of what one gateway holds and accepts.
pub(crate) const MAX_BUNDLE_FILES: usize = 1024;
pub(crate) const MAX_BUNDLE_BYTES: u64 = 64 * 1024 * 1024;
pub(crate) const MAX_REQUEST_BODY: usize = 1_048_576;
const MAX_CONNECTIONS: usize = 16;
const MAX_HEADER_BYTES: usize = 65_536;
/// The relay's own limit is 30 seconds; this bounds the whole connection.
const CONNECTION_DEADLINE: Duration = Duration::from_secs(40);

/// Every response forbids caching, sniffing, framing and cross-origin use.
/// Pages may run their own scripts and WebAssembly and talk only to the
/// gateway.
const CONTENT_SECURITY_POLICY: &str = "default-src 'none'; script-src 'self' 'wasm-unsafe-eval'; \
style-src 'self'; img-src 'self' data:; font-src 'self'; connect-src 'self'; manifest-src 'self'; \
base-uri 'none'; form-action 'none'; frame-ancestors 'none'";

#[derive(Debug, Clone, Args)]
pub struct GatewayArgs {
    #[command(subcommand)]
    pub command: GatewayCommand,
}

#[derive(Debug, Clone, Subcommand)]
pub enum GatewayCommand {
    /// Relay a browser page's JSON-RPC to the pinned chain, on a loopback
    /// address. Open the printed http://127.0.0.1:PORT/ URL in the browser.
    Serve {
        /// A literal loopback IP:PORT, such as 127.0.0.1:4173.
        #[arg(long)]
        listen: String,
        /// A loopback http://IP:PORT node or a remote endpoint's pin file;
        /// defaults to the pinned chain's endpoint.
        #[arg(long)]
        endpoint: Option<String>,
        /// A wallet bundle directory to serve at `/`.
        #[arg(long, requires = "bundle_sha256")]
        bundle: Option<PathBuf>,
        /// The bundle's manifest digest (`dytallix gateway bundle-digest`),
        /// from a source you trust.
        #[arg(long, requires = "bundle")]
        bundle_sha256: Option<String>,
    },
    /// Print a wallet bundle directory's manifest digest.
    BundleDigest {
        /// The bundle directory.
        dir: PathBuf,
    },
}

pub async fn run(args: GatewayArgs) -> Result<()> {
    match args.command {
        GatewayCommand::BundleDigest { dir } => {
            let bundle = Bundle::read(&dir)?;
            println!(
                "{}",
                json!({"bundle_sha256": bundle.digest, "files": bundle.files.len()})
            );
            Ok(())
        }
        GatewayCommand::Serve {
            listen,
            endpoint,
            bundle,
            bundle_sha256,
        } => {
            let address: SocketAddr = listen
                .parse()
                .context("--listen is a literal loopback IP:PORT")?;
            ensure!(
                address.ip().is_loopback(),
                "the gateway listens only on a loopback address"
            );
            let config = ChainConfig::load()?;
            let client = config.client(endpoint.as_deref())?;
            let bundle = match (bundle, bundle_sha256) {
                (Some(dir), Some(expected)) => {
                    let bundle = Bundle::read(&dir)?;
                    ensure!(
                        bundle.digest == expected,
                        "the bundle's manifest digest is {}, not the pinned {expected}; nothing is served",
                        bundle.digest
                    );
                    Some(bundle)
                }
                _ => None,
            };
            let chain = json!({
                "network": config.network,
                "chain_id": config.chain_id,
                "genesis_digest": config.genesis_digest,
                "endpoint": client.endpoint().describe(),
            });
            serve(address, client, chain, bundle).await
        }
    }
}

/// A wallet bundle: every regular file under a directory, read into memory.
pub(crate) struct Bundle {
    pub(crate) files: BTreeMap<String, Vec<u8>>,
    /// The SHA-256 of the manifest: one `<sha256>  <path>\n` line per file,
    /// sorted by path, as `sha256sum` prints it.
    pub(crate) digest: String,
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

impl Bundle {
    pub(crate) fn read(dir: &Path) -> Result<Self> {
        let mut files = BTreeMap::new();
        let mut total = 0u64;
        collect(dir, "", &mut files, &mut total)?;
        ensure!(!files.is_empty(), "the bundle directory holds no files");
        let manifest: String = files
            .iter()
            .map(|(path, bytes)| format!("{}  {path}\n", hex(&Sha256::digest(bytes))))
            .collect();
        Ok(Bundle {
            digest: hex(&Sha256::digest(manifest.as_bytes())),
            files,
        })
    }
}

fn collect(
    dir: &Path,
    prefix: &str,
    files: &mut BTreeMap<String, Vec<u8>>,
    total: &mut u64,
) -> Result<()> {
    let mut entries = std::fs::read_dir(dir)
        .with_context(|| format!("cannot read bundle directory {}", dir.display()))?
        .collect::<std::io::Result<Vec<_>>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| anyhow!("bundle file names must be UTF-8"))?;
        ensure!(
            !name.is_empty() && !name.contains('\\'),
            "invalid bundle file name {name:?}"
        );
        let path = format!("{prefix}{name}");
        let meta = std::fs::symlink_metadata(entry.path())?;
        if meta.is_dir() {
            collect(&entry.path(), &format!("{path}/"), files, total)?;
        } else {
            ensure!(
                meta.is_file(),
                "the bundle may hold only regular files and directories: {path}"
            );
            *total += meta.len();
            ensure!(
                files.len() < MAX_BUNDLE_FILES && *total <= MAX_BUNDLE_BYTES,
                "the bundle exceeds {MAX_BUNDLE_FILES} files or {MAX_BUNDLE_BYTES} bytes"
            );
            files.insert(path, std::fs::read(entry.path())?);
        }
    }
    Ok(())
}

struct State {
    client: CometClient,
    chain: Value,
    /// This gateway's own `IP:PORT`, the only accepted `Host`.
    authority: String,
    /// `http://IP:PORT`, the only accepted `Origin`.
    origin: String,
    bundle: Option<Bundle>,
}

async fn serve(
    address: SocketAddr,
    client: CometClient,
    chain: Value,
    bundle: Option<Bundle>,
) -> Result<()> {
    let listener = TcpListener::bind(address)
        .await
        .with_context(|| format!("cannot listen on {address}"))?;
    let authority = listener.local_addr()?.to_string();
    println!(
        "{}",
        json!({"status": "serving", "url": format!("http://{authority}/"),
            "chain_id": chain["chain_id"], "endpoint": chain["endpoint"],
            "bundle_sha256": bundle.as_ref().map(|b| b.digest.clone())})
    );
    let state = Arc::new(State {
        client,
        chain,
        origin: format!("http://{authority}"),
        authority,
        bundle,
    });
    let capacity = Arc::new(Semaphore::new(MAX_CONNECTIONS));
    loop {
        let (stream, peer) = tokio::select! {
            accepted = listener.accept() => accepted?,
            _ = tokio::signal::ctrl_c() => return Ok(()),
        };
        if !peer.ip().is_loopback() {
            continue;
        }
        let Ok(permit) = capacity.clone().try_acquire_owned() else {
            continue;
        };
        let state = state.clone();
        tokio::spawn(async move {
            let _permit = permit;
            let service = hyper::service::service_fn(move |request| handle(request, state.clone()));
            let mut builder = hyper::server::conn::http1::Builder::new();
            builder
                .timer(TokioTimer::new())
                .keep_alive(false)
                .max_buf_size(MAX_HEADER_BYTES)
                .header_read_timeout(Duration::from_secs(10));
            let _ = tokio::time::timeout(
                CONNECTION_DEADLINE,
                builder.serve_connection(TokioIo::new(stream), service),
            )
            .await;
        });
    }
}

fn respond(status: StatusCode, content_type: &str, body: Vec<u8>) -> Response<Full<Bytes>> {
    let mut response = Response::new(Full::new(Bytes::from(body)));
    *response.status_mut() = status;
    let headers = response.headers_mut();
    for (name, value) in [
        (header::CONTENT_TYPE, content_type),
        (header::CACHE_CONTROL, "no-store"),
        (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
        (header::REFERRER_POLICY, "no-referrer"),
        (header::X_FRAME_OPTIONS, "DENY"),
        (header::CONTENT_SECURITY_POLICY, CONTENT_SECURITY_POLICY),
    ] {
        headers.insert(name, HeaderValue::from_str(value).expect("fixed header"));
    }
    for (name, value) in [
        ("cross-origin-resource-policy", "same-origin"),
        ("cross-origin-opener-policy", "same-origin"),
    ] {
        headers.insert(name, HeaderValue::from_static(value));
    }
    response
}

fn refuse(status: StatusCode, message: &str) -> Response<Full<Bytes>> {
    respond(
        status,
        "application/json",
        json!({"error": message}).to_string().into_bytes(),
    )
}

/// The one value of a header that must appear at most once.
fn single(headers: &HeaderMap, name: header::HeaderName) -> Result<Option<&str>, ()> {
    let mut values = headers.get_all(name).iter();
    let first = values.next();
    if values.next().is_some() {
        return Err(());
    }
    first.map(|v| v.to_str().map_err(|_| ())).transpose()
}

/// Only pages this gateway served may use it. A `Host` other than its own
/// literal address is a rebound DNS name; a foreign `Origin` or a cross-site
/// fetch is another site's page.
fn admitted(headers: &HeaderMap, state: &State) -> Result<(), &'static str> {
    if single(headers, header::HOST) != Ok(Some(state.authority.as_str())) {
        return Err("Host is not this gateway's address");
    }
    match single(headers, header::ORIGIN) {
        Ok(None) => {}
        Ok(Some(origin)) if origin == state.origin => {}
        _ => return Err("Origin is not this gateway"),
    }
    match single(headers, header::HeaderName::from_static("sec-fetch-site")) {
        Ok(None | Some("same-origin" | "none")) => Ok(()),
        _ => Err("cross-site requests are refused"),
    }
}

fn content_type(path: &str) -> &'static str {
    match path.rsplit_once('.').map(|(_, extension)| extension) {
        Some("html") => "text/html; charset=utf-8",
        Some("js" | "mjs") => "text/javascript; charset=utf-8",
        Some("wasm") => "application/wasm",
        Some("css") => "text/css; charset=utf-8",
        Some("json") => "application/json",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("ico") => "image/x-icon",
        Some("txt") => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

async fn handle(
    request: Request<Incoming>,
    state: Arc<State>,
) -> Result<Response<Full<Bytes>>, Infallible> {
    if let Err(reason) = admitted(request.headers(), &state) {
        return Ok(refuse(StatusCode::FORBIDDEN, reason));
    }
    let path = request.uri().path().to_owned();
    Ok(match (request.method(), path.as_str()) {
        (&Method::POST, "/rpc") => relay(request, &state).await,
        (&Method::GET, "/chain") => respond(
            StatusCode::OK,
            "application/json",
            state.chain.to_string().into_bytes(),
        ),
        (&Method::GET, _) => {
            let file = if path == "/" {
                "index.html"
            } else {
                &path[1..]
            };
            match state.bundle.as_ref().and_then(|b| b.files.get(file)) {
                Some(bytes) => respond(StatusCode::OK, content_type(file), bytes.clone()),
                None => refuse(StatusCode::NOT_FOUND, "not found"),
            }
        }
        _ => refuse(StatusCode::METHOD_NOT_ALLOWED, "only GET and POST /rpc"),
    })
}

async fn relay(request: Request<Incoming>, state: &State) -> Response<Full<Bytes>> {
    let json = matches!(
        single(request.headers(), header::CONTENT_TYPE),
        Ok(Some(value)) if value.split(';').next().unwrap_or("").trim().eq_ignore_ascii_case("application/json")
    );
    if !json {
        return refuse(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "JSON-RPC needs application/json",
        );
    }
    let body = match Limited::new(request.into_body(), MAX_REQUEST_BODY)
        .collect()
        .await
    {
        Ok(body) => body.to_bytes().to_vec(),
        Err(_) => return refuse(StatusCode::PAYLOAD_TOO_LARGE, "request body exceeds 1 MiB"),
    };
    let parsed = serde_json::from_slice::<Value>(&body);
    if !matches!(parsed, Ok(Value::Object(_) | Value::Array(_))) {
        return refuse(
            StatusCode::BAD_REQUEST,
            "JSON-RPC needs an object or an array",
        );
    }
    match state.client.relay(body).await {
        Ok(reply) => respond(StatusCode::OK, "application/json", reply),
        Err(error) => refuse(StatusCode::BAD_GATEWAY, &error.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bundle_digest_is_its_sorted_sha256sum_manifest() {
        let dir = std::env::temp_dir().join(format!("dytallix-bundle-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("assets")).unwrap();
        std::fs::write(dir.join("index.html"), b"<p>wallet</p>").unwrap();
        std::fs::write(dir.join("assets/app.js"), b"console.log(1)").unwrap();
        let bundle = Bundle::read(&dir).unwrap();
        let manifest = format!(
            "{}  assets/app.js\n{}  index.html\n",
            hex(&Sha256::digest(b"console.log(1)")),
            hex(&Sha256::digest(b"<p>wallet</p>"))
        );
        assert_eq!(bundle.digest, hex(&Sha256::digest(manifest.as_bytes())));
        assert_eq!(bundle.files.len(), 2);

        // A symlink is refused, and so is an empty directory.
        std::os::unix::fs::symlink(dir.join("index.html"), dir.join("link.html")).unwrap();
        assert!(Bundle::read(&dir).is_err());
        let empty = dir.join("empty");
        std::fs::remove_file(dir.join("link.html")).unwrap();
        std::fs::create_dir_all(&empty).unwrap();
        assert!(Bundle::read(&empty).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn content_types_follow_the_extension() {
        assert_eq!(content_type("index.html"), "text/html; charset=utf-8");
        assert_eq!(content_type("pkg/wallet_bg.wasm"), "application/wasm");
        assert_eq!(content_type("a.js"), "text/javascript; charset=utf-8");
        assert_eq!(content_type("README"), "application/octet-stream");
    }
}
