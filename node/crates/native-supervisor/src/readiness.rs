//! Bounded development readiness probes. A response does not grant release
//! authority. The caller must retain and monitor the actual owned child handles.
use crate::config::ProcessLimits;
use crate::processes::Role;
use crate::startup_diagnostic::{phase, Stage, ReadinessExpired};
use anyhow::{bail, ensure, Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use dytallix_fast_node::consensus_settlement::Info;
use serde::{
    de::{MapAccess, SeqAccess, Visitor},
    Deserialize, Deserializer,
};
use serde_json::{Map, Value};
use std::collections::BTreeSet;
use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, TcpStream};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{FileTypeExt, MetadataExt};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::{Duration, Instant};

const IPC_FRAME_LIMIT: usize = 2_097_152;
const HTTP_HEADER_LIMIT: usize = 16_384;
const HTTP_HEADER_COUNT: usize = 64;
const FD_ENTRY_LIMIT: usize = 1024;
const NET_ROW_LIMIT: usize = 4096;

#[derive(Clone, Debug)]
pub struct EngineReady {
    chain_id: String,
    block_height: u64,
    application: Info,
    engine_pid: u32,
}
impl EngineReady {
    pub fn chain_id(&self) -> &str {
        &self.chain_id
    }
    pub fn block_height(&self) -> u64 {
        self.block_height
    }
    pub fn minimum_height(&self) -> u64 {
        self.block_height
    }
    pub fn application_info(&self) -> &Info {
        &self.application
    }
    pub fn engine_pid(&self) -> u32 {
        self.engine_pid
    }
}
#[derive(Clone, Debug)]
pub struct AdapterReady {
    chain_id: String,
    block_height: u64,
    adapter_pid: u32,
}
impl AdapterReady {
    pub fn chain_id(&self) -> &str {
        &self.chain_id
    }
    pub fn block_height(&self) -> u64 {
        self.block_height
    }
    pub fn adapter_pid(&self) -> u32 {
        self.adapter_pid
    }
    pub fn listener_identity_bound(&self) -> bool {
        true
    }
}

struct Deadline<'a> {
    end: Instant,
    poll: Duration,
    health: &'a mut dyn FnMut() -> Result<()>,
}
impl<'a> Deadline<'a> {
    fn new(limits: &ProcessLimits, health: &'a mut dyn FnMut() -> Result<()>) -> Result<Self> {
        let bounds = limits.bounds()?;
        Ok(Self {
            end: Instant::now()
                .checked_add(bounds.startup_timeout)
                .context("Readiness deadline overflow")?,
            poll: bounds.poll_interval,
            health,
        })
    }
    fn check(&mut self) -> Result<()> {
        (self.health)()?;
        ensure!(Instant::now() < self.end, ReadinessExpired);
        Ok(())
    }
    fn quantum(&mut self) -> Result<Duration> {
        self.check()?;
        Ok(self
            .poll
            .min(self.end.saturating_duration_since(Instant::now())))
    }
    fn wait(&mut self, fd: Option<RawFd>, events: i16) -> Result<()> {
        let duration = self.quantum()?;
        let millis = duration
            .as_millis()
            .saturating_add(u128::from(duration.subsec_nanos() % 1_000_000 != 0))
            .min(i32::MAX as u128) as i32;
        let mut request = libc::pollfd {
            fd: fd.unwrap_or(-1),
            events,
            revents: 0,
        };
        let result = unsafe { libc::poll(&mut request, 1, millis.max(1)) };
        if result < 0 && std::io::Error::last_os_error().kind() != std::io::ErrorKind::Interrupted {
            return Err(std::io::Error::last_os_error().into());
        }
        ensure!(
            request.revents & libc::POLLNVAL == 0,
            "Readiness descriptor is invalid"
        );
        self.check()
    }
}
fn write_all<S: Write + AsRawFd>(
    stream: &mut S,
    mut bytes: &[u8],
    deadline: &mut Deadline<'_>,
) -> Result<()> {
    while !bytes.is_empty() {
        deadline.check()?;
        match stream.write(bytes) {
            Ok(0) => bail!("Readiness write closed"),
            Ok(n) => bytes = &bytes[n..],
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                deadline.wait(Some(stream.as_raw_fd()), libc::POLLOUT)?
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => (),
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}
fn read_some<S: Read + AsRawFd>(
    stream: &mut S,
    bytes: &mut [u8],
    deadline: &mut Deadline<'_>,
) -> Result<usize> {
    loop {
        deadline.check()?;
        match stream.read(bytes) {
            Ok(n) => return Ok(n),
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                deadline.wait(Some(stream.as_raw_fd()), libc::POLLIN)?
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => (),
            Err(e) => return Err(e.into()),
        }
    }
}
fn read_exact<S: Read + AsRawFd>(
    stream: &mut S,
    mut bytes: &mut [u8],
    deadline: &mut Deadline<'_>,
) -> Result<()> {
    while !bytes.is_empty() {
        let n = read_some(stream, bytes, deadline)?;
        ensure!(n > 0, "Readiness response ended early");
        bytes = &mut bytes[n..];
    }
    Ok(())
}
fn require_eof<S: Read + AsRawFd>(stream: &mut S, deadline: &mut Deadline<'_>) -> Result<()> {
    let mut byte = [0u8; 1];
    ensure!(
        read_some(stream, &mut byte, deadline)? == 0,
        "Trailing readiness response bytes"
    );
    Ok(())
}

fn connect_unix(path: &Path, deadline: &mut Deadline<'_>) -> Result<UnixStream> {
    deadline.check()?;
    ensure!(
        path.is_absolute() && path.canonicalize()?.as_os_str() == path.as_os_str(),
        "RPC socket path must be canonical"
    );
    let mut address: libc::sockaddr_un = unsafe { std::mem::zeroed() };
    let path_bytes = path.as_os_str().as_bytes();
    ensure!(
        !path_bytes.contains(&0) && path_bytes.len() < address.sun_path.len(),
        "RPC socket path exceeds native limit"
    );
    address.sun_family = libc::AF_UNIX as libc::sa_family_t;
    for (slot, byte) in address.sun_path.iter_mut().zip(path_bytes) {
        *slot = *byte as libc::c_char;
    }
    let length = std::mem::offset_of!(libc::sockaddr_un, sun_path) + path_bytes.len() + 1;
    #[cfg(any(target_os = "macos", target_os = "ios", target_os = "freebsd"))]
    {
        address.sun_len = u8::try_from(length)?;
    }
    let raw = unsafe { libc::socket(libc::AF_UNIX, libc::SOCK_STREAM, 0) };
    ensure!(
        raw >= 0,
        "Cannot create readiness Unix socket: {}",
        std::io::Error::last_os_error()
    );
    let fd = unsafe { OwnedFd::from_raw_fd(raw) };
    ensure!(
        unsafe { libc::fcntl(raw, libc::F_SETFL, libc::O_NONBLOCK) } == 0
            && unsafe { libc::fcntl(raw, libc::F_SETFD, libc::FD_CLOEXEC) } == 0,
        "Cannot protect readiness socket"
    );
    let result = unsafe {
        libc::connect(
            raw,
            (&address as *const libc::sockaddr_un).cast(),
            length as libc::socklen_t,
        )
    };
    if result < 0 {
        let error = std::io::Error::last_os_error();
        ensure!(
            matches!(
                error.raw_os_error(),
                Some(libc::EINPROGRESS) | Some(libc::EALREADY) | Some(libc::EINTR)
            ),
            "Readiness Unix connect failed: {error}"
        );
        loop {
            deadline.wait(Some(raw), libc::POLLOUT)?;
            let mut error: libc::c_int = 0;
            let mut size = std::mem::size_of_val(&error) as libc::socklen_t;
            ensure!(
                unsafe {
                    libc::getsockopt(
                        raw,
                        libc::SOL_SOCKET,
                        libc::SO_ERROR,
                        (&mut error as *mut libc::c_int).cast(),
                        &mut size,
                    )
                } == 0,
                "Cannot inspect readiness connect"
            );
            if error == libc::EINPROGRESS || error == libc::EALREADY {
                continue;
            }
            ensure!(
                error == 0,
                "Readiness Unix connection failed: {}",
                std::io::Error::from_raw_os_error(error)
            );
            let mut peer: libc::sockaddr_un = unsafe { std::mem::zeroed() };
            let mut peer_size = std::mem::size_of_val(&peer) as libc::socklen_t;
            if unsafe {
                libc::getpeername(
                    raw,
                    (&mut peer as *mut libc::sockaddr_un).cast(),
                    &mut peer_size,
                )
            } < 0
            {
                let error = std::io::Error::last_os_error();
                ensure!(
                    error.raw_os_error() == Some(libc::ENOTCONN),
                    "Cannot inspect connected IPC socket: {error}"
                );
                continue;
            }
            break;
        }
    }
    deadline.check()?;
    Ok(UnixStream::from(fd))
}
#[cfg(target_os = "linux")]
fn verify_unix_peer(stream: &UnixStream, expected_pid: u32) -> Result<()> {
    let mut peer: libc::ucred = unsafe { std::mem::zeroed() };
    let mut size = std::mem::size_of_val(&peer) as libc::socklen_t;
    ensure!(
        expected_pid > 0
            && unsafe {
                libc::getsockopt(
                    stream.as_raw_fd(),
                    libc::SOL_SOCKET,
                    libc::SO_PEERCRED,
                    (&mut peer as *mut libc::ucred).cast(),
                    &mut size,
                )
            } == 0,
        "Cannot verify engine IPC peer"
    );
    ensure!(
        size as usize == std::mem::size_of_val(&peer)
            && u32::try_from(peer.pid)? == expected_pid
            && peer.uid == unsafe { libc::geteuid() },
        "Engine IPC peer differs from owned child"
    );
    Ok(())
}
#[cfg(not(target_os = "linux"))]
fn verify_unix_peer(_: &UnixStream, _: u32) -> Result<()> {
    bail!("Engine IPC peer ownership requires Linux")
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct IpcResponse {
    version: u8,
    status: u16,
    headers: IpcHeaders,
    body_base64: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct IpcHeaders {
    #[serde(rename = "Content-Type")]
    content_type: String,
    #[serde(rename = "Cache-Control")]
    cache_control: Option<String>,
}
fn decode_ipc(raw: &[u8], limit: usize) -> Result<Vec<u8>> {
    ensure!(
        !raw.is_empty() && raw.len() <= limit.min(IPC_FRAME_LIMIT),
        "IPC response bound exceeded"
    );
    let response: IpcResponse = serde_json::from_slice(raw)?;
    ensure!(
        response.version == 1
            && response.status == 200
            && response.headers.content_type == "application/json",
        "Invalid IPC readiness response metadata"
    );
    if let Some(cache) = response.headers.cache_control {
        ensure!(
            cache.len() <= 256 && !cache.bytes().any(|b| b < 0x20 || b == 0x7f),
            "Invalid IPC cache header"
        );
    }
    let body = STANDARD.decode(&response.body_base64)?;
    ensure!(
        body.len() <= limit && STANDARD.encode(&body) == response.body_base64,
        "IPC response body encoding or bound invalid"
    );
    Ok(body)
}
fn ipc_probe(
    path: &Path,
    peer: u32,
    method: &str,
    limits: &ProcessLimits,
    deadline: &mut Deadline<'_>,
) -> Result<Vec<u8>> {
    phase((|| -> Result<Vec<u8>> {
    let before = std::fs::symlink_metadata(path)?;
    ensure!(
        before.file_type().is_socket()
            && before.mode() & 0o777 == 0o600
            && before.uid() == unsafe { libc::geteuid() },
        "Engine IPC socket ownership or mode invalid"
    );
    let mut stream = phase(connect_unix(path, deadline), Role::Engine, Stage::IpcConnect)?;
    phase(verify_unix_peer(&stream, peer), Role::Engine, Stage::IpcPeer)?;
    let request = serde_json::to_vec(
        &serde_json::json!({"version":1,"method":"GET","path":method,"query":"","body_base64":"","remote_addr":"127.0.0.1:1"}),
    )?;
    ensure!(
        request
            .len()
            .checked_add(4)
            .is_some_and(|n| n <= limits.max_probe_request_bytes)
            && request.len() <= IPC_FRAME_LIMIT,
        "IPC readiness request bound"
    );
    write_all(&mut stream, &(request.len() as u32).to_be_bytes(), deadline)?;
    write_all(&mut stream, &request, deadline)?;
    let mut prefix = [0; 4];
    read_exact(&mut stream, &mut prefix, deadline)?;
    let length = u32::from_be_bytes(prefix) as usize;
    ensure!(
        length > 0 && length <= limits.max_probe_response_bytes.min(IPC_FRAME_LIMIT),
        "IPC response frame bound"
    );
    let mut raw = vec![0; length];
    read_exact(&mut stream, &mut raw, deadline)?;
    require_eof(&mut stream, deadline)?;
    phase(verify_unix_peer(&stream, peer), Role::Engine, Stage::IpcPeer)?;
    let after = std::fs::symlink_metadata(path)?;
    ensure!(
        (before.dev(), before.ino(), before.mode(), before.uid())
            == (after.dev(), after.ino(), after.mode(), after.uid()),
        "Engine IPC socket changed during probe"
    );
    decode_ipc(&raw, limits.max_probe_response_bytes)
    })(), Role::Engine, Stage::IpcFrame)
}

// Preserve duplicate keys during parsing and reject them at every nesting level.
struct Unique(Value);
impl<'de> Deserialize<'de> for Unique {
    fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = Unique;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("JSON without duplicate keys")
            }
            fn visit_map<A: MapAccess<'de>>(
                self,
                mut a: A,
            ) -> std::result::Result<Unique, A::Error> {
                let mut m = Map::new();
                while let Some((k, v)) = a.next_entry::<String, Unique>()? {
                    if m.insert(k, v.0).is_some() {
                        return Err(serde::de::Error::custom("Duplicate JSON field"));
                    }
                }
                Ok(Unique(Value::Object(m)))
            }
            fn visit_seq<A: SeqAccess<'de>>(
                self,
                mut a: A,
            ) -> std::result::Result<Unique, A::Error> {
                let mut v = Vec::new();
                while let Some(x) = a.next_element::<Unique>()? {
                    v.push(x.0);
                }
                Ok(Unique(Value::Array(v)))
            }
            fn visit_str<E: serde::de::Error>(self, v: &str) -> std::result::Result<Unique, E> {
                Ok(Unique(Value::String(v.into())))
            }
            fn visit_string<E: serde::de::Error>(
                self,
                v: String,
            ) -> std::result::Result<Unique, E> {
                Ok(Unique(Value::String(v)))
            }
            fn visit_bool<E: serde::de::Error>(self, v: bool) -> std::result::Result<Unique, E> {
                Ok(Unique(Value::Bool(v)))
            }
            fn visit_i64<E: serde::de::Error>(self, v: i64) -> std::result::Result<Unique, E> {
                Ok(Unique(Value::Number(v.into())))
            }
            fn visit_u64<E: serde::de::Error>(self, v: u64) -> std::result::Result<Unique, E> {
                Ok(Unique(Value::Number(v.into())))
            }
            fn visit_f64<E: serde::de::Error>(self, v: f64) -> std::result::Result<Unique, E> {
                serde_json::Number::from_f64(v)
                    .map(|n| Unique(Value::Number(n)))
                    .ok_or_else(|| E::custom("Invalid JSON number"))
            }
            fn visit_unit<E: serde::de::Error>(self) -> std::result::Result<Unique, E> {
                Ok(Unique(Value::Null))
            }
        }
        d.deserialize_any(V)
    }
}
fn rpc_result(raw: &[u8]) -> Result<Value> {
    let mut root: Value = serde_json::from_slice::<Unique>(raw)?.0;
    let object = root
        .as_object_mut()
        .context("RPC response must be an object")?;
    ensure!(
        object.len() == 3
            && object.get("jsonrpc").and_then(Value::as_str) == Some("2.0")
            && object.get("id").and_then(Value::as_i64) == Some(-1),
        "Invalid RPC readiness envelope"
    );
    object
        .remove("result")
        .context("RPC readiness result missing")
}
fn decimal(value: &Value) -> Result<u64> {
    let s = value
        .as_str()
        .context("RPC height must be a decimal string")?;
    ensure!(
        !s.is_empty() && (s == "0" || !s.starts_with('0')) && s.bytes().all(|b| b.is_ascii_digit()),
        "Noncanonical RPC height"
    );
    let n: u64 = s.parse()?;
    ensure!(n <= i64::MAX as u64, "RPC height exceeds protocol range");
    Ok(n)
}
fn status(raw: &[u8], chain: &str) -> Result<(u64, bool)> {
    let result = rpc_result(raw)?;
    ensure!(
        result
            .get("node_info")
            .and_then(|v| v.get("network"))
            .and_then(Value::as_str)
            == Some(chain),
        "RPC chain identity mismatch"
    );
    let sync = result
        .get("sync_info")
        .context("Status sync info missing")?;
    Ok((
        decimal(
            sync.get("latest_block_height")
                .context("Status height missing")?,
        )?,
        sync.get("catching_up")
            .and_then(Value::as_bool)
            .context("Status catching_up missing")?,
    ))
}
fn application_info(raw: &[u8]) -> Result<Info> {
    let result = rpc_result(raw)?;
    let response = result
        .get("response")
        .and_then(Value::as_object)
        .context("ABCI info response missing")?;
    ensure!(
        response.get("data").and_then(Value::as_str) == Some("Dytallix local qualification")
            && response.get("version").and_then(Value::as_str) == Some("batch9"),
        "ABCI application identity fields are missing or unsupported"
    );
    // Protobuf JSON omits a zero height and empty hash.
    let height = match response.get("last_block_height") {
        Some(v) => decimal(v)?,
        None => 0,
    };
    let encoded = match response.get("last_block_app_hash") {
        Some(v) => v.as_str().context("ABCI app hash must be base64")?,
        None => "",
    };
    let bytes = STANDARD.decode(encoded)?;
    ensure!(
        STANDARD.encode(&bytes) == encoded
            && (bytes.len() == 32 || (height == 0 && bytes.is_empty())),
        "ABCI app hash encoding or length invalid"
    );
    Ok(Info {
        height,
        app_hash: hex::encode(bytes),
    })
}
fn check_application(info: &Info, preflight: Option<&Info>) -> Result<bool> {
    if let Some(prior) = preflight {
        if info.height < prior.height {
            return Ok(false);
        }
        if info.height == prior.height {
            ensure!(
                info.app_hash == prior.app_hash,
                "Equal-height application hash differs from verified preflight"
            );
        }
    }
    Ok(true)
}

pub fn verify_engine(
    socket: &Path,
    expected_engine_pid: u32,
    expected_chain: &str,
    preflight: Option<&Info>,
    limits: &ProcessLimits,
    mut health: impl FnMut() -> Result<()>,
) -> Result<EngineReady> {
    ensure!(
        !expected_chain.is_empty() && expected_chain.len() <= 128,
        "Expected chain identifier invalid"
    );
    let mut deadline = Deadline::new(limits, &mut health)?;
    loop {
        let status_raw = phase(ipc_probe(socket, expected_engine_pid, "/status", limits, &mut deadline), Role::Engine, Stage::StatusRpc)?;
        let (height, catching_up) = phase(status(&status_raw, expected_chain), Role::Engine, Stage::StatusValidation)?;
        let application_raw = phase(ipc_probe(socket, expected_engine_pid, "/abci_info", limits, &mut deadline), Role::Engine, Stage::ApplicationRpc)?;
        let application = phase(application_info(&application_raw), Role::Engine, Stage::ApplicationValidation)?;
        let application_ready = phase(check_application(&application, preflight), Role::Engine, Stage::ApplicationComparison)?;
        if !catching_up
            && height >= preflight.map_or(0, |v| v.height)
            && application.height >= height
            && application_ready
        {
            phase(deadline.check(), Role::Engine, Stage::EngineReadiness)?;
            return Ok(EngineReady {
                chain_id: expected_chain.into(),
                block_height: height,
                application,
                engine_pid: expected_engine_pid,
            });
        }
        phase(deadline.wait(None, 0), Role::Engine, Stage::EngineReadiness)?;
    }
}

fn connect_tcp(address: SocketAddr, deadline: &mut Deadline<'_>) -> Result<TcpStream> {
    ensure!(
        address.ip().is_loopback() && address.port() > 0,
        "Readiness HTTP target must be explicit numeric loopback"
    );
    loop {
        let quantum = deadline.quantum()?;
        match TcpStream::connect_timeout(&address, quantum) {
            Ok(stream) => {
                stream.set_nonblocking(true)?;
                deadline.check()?;
                return Ok(stream);
            }
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::TimedOut
                        | std::io::ErrorKind::WouldBlock
                        | std::io::ErrorKind::ConnectionRefused
                        | std::io::ErrorKind::Interrupted
                ) =>
            {
                deadline.wait(None, 0)?
            }
            Err(error) => return Err(error.into()),
        }
    }
}
fn http_body(stream: &mut TcpStream, limit: usize, deadline: &mut Deadline<'_>) -> Result<Vec<u8>> {
    let mut head = Vec::new();
    let mut byte = [0; 1];
    loop {
        ensure!(
            head.len() < limit.min(HTTP_HEADER_LIMIT),
            "HTTP readiness header bound"
        );
        read_exact(stream, &mut byte, deadline)?;
        head.push(byte[0]);
        if head.ends_with(b"\r\n\r\n") {
            break;
        }
    }
    let text = std::str::from_utf8(&head)?;
    let mut lines = text[..text.len() - 4].split("\r\n");
    let first = lines.next().context("HTTP status missing")?;
    ensure!(
        first.starts_with("HTTP/1.1 200 ") && first.bytes().all(|b| b >= 0x20 && b < 0x7f),
        "HTTP readiness status is not 200"
    );
    let mut names = BTreeSet::new();
    let mut length = None;
    let mut content_type = false;
    for line in lines {
        ensure!(
            names.len() < HTTP_HEADER_COUNT,
            "HTTP readiness header count"
        );
        let (name, value) = line
            .split_once(':')
            .context("Malformed HTTP readiness header")?;
        ensure!(
            !name.is_empty() && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-'),
            "Malformed HTTP readiness header name"
        );
        let name = name.to_ascii_lowercase();
        ensure!(
            names.insert(name.clone()),
            "Duplicate HTTP readiness header"
        );
        let value = value.trim_matches(' ');
        ensure!(
            !value.bytes().any(|b| b < 0x20 || b == 0x7f),
            "Malformed HTTP readiness header value"
        );
        match name.as_str() {
            "transfer-encoding" | "content-encoding" => {
                bail!("Encoded HTTP readiness body is unsupported")
            }
            "content-length" => {
                ensure!(
                    !value.is_empty()
                        && (value == "0" || !value.starts_with('0'))
                        && value.bytes().all(|b| b.is_ascii_digit()),
                    "Invalid HTTP Content-Length"
                );
                length = Some(value.parse::<usize>()?);
            }
            "content-type" => {
                ensure!(
                    value == "application/json",
                    "HTTP readiness content type mismatch"
                );
                content_type = true;
            }
            _ => (),
        }
    }
    ensure!(content_type, "HTTP readiness content type missing");
    let length = length.context("HTTP readiness Content-Length missing")?;
    ensure!(
        head.len().checked_add(length).is_some_and(|n| n <= limit),
        "HTTP readiness response bound"
    );
    let mut body = vec![0; length];
    read_exact(stream, &mut body, deadline)?;
    require_eof(stream, deadline)?;
    Ok(body)
}
fn http_probe(
    address: SocketAddr,
    limits: &ProcessLimits,
    deadline: &mut Deadline<'_>,
) -> Result<Vec<u8>> {
    let mut stream = connect_tcp(address, deadline)?;
    let request=format!("GET /status HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\nAccept: application/json\r\n\r\n");
    ensure!(
        request.len() <= limits.max_probe_request_bytes,
        "HTTP readiness request bound"
    );
    write_all(&mut stream, request.as_bytes(), deadline)?;
    http_body(&mut stream, limits.max_probe_response_bytes, deadline)
}

fn proc_read(path: &Path, limit: usize, deadline: &mut Deadline<'_>) -> Result<Vec<u8>> {
    deadline.check()?;
    ensure!(limit > 0, "Process inspection byte bound missing");
    let mut file = std::fs::File::open(path)?;
    let mut bytes = Vec::new();
    let mut buffer = [0; 4096];
    loop {
        deadline.check()?;
        let amount = buffer
            .len()
            .min(limit.saturating_sub(bytes.len()).saturating_add(1));
        let n = file.read(&mut buffer[..amount])?;
        if n == 0 {
            break;
        }
        ensure!(
            bytes.len().checked_add(n).is_some_and(|n| n <= limit),
            "Process inspection byte bound"
        );
        bytes.extend_from_slice(&buffer[..n]);
    }
    Ok(bytes)
}
fn tcp_listener_inodes(
    raw: &[u8],
    address: SocketAddr,
    owned: &BTreeSet<u64>,
) -> Result<BTreeSet<u64>> {
    let text = std::str::from_utf8(raw)?;
    let mut result = BTreeSet::new();
    for (index, line) in text.lines().enumerate().skip(1) {
        ensure!(index <= NET_ROW_LIMIT, "TCP inspection row bound");
        let columns: Vec<_> = line.split_ascii_whitespace().collect();
        ensure!(columns.len() >= 10, "Malformed TCP inspection row");
        if columns[3] != "0A" {
            continue;
        }
        let (ip, port) = columns[1]
            .split_once(':')
            .context("Malformed TCP local address")?;
        let port = u16::from_str_radix(port, 16)?;
        let ip = match ip.len() {
            8 => IpAddr::V4(Ipv4Addr::from(u32::from_str_radix(ip, 16)?.to_le_bytes())),
            32 => {
                let mut bytes = [0; 16];
                for (i, part) in ip.as_bytes().chunks_exact(8).enumerate() {
                    bytes[i * 4..i * 4 + 4].copy_from_slice(
                        &u32::from_str_radix(std::str::from_utf8(part)?, 16)?.to_le_bytes(),
                    );
                }
                IpAddr::V6(Ipv6Addr::from(bytes))
            }
            _ => bail!("Malformed TCP IP length"),
        };
        let inode: u64 = columns[9].parse()?;
        let exact = SocketAddr::new(ip, port) == address;
        let overlaps = port == address.port() && (exact || ip.is_unspecified());
        if overlaps {
            ensure!(
                owned.contains(&inode),
                "Competing listener is not owned by adapter child"
            );
            if exact {
                result.insert(inode);
            }
        }
    }
    Ok(result)
}
#[cfg(target_os = "linux")]
fn socket_fds(
    root: &Path,
    limit: usize,
    deadline: &mut Deadline<'_>,
) -> Result<Option<(BTreeSet<u64>, usize)>> {
    let mut inodes = BTreeSet::new();
    let mut bytes = 0usize;
    for (index, entry) in std::fs::read_dir(root.join("fd"))?.enumerate() {
        deadline.check()?;
        ensure!(
            index < FD_ENTRY_LIMIT,
            "Adapter descriptor inspection bound"
        );
        let target = match std::fs::read_link(entry?.path()) {
            Ok(target) => target,
            // An enumerated descriptor closed before readlink. This snapshot
            // proves nothing; the caller must retry within the same deadline.
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        bytes = bytes
            .checked_add(target.as_os_str().len())
            .context("Descriptor inspection overflow")?;
        ensure!(bytes <= limit, "Adapter descriptor byte bound");
        if let Some(text) = target.to_str() {
            if let Some(inode) = text
                .strip_prefix("socket:[")
                .and_then(|s| s.strip_suffix(']'))
            {
                inodes.insert(inode.parse::<u64>()?);
            }
        }
    }
    Ok(Some((inodes, bytes)))
}
#[cfg(target_os = "linux")]
fn listener_binding(
    pid: u32,
    address: SocketAddr,
    limit: usize,
    deadline: &mut Deadline<'_>,
) -> Result<BTreeSet<u64>> {
    ensure!(pid > 0, "Owned adapter PID missing");
    let root = std::path::PathBuf::from(format!("/proc/{pid}"));
    loop {
        deadline.check()?;
        let metadata = std::fs::symlink_metadata(&root)?;
        ensure!(
            metadata.is_dir() && metadata.uid() == unsafe { libc::geteuid() },
            "Adapter process ownership is invalid"
        );
        let Some((inodes, mut bytes)) = socket_fds(&root, limit, deadline)? else {
            deadline.wait(None, 0)?;
            continue;
        };
        let mut tables = Vec::new();
        for net in ["net/tcp", "net/tcp6"] {
            let raw = proc_read(
                &root.join(net),
                limit
                    .checked_sub(bytes)
                    .context("Adapter inspection bound")?,
                deadline,
            )?;
            bytes = bytes
                .checked_add(raw.len())
                .context("Adapter inspection overflow")?;
            tables.push(raw);
        }
        let Some((after, _)) = socket_fds(
            &root,
            limit
                .checked_sub(bytes)
                .context("Adapter inspection bound")?,
            deadline,
        )?
        else {
            deadline.wait(None, 0)?;
            continue;
        };
        if after != inodes {
            deadline.wait(None, 0)?;
            continue;
        }
        let mut matched = BTreeSet::new();
        for table in tables {
            matched.extend(tcp_listener_inodes(&table, address, &inodes)?);
        }
        // Empty means that the healthy owned process has not bound yet. A
        // competing listener still fails inside tcp_listener_inodes.
        return Ok(matched);
    }
}
#[cfg(not(target_os = "linux"))]
fn listener_binding(
    _: u32,
    _: SocketAddr,
    _: usize,
    _: &mut Deadline<'_>,
) -> Result<BTreeSet<u64>> {
    bail!("HTTP listener ownership requires Linux")
}

pub fn verify_adapter(
    address: SocketAddr,
    expected_adapter_pid: u32,
    expected_chain: &str,
    minimum_height: u64,
    limits: &ProcessLimits,
    mut health: impl FnMut() -> Result<()>,
) -> Result<AdapterReady> {
    ensure!(
        address.ip().is_loopback()
            && address.port() > 0
            && !expected_chain.is_empty()
            && expected_chain.len() <= 128,
        "Invalid adapter readiness target"
    );
    let mut deadline = Deadline::new(limits, &mut health)?;
    loop {
        let before = phase(listener_binding(
            expected_adapter_pid,
            address,
            limits.max_probe_response_bytes,
            &mut deadline,
        ), Role::Adapter, Stage::AdapterListener)?;
        if before.is_empty() {
            phase(deadline.wait(None, 0), Role::Adapter, Stage::AdapterReadiness)?;
            continue;
        }
        let (height, catching_up) =
            phase(status(&phase(http_probe(address, limits, &mut deadline), Role::Adapter, Stage::AdapterHttp)?, expected_chain), Role::Adapter, Stage::StatusValidation)?;
        let after = phase(listener_binding(
            expected_adapter_pid,
            address,
            limits.max_probe_response_bytes,
            &mut deadline,
        ), Role::Adapter, Stage::AdapterListener)?;
        phase((|| { ensure!(before == after,"Adapter listener identity changed during readiness"); Ok(()) })(), Role::Adapter, Stage::AdapterIdentity)?;
        if height >= minimum_height && !catching_up {
            phase(deadline.check(), Role::Adapter, Stage::AdapterReadiness)?;
            return Ok(AdapterReady {
                chain_id: expected_chain.into(),
                block_height: height,
                adapter_pid: expected_adapter_pid,
            });
        }
        phase(deadline.wait(None, 0), Role::Adapter, Stage::AdapterReadiness)?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::thread::{self, JoinHandle};
    fn limits() -> ProcessLimits {
        ProcessLimits {
            startup_millis: 200,
            stop_millis: 100,
            kill_millis: 100,
            poll_millis: 2,
            max_argument_bytes: 4096,
            max_environment_bytes: 4096,
            max_probe_request_bytes: 4096,
            max_probe_response_bytes: 65536,
        }
    }
    fn status_body(chain: &str, height: u64) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({"jsonrpc":"2.0","id":-1,"result":{
            "node_info":{"network":chain},"sync_info":{"latest_block_height":height.to_string(),"catching_up":false}}})).unwrap()
    }
    fn app_body(height: u64, hash: &[u8]) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({"jsonrpc":"2.0","id":-1,"result":{"response":{
            "data":"Dytallix local qualification","version":"batch9","last_block_height":height.to_string(),"last_block_app_hash":STANDARD.encode(hash)}}})).unwrap()
    }
    fn http_response(body: &[u8]) -> Vec<u8> {
        let mut raw=format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",body.len()).into_bytes();
        raw.extend_from_slice(body);
        raw
    }
    fn server(raw: Option<Vec<u8>>) -> (SocketAddr, JoinHandle<()>) {
        server_with_lifetime(raw, None)
    }
    fn server_with_lifetime(
        raw: Option<Vec<u8>>,
        lifetime: Option<std::sync::mpsc::Receiver<()>>,
    ) -> (SocketAddr, JoinHandle<()>) {
        server_with_delay(raw, lifetime, Duration::ZERO)
    }
    fn server_with_delay(
        raw: Option<Vec<u8>>,
        lifetime: Option<std::sync::mpsc::Receiver<()>>,
        bind_delay: Duration,
    ) -> (SocketAddr, JoinHandle<()>) {
        let reserved = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = reserved.local_addr().unwrap();
        let listener = if bind_delay.is_zero() {
            Some(reserved)
        } else {
            drop(reserved);
            None
        };
        let task = thread::spawn(move || {
            let listener = match listener {
                Some(listener) => listener,
                None => {
                    thread::sleep(bind_delay);
                    TcpListener::bind(address).unwrap()
                }
            };
            listener.set_nonblocking(true).unwrap();
            let end = Instant::now() + Duration::from_secs(1);
            let mut stream = loop {
                match listener.accept() {
                    Ok((s, _)) => break s,
                    Err(e)
                        if e.kind() == std::io::ErrorKind::WouldBlock && Instant::now() < end =>
                    {
                        thread::sleep(Duration::from_millis(1))
                    }
                    _ => return,
                }
            };
            // macOS can inherit listener nonblocking mode on accepted sockets.
            // This fixture uses bounded blocking I/O; the actual client stays nonblocking.
            stream.set_nonblocking(false).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_millis(500)))
                .unwrap();
            stream
                .set_write_timeout(Some(Duration::from_millis(500)))
                .unwrap();
            let mut request = Vec::new();
            let mut b = [0; 1];
            while request.len() < 4096 {
                match stream.read(&mut b) {
                    Ok(1) => {
                        request.push(b[0]);
                        if request.ends_with(b"\r\n\r\n") {
                            break;
                        }
                    }
                    other => {
                        panic!("Synthetic server did not receive the bounded request: {other:?}")
                    }
                }
            }
            assert!(request.ends_with(b"\r\n\r\n"));
            assert!(request.starts_with(b"GET /status HTTP/1.1\r\n"));
            assert!(request.windows(19).any(|s| s == b"Connection: close\r\n"));
            if let Some(raw) = raw {
                let _ = stream.write_all(&raw);
            } else {
                let _ = stream.read(&mut b);
            }
            drop(stream);
            if let Some(lifetime) = lifetime {
                let _ = lifetime.recv_timeout(Duration::from_secs(1));
            }
        });
        (address, task)
    }
    fn probe(raw: Vec<u8>, limit: usize) -> Result<Vec<u8>> {
        let (address, task) = server(Some(raw));
        let mut bounds = limits();
        bounds.max_probe_response_bytes = limit;
        let mut health = || Ok(());
        let mut deadline = Deadline::new(&bounds, &mut health).unwrap();
        let result = http_probe(address, &bounds, &mut deadline);
        task.join().unwrap();
        result
    }
    #[test]
    fn readiness_http_actual_loopback_response_is_bounded_and_chain_checked() {
        let raw = probe(http_response(&status_body("development-ready-1", 8)), 65536).unwrap();
        assert_eq!(status(&raw, "development-ready-1").unwrap(), (8, false));
        assert!(status(&raw, "different-chain")
            .unwrap_err()
            .to_string()
            .contains("chain identity"));
    }
    #[test]
    fn readiness_http_rejects_malformed_framing_and_oversized_responses() {
        for raw in [
            b"HTTP/1.1 503 Unavailable\r\nContent-Length: 0\r\nContent-Type: application/json\r\n\r\n".to_vec(),
            b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nContent-Length: 0\r\nContent-Type: application/json\r\n\r\n".to_vec(),
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nContent-Type: application/json\r\n\r\n0\r\n\r\n".to_vec(),
            b"HTTP/1.1 200 OK\r\nContent-Length: +2\r\nContent-Type: application/json\r\n\r\n{}".to_vec(),
            b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n{}".to_vec(),
            b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\nContent-Type: application/json\r\n\r\n{}".to_vec(),
            b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nContent-Type: application/json\r\n\r\n{}x".to_vec(),
            b"HTTP/1.1 200 OK\r\nContent-Length: 9999999999999999999999999999\r\nContent-Type: application/json\r\n\r\n".to_vec(),
        ]{assert!(probe(raw,65536).is_err());}
        assert!(probe(http_response(&status_body("chain", 1)), 32).is_err());
    }
    #[test]
    fn readiness_hanging_response_respects_deadline_and_owner_cancellation() {
        for cancel in [false, true] {
            let (address, task) = server(None);
            let mut bounds = limits();
            bounds.startup_millis = 30;
            let start = Instant::now();
            let mut health = || {
                if cancel && start.elapsed() >= Duration::from_millis(5) {
                    bail!("owner canceled");
                }
                Ok(())
            };
            let mut deadline = Deadline::new(&bounds, &mut health).unwrap();
            let error = http_probe(address, &bounds, &mut deadline).unwrap_err();
            assert!(error
                .to_string()
                .contains(if cancel { "owner canceled" } else { "deadline" }));
            assert!(start.elapsed() < Duration::from_millis(500));
            task.join().unwrap();
        }
    }
    #[test]
    fn readiness_rpc_rejects_duplicate_wrong_envelope_and_noncanonical_heights() {
        for raw in [
            br#"{"jsonrpc":"2.0","id":-1,"result":{"node_info":{"network":"good","network":"bad"},"sync_info":{"latest_block_height":"1","catching_up":false}}}"#.as_slice(),
            br#"{"jsonrpc":"2.0","id":-1,"error":null}"#.as_slice(),
            br#"{"jsonrpc":"2.0","id":0,"result":{}}"#.as_slice(),
        ]{assert!(status(raw,"good").is_err());}
        for value in [
            serde_json::json!(1),
            serde_json::json!("01"),
            serde_json::json!("-1"),
            serde_json::json!("18446744073709551615"),
        ] {
            assert!(decimal(&value).is_err());
        }
        assert!(
            application_info(br#"{"jsonrpc":"2.0","id":-1,"result":{"response":{}}}"#).is_err()
        );
    }
    #[test]
    fn readiness_application_progress_uses_abci_current_hash_and_allows_advancing_heights() {
        let prior = Info {
            height: 10,
            app_hash: hex::encode([7; 32]),
        };
        assert_eq!(status(&status_body("chain", 10), "chain").unwrap().0, 10);
        let advanced = application_info(&app_body(11, &[8; 32])).unwrap();
        assert!(check_application(&advanced, Some(&prior)).unwrap());
        assert!(check_application(
            &application_info(&app_body(10, &[7; 32])).unwrap(),
            Some(&prior)
        )
        .unwrap());
        assert!(check_application(
            &application_info(&app_body(10, &[8; 32])).unwrap(),
            Some(&prior)
        )
        .is_err());
        assert!(!check_application(
            &application_info(&app_body(9, &[7; 32])).unwrap(),
            Some(&prior)
        )
        .unwrap());
        assert!(check_application(&application_info(&app_body(0, &[])).unwrap(), None).unwrap());
        assert!(application_info(&app_body(1, &[])).is_err());
        let mut raw: Value = serde_json::from_slice(&app_body(1, &[7; 32])).unwrap();
        raw["result"]["response"]["last_block_app_hash"] =
            serde_json::json!(STANDARD.encode([7; 32]).trim_end_matches('='));
        assert!(application_info(&serde_json::to_vec(&raw).unwrap()).is_err());
    }
    #[test]
    fn readiness_ipc_envelope_enforces_framing_contract() {
        let body = status_body("chain", 3);
        let value = serde_json::json!({"version":1,"status":200,"headers":{"Content-Type":"application/json"},"body_base64":STANDARD.encode(&body)});
        let raw = serde_json::to_vec(&value).unwrap();
        assert_eq!(decode_ipc(&raw, 65536).unwrap(), body);
        for (key, v) in [
            ("version", serde_json::json!(2)),
            ("status", serde_json::json!(503)),
            ("body_base64", serde_json::json!("e30")),
            ("unknown", serde_json::json!(true)),
        ] {
            let mut invalid = value.clone();
            invalid[key] = v;
            assert!(decode_ipc(&serde_json::to_vec(&invalid).unwrap(), 65536).is_err());
        }
        assert!(decode_ipc(&raw, 1).is_err());
    }
    #[test]
    fn readiness_listener_parser_binds_exact_address_port_inode_and_rejects_competitors() {
        let header="sl local_address rem_address st tx_queue rx_queue tr tm_when retrnsmt uid timeout inode\n";
        let row =
            "0: 0100007F:1234 00000000:0000 0A 00000000:00000000 00:00000000 00000000 1000 0 42\n";
        let address = "127.0.0.1:4660".parse().unwrap();
        assert_eq!(
            tcp_listener_inodes(
                format!("{header}{row}").as_bytes(),
                address,
                &[42].into_iter().collect()
            )
            .unwrap(),
            [42].into_iter().collect()
        );
        assert!(tcp_listener_inodes(
            format!("{header}{row}").as_bytes(),
            address,
            &[43].into_iter().collect()
        )
        .is_err());
        let competing = row.replace(" 42\n", " 43\n");
        assert!(tcp_listener_inodes(
            format!("{header}{row}{competing}").as_bytes(),
            address,
            &[42].into_iter().collect()
        )
        .is_err());
        let v6 = row.replace("0100007F:1234", "00000000000000000000000001000000:1234");
        assert_eq!(
            tcp_listener_inodes(
                format!("{header}{v6}").as_bytes(),
                "[::1]:4660".parse().unwrap(),
                &[42].into_iter().collect()
            )
            .unwrap()
            .len(),
            1
        );
        assert!(tcp_listener_inodes(
            format!("{header}{row}").as_bytes(),
            "127.0.0.1:4661".parse().unwrap(),
            &[42].into_iter().collect()
        )
        .unwrap()
        .is_empty());
    }
    #[test]
    fn readiness_rejects_non_loopback_before_connecting() {
        let bounds = limits();
        let mut health = || Ok(());
        let mut deadline = Deadline::new(&bounds, &mut health).unwrap();
        assert!(connect_tcp("192.0.2.1:1234".parse().unwrap(), &mut deadline).is_err());
        assert!(verify_adapter(
            "0.0.0.0:1234".parse().unwrap(),
            std::process::id(),
            "chain",
            0,
            &bounds,
            || Ok(())
        )
        .is_err());
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn readiness_actual_loopback_listener_is_bound_to_expected_process() {
        let (release, lifetime) = std::sync::mpsc::channel();
        let (address, task) = server_with_lifetime(
            Some(http_response(&status_body("chain", 5))),
            Some(lifetime),
        );
        let result = verify_adapter(
            address,
            std::process::id(),
            "chain",
            4,
            &limits(),
            || Ok(()),
        );
        release.send(()).unwrap();
        let ready = result.unwrap();
        assert!(ready.listener_identity_bound());
        assert_eq!(ready.block_height(), 5);
        task.join().unwrap();
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn readiness_actual_engine_ipc_checks_owned_peer_and_moving_application_state() {
        use std::os::unix::fs::PermissionsExt;
        use std::os::unix::net::UnixListener;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().canonicalize().unwrap().join("rpc.sock");
        let listener = UnixListener::bind(&path).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        listener.set_nonblocking(true).unwrap();
        let task = thread::spawn(move || {
            for body in [status_body("chain", 10), app_body(11, &[8; 32])] {
                let end = Instant::now() + Duration::from_secs(1);
                let mut stream = loop {
                    match listener.accept() {
                        Ok((s, _)) => break s,
                        Err(e)
                            if e.kind() == std::io::ErrorKind::WouldBlock
                                && Instant::now() < end =>
                        {
                            thread::sleep(Duration::from_millis(1))
                        }
                        _ => return,
                    }
                };
                stream
                    .set_read_timeout(Some(Duration::from_millis(500)))
                    .unwrap();
                stream
                    .set_write_timeout(Some(Duration::from_millis(500)))
                    .unwrap();
                let mut prefix = [0; 4];
                std::io::Read::read_exact(&mut stream, &mut prefix).unwrap();
                let length = u32::from_be_bytes(prefix) as usize;
                assert!(length <= 4096);
                let mut request = vec![0; length];
                std::io::Read::read_exact(&mut stream, &mut request).unwrap();
                let raw=serde_json::to_vec(&serde_json::json!({"version":1,"status":200,"headers":{"Content-Type":"application/json"},"body_base64":STANDARD.encode(body)})).unwrap();
                stream.write_all(&(raw.len() as u32).to_be_bytes()).unwrap();
                stream.write_all(&raw).unwrap();
            }
        });
        let (peer, _other) = UnixStream::pair().unwrap();
        verify_unix_peer(&peer, std::process::id()).unwrap();
        assert!(verify_unix_peer(&peer, 0).is_err());
        let prior = Info {
            height: 10,
            app_hash: hex::encode([7; 32]),
        };
        let ready = verify_engine(
            &path,
            std::process::id(),
            "chain",
            Some(&prior),
            &limits(),
            || Ok(()),
        )
        .unwrap();
        assert_eq!(ready.block_height(), 10);
        assert_eq!(ready.application_info().height, 11);
        task.join().unwrap();
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn readiness_waits_for_owned_adapter_delayed_bind() {
        let (release, lifetime) = std::sync::mpsc::channel();
        let delay = Duration::from_millis(30);
        let (address, task) = server_with_delay(
            Some(http_response(&status_body("chain", 5))),
            Some(lifetime),
            delay,
        );
        let start = Instant::now();
        let mut health_checks = 0;
        let result = verify_adapter(address, std::process::id(), "chain", 4, &limits(), || {
            health_checks += 1;
            Ok(())
        });
        release.send(()).unwrap();
        let ready = result.unwrap();
        assert_eq!(ready.block_height(), 5);
        assert!(start.elapsed() >= delay);
        assert!(health_checks > 2);
        task.join().unwrap();
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn readiness_refuses_listener_outside_the_expected_process() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        // Own a separate bounded child with no listener. Its namespace contains
        // this test listener, but its socket FDs cannot establish ownership.
        let mut child = std::process::Command::new("/bin/sleep")
            .arg("2")
            .spawn()
            .unwrap();
        let result = verify_adapter(
            listener.local_addr().unwrap(),
            child.id(),
            "chain",
            0,
            &limits(),
            || {
                ensure!(child.try_wait()?.is_none(), "Test child exited");
                Ok(())
            },
        );
        let _ = child.kill();
        let _ = child.wait();
        let error = result.unwrap_err();
        assert_eq!(crate::startup_diagnostic::record(&error)["stage"], "adapter_listener");
        assert!(error.chain().any(|cause| cause.to_string().contains("Competing listener")));
    }
}
