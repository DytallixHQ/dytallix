//! The client channel endpoint (E04 gap 19, C-b;
//! `docs/architecture/client-channel-v1.md`).
//!
//! Remote clients reach the engine's client socket through the post-quantum
//! channel instead of TLS. Each connection carries one handshake, one
//! request and one response, under the adapter's deadline and bounds, and
//! goes to the same engine path as the loopback HTTP listener.

use dytallix_client_channel::{
    endpoint_offer, fingerprint, handshake_payload_len, HandshakeType, Identity, MessageReader,
    Method, Request, Response, Session, HANDSHAKE_HEADER_LEN, MAX_PATH, MAX_QUERY,
    MAX_REQUEST_MESSAGE, SEED_LEN,
};
use hyper::StatusCode;
use std::{
    collections::HashMap,
    net::{IpAddr, SocketAddr},
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    net::TcpListener,
    sync::Semaphore,
    time::Instant,
};
use zeroize::Zeroizing;

use crate::{engine, own_metadata, valid_rpc_path, Limits, Reply};

/// Compiled ceilings; operator flags can only lower them. They are
/// prototype bounds: D12-Q01 sets production capacity and rates.
pub const MAX_CHANNEL_CONNECTIONS: usize = 64;
pub const MAX_CHANNEL_CONNECTIONS_PER_ADDRESS: usize = 4;
/// The endpoint's key seed, relative to the node home. It is a role key of
/// its own; the adapter refuses the peer transport's seed.
pub const SEED_PATH: &str = "config/client_channel_seed.bin";
const PEER_SEED_PATH: &str = "config/pqc_peer_seed.bin";

/// The listener's settings, with the identity derived from the seed file.
pub struct ChannelConfig {
    pub listen: SocketAddr,
    pub network: String,
    pub max_connections: usize,
    pub max_per_address: usize,
    identity: Arc<Identity>,
}

impl ChannelConfig {
    /// The SHA-256 of the endpoint's public key, as clients' pins show it.
    pub fn fingerprint(&self) -> String {
        fingerprint(self.identity.public_key())
    }
}

impl std::fmt::Debug for ChannelConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChannelConfig")
            .field("listen", &self.listen)
            .field("network", &self.network)
            .field("max_connections", &self.max_connections)
            .field("max_per_address", &self.max_per_address)
            .field("key_sha256", &self.fingerprint())
            .finish()
    }
}

/// The channel flags as parsed, before the seed is read.
#[derive(Default)]
pub(crate) struct ChannelArgs {
    listen: Option<SocketAddr>,
    network: Option<String>,
    max_connections: Option<usize>,
    max_per_address: Option<usize>,
}

impl ChannelArgs {
    pub(crate) const FLAGS: [&'static str; 4] = [
        "--channel-listen",
        "--channel-network",
        "--max-channel-connections",
        "--max-channel-connections-per-address",
    ];

    pub(crate) fn set(&mut self, flag: &str, value: Option<String>) -> Result<(), String> {
        let value = value.ok_or("Missing channel argument value")?;
        let lowered = |ceiling: usize| -> Result<usize, String> {
            let value: usize = value.parse().map_err(|_| "Limits are decimal integers")?;
            if !(1..=ceiling).contains(&value) {
                return Err("A limit can only be lowered from its ceiling".into());
            }
            Ok(value)
        };
        match flag {
            "--channel-listen" => {
                let listen: SocketAddr = value.parse().map_err(|_| "Use a numeric IP:port")?;
                if listen.ip().is_unspecified() || listen.ip().is_multicast() || listen.port() == 0
                {
                    return Err("The channel listener needs an explicit address and port".into());
                }
                self.listen = Some(listen);
            }
            "--channel-network" => {
                let valid = (1..=dytallix_client_channel::MAX_NETWORK_LEN).contains(&value.len())
                    && value.bytes().all(|b| (0x21..=0x7e).contains(&b));
                if !valid {
                    return Err("The channel network is the chain ID".into());
                }
                self.network = Some(value);
            }
            "--max-channel-connections" => {
                self.max_connections = Some(lowered(MAX_CHANNEL_CONNECTIONS)?)
            }
            "--max-channel-connections-per-address" => {
                self.max_per_address = Some(lowered(MAX_CHANNEL_CONNECTIONS_PER_ADDRESS)?)
            }
            _ => return Err("Unknown or duplicate argument".into()),
        }
        Ok(())
    }

    /// Reads the seed only when the listener is configured.
    pub(crate) fn finish(self, home: &Path) -> Result<Option<ChannelConfig>, String> {
        let (listen, network) = match (self.listen, self.network) {
            (None, None) if self.max_connections.is_none() && self.max_per_address.is_none() => {
                return Ok(None)
            }
            (Some(listen), Some(network)) => (listen, network),
            _ => return Err("The channel listener needs both its address and its network".into()),
        };
        let seed = read_seed(&home.join(SEED_PATH))?;
        if let Ok(peer) = std::fs::read(home.join(PEER_SEED_PATH)) {
            if peer.as_slice() == seed.as_slice() {
                return Err("The channel key must not be the peer transport key".into());
            }
        }
        Ok(Some(ChannelConfig {
            listen,
            network,
            max_connections: self.max_connections.unwrap_or(MAX_CHANNEL_CONNECTIONS),
            max_per_address: self
                .max_per_address
                .unwrap_or(MAX_CHANNEL_CONNECTIONS_PER_ADDRESS),
            identity: Arc::new(Identity::from_seed(&seed)),
        }))
    }
}

/// Reads an endpoint key seed: a regular owner-only (0600) file of the
/// service user with one link and exactly 32 bytes.
pub fn read_seed(path: &Path) -> Result<Zeroizing<[u8; SEED_LEN]>, String> {
    let meta = own_metadata(path).map_err(|_| "Channel key seed is unavailable")?;
    if !meta.is_file() || meta.mode() & 0o777 != 0o600 || meta.nlink() != 1 {
        return Err("Channel key seed must be a regular owner-only file".into());
    }
    let bytes = Zeroizing::new(std::fs::read(path).map_err(|_| "Channel key seed is unreadable")?);
    let mut seed = Zeroizing::new([0u8; SEED_LEN]);
    if bytes.len() != SEED_LEN {
        return Err("Channel key seed must be 32 bytes".into());
    }
    seed.copy_from_slice(&bytes);
    Ok(seed)
}

/// Open connections per client address.
pub(crate) struct AddressSlots {
    counts: Mutex<HashMap<IpAddr, usize>>,
    max: usize,
}

pub(crate) struct AddressSlot {
    slots: Arc<AddressSlots>,
    ip: IpAddr,
}

impl AddressSlots {
    pub(crate) fn new(max: usize) -> Arc<Self> {
        Arc::new(AddressSlots {
            counts: Mutex::new(HashMap::new()),
            max,
        })
    }

    pub(crate) fn acquire(self: &Arc<Self>, ip: IpAddr) -> Option<AddressSlot> {
        let mut counts = self.counts.lock().ok()?;
        let count = counts.entry(ip).or_insert(0);
        if *count >= self.max {
            return None;
        }
        *count += 1;
        Some(AddressSlot {
            slots: self.clone(),
            ip,
        })
    }
}

impl Drop for AddressSlot {
    fn drop(&mut self) {
        if let Ok(mut counts) = self.slots.counts.lock() {
            if let Some(count) = counts.get_mut(&self.ip) {
                *count -= 1;
                if *count == 0 {
                    counts.remove(&self.ip);
                }
            }
        }
    }
}

/// Accepts channel connections. A connection over either bound is closed
/// at once; each exchange ends at the adapter's deadline.
pub(crate) async fn serve(
    listener: TcpListener,
    config: ChannelConfig,
    socket: Arc<PathBuf>,
    limits: Limits,
) -> Result<(), String> {
    let capacity = Arc::new(Semaphore::new(config.max_connections));
    let addresses = AddressSlots::new(config.max_per_address);
    let network = Arc::new(config.network);
    loop {
        let (mut stream, peer) = listener
            .accept()
            .await
            .map_err(|_| "Channel listener failed")?;
        let Ok(permit) = capacity.clone().try_acquire_owned() else {
            continue;
        };
        let Some(slot) = addresses.acquire(peer.ip()) else {
            continue;
        };
        let (identity, network, socket) =
            (config.identity.clone(), network.clone(), socket.clone());
        tokio::spawn(async move {
            let _held = (permit, slot);
            let deadline = Instant::now() + limits.deadline;
            // Dropping the exchange closes the IPC stream and cancels the
            // engine request, as the HTTP listener does.
            let _ = tokio::time::timeout_at(
                deadline,
                exchange(&mut stream, &identity, &network, peer, &socket, &limits),
            )
            .await;
        });
    }
}

async fn read_vec<S: AsyncRead + Unpin>(stream: &mut S, len: usize) -> Result<Vec<u8>, ()> {
    let mut buffer = vec![0; len];
    stream.read_exact(&mut buffer).await.map_err(|_| ())?;
    Ok(buffer)
}

async fn read_handshake<S: AsyncRead + Unpin>(
    stream: &mut S,
    kind: HandshakeType,
) -> Result<Vec<u8>, ()> {
    let mut header = [0u8; HANDSHAKE_HEADER_LEN];
    stream.read_exact(&mut header).await.map_err(|_| ())?;
    let len = handshake_payload_len(&header, kind).map_err(|_| ())?;
    read_vec(stream, len).await
}

/// One connection: handshake, request, engine, response. Any failure closes
/// the connection without a reply, since nothing can be sealed before the
/// handshake completes.
pub(crate) async fn exchange<S: AsyncRead + AsyncWrite + Unpin>(
    stream: &mut S,
    identity: &Identity,
    network: &str,
    peer: SocketAddr,
    socket: &Path,
    limits: &Limits,
) -> Result<(), ()> {
    let hello = read_handshake(stream, HandshakeType::Hello).await?;
    let (pending, offer) = endpoint_offer(identity, network, &hello).map_err(|_| ())?;
    stream.write_all(&offer).await.map_err(|_| ())?;
    let finish = read_handshake(stream, HandshakeType::Finish).await?;
    let Session { mut sealer, opener } = pending.finish(&finish).map_err(|_| ())?;

    // The request's bound follows the adapter's lowered body limit.
    let bound =
        (2 + 2 + MAX_PATH + 2 + MAX_QUERY + 4 + limits.max_request_body).min(MAX_REQUEST_MESSAGE);
    let mut reader = MessageReader::new(opener, bound);
    let message = loop {
        let bytes = read_vec(stream, reader.wanted()).await?;
        if let Some(message) = reader.feed(&bytes).map_err(|_| ())? {
            break message;
        }
    };
    let reply = match Request::decode(&message) {
        Ok(request) => respond(request, peer, socket, limits).await,
        Err(_) => Reply::error(StatusCode::BAD_REQUEST, "Malformed channel request"),
    };
    let response = channel_response(reply)
        .or_else(|_| {
            channel_response(Reply::error(
                StatusCode::BAD_GATEWAY,
                "Engine RPC response failed",
            ))
        })
        .map_err(|_| ())?;
    let sealed = sealer.seal_message(&response).map_err(|_| ())?;
    stream.write_all(&sealed).await.map_err(|_| ())?;
    stream.shutdown().await.map_err(|_| ())
}

/// The HTTP listener's request checks, in channel form.
async fn respond(request: Request, peer: SocketAddr, socket: &Path, limits: &Limits) -> Reply {
    if !valid_rpc_path(&request.path) {
        return Reply::error(StatusCode::BAD_REQUEST, "Unsupported request target");
    }
    if request.method == Method::Get && request.path == "/" {
        return Reply::error(
            StatusCode::NOT_IMPLEMENTED,
            "RPC route is unsupported in this profile",
        );
    }
    if request.body.len() > limits.max_request_body {
        return Reply::error(StatusCode::PAYLOAD_TOO_LARGE, "Request body exceeds limit");
    }
    let method = match request.method {
        Method::Get => "GET",
        Method::Post => "POST",
    };
    engine(
        method,
        &request.path,
        &request.query,
        &request.body,
        peer,
        socket,
        limits,
    )
    .await
}

fn channel_response(reply: Reply) -> Result<Vec<u8>, dytallix_client_channel::Error> {
    Response {
        status: reply.status,
        content_type: reply.content_type.unwrap_or_default(),
        cache_control: reply.cache_control.unwrap_or_default(),
        body: reply.body,
    }
    .encode()
}

#[cfg(test)]
#[path = "channel_tests.rs"]
mod tests;
