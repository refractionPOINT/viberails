use std::time::{Instant, SystemTime};

use anyhow::{Context, Result, bail};
use derive_more::Display;
use log::{debug, error, info, warn};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::{
    common::{PROJECT_VERSION, PROJECT_VERSION_HASH, display_authorize_help, user_agent},
    config::Config,
    providers::Providers,
};

const CLOUD_API_TIMEOUT_SECS: u64 = 10;

/// Maximum body size accepted by the `lc_sensor` event socket (60 KB).
const MAX_LC_SOCKET_BODY: usize = 60 * 1024;

#[derive(Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CloudQueryType {
    Auth,
    Notify,
}

#[derive(Display)]
pub enum CloudVerdict {
    Allow,
    Deny(String),
}

#[derive(Deserialize)]
struct CloudResponse {
    success: bool,
    reason: Option<String>,
    #[allow(dead_code)]
    error: Option<String>,
    #[allow(dead_code)]
    rejected: Option<bool>,
    rule: Option<String>,
}

impl CloudResponse {
    pub fn block_message(&self) -> String {
        let mut parts = Vec::new();

        parts.push("Command blocked by policy.".to_string());

        if let Some(reason) = &self.reason {
            parts.push(format!("Reason: {reason}"));
        }

        if let Some(rule) = &self.rule {
            parts.push(format!("Rule: {rule}"));
        }

        if let Some(error) = &self.error {
            parts.push(format!("Error: {error}"));
        }

        parts.join(" ")
    }
}

#[derive(Serialize)]
struct CloudRequestMetaVersion {
    version: &'static str,
    hash: &'static str,
}

#[derive(Serialize)]
struct CloudRequestMeta<'a> {
    ts: u128,
    installation_id: &'a str,
    request_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    hostname: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    session_id: Option<String>,
    source: &'a Providers,
    #[serde(rename = "type")]
    query_type: CloudQueryType,
    #[serde(skip_serializing_if = "Option::is_none")]
    username: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    ppid: Option<u32>,
    version: CloudRequestMetaVersion,
}

#[derive(Serialize)]
struct CloudRequest<'a> {
    meta_data: CloudRequestMeta<'a>,
    #[serde(skip_serializing_if = "Option::is_none")]
    auth: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    notify: Option<Value>,
}

pub struct CloudQuery<'a> {
    config: &'a Config,
    url: String,
    secret: String,
    provider: Providers,
}

fn get_ppid() -> Option<u32> {
    use sysinfo::{ProcessRefreshKind, System, UpdateKind};

    let pid = sysinfo::get_current_pid().ok()?;
    let mut sys = System::new();
    sys.refresh_processes_specifics(
        sysinfo::ProcessesToUpdate::Some(&[pid]),
        false,
        ProcessRefreshKind::nothing().with_exe(UpdateKind::Never),
    );
    sys.process(pid)?.parent().map(sysinfo::Pid::as_u32)
}

fn mine_session_id(data: &Value) -> Option<String> {
    //
    // This is to be accomodating for various providers and or versions
    // so we're mining for some kind of session id
    //
    if let Some(session_value) = data.get("session_id")
        && let Some(session_id) = session_value.as_str()
    {
        return Some(session_id.to_string());
    }

    //
    // We'll log it and hopefully it'll percolate so we can fix this
    //
    warn!("Unable to find a session id in hook data");
    None
}

/// Return the path to the `LimaCharlie` EDR event socket, if it exists on disk.
///
/// Matches `lc_sensor` collector 30 logic: root uses `/var/run/lc_event.sock`,
/// non-root uses `$HOME/.local/run/lc_event.sock` (falls back to the root path
/// when `HOME` is not set, same as the sensor).
#[cfg(unix)]
fn get_lc_socket_path() -> Option<std::path::PathBuf> {
    use std::path::PathBuf;

    // SAFETY: geteuid() is a simple syscall with no preconditions.
    let path = if unsafe { libc::geteuid() } == 0 {
        PathBuf::from("/var/run/lc_event.sock")
    } else if let Ok(home) = std::env::var("HOME") {
        PathBuf::from(home).join(".local/run/lc_event.sock")
    } else {
        // Match lc_sensor: fall back to root path when HOME is not set
        PathBuf::from("/var/run/lc_event.sock")
    };

    if path.exists() {
        Some(path)
    } else {
        None
    }
}

/// Send an HTTP POST to the `lc_sensor` event socket at the given path.
///
/// Sends `POST /event?ppid=<ppid>&event_id=<event_id>` with the body as the
/// request payload.  If the body exceeds the sensor's 60 KB limit it is dropped
/// and only the metadata (ppid + `event_id`) is sent.
#[cfg(unix)]
fn post_to_lc_socket(
    socket_path: &std::path::Path,
    ppid: u32,
    event_id: &str,
    body: &str,
) -> Result<()> {
    use std::io::{Read, Write};
    use std::os::unix::net::UnixStream;
    use std::time::Duration;

    let mut stream =
        UnixStream::connect(socket_path).context("connect to lc_event socket")?;
    stream
        .set_write_timeout(Some(Duration::from_secs(5)))
        .context("set write timeout")?;
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .context("set read timeout")?;

    let body_bytes: &[u8] = if body.len() > MAX_LC_SOCKET_BODY {
        warn!(
            "EDR event body too large ({} bytes), sending without body",
            body.len()
        );
        &[]
    } else {
        body.as_bytes()
    };

    let header = format!(
        "POST /event?ppid={ppid}&event_id={} HTTP/1.0\r\nContent-Length: {}\r\n\r\n",
        urlencoding::encode(event_id),
        body_bytes.len()
    );

    stream
        .write_all(header.as_bytes())
        .context("write request header")?;
    stream
        .write_all(body_bytes)
        .context("write request body")?;

    let mut response = Vec::new();
    stream
        .read_to_end(&mut response)
        .context("read response")?;

    let response_str = String::from_utf8_lossy(&response);
    let status_line = response_str.lines().next().unwrap_or("empty response");
    if status_line.contains("200") {
        Ok(())
    } else {
        bail!("lc_event socket returned: {status_line}");
    }
}

/// Return the path to the `LimaCharlie` EDR event socket on Windows.
///
/// Matches `lc_sensor` collector 30: `%ProgramData%\limacharlie\lc_event.sock`.
#[cfg(windows)]
fn get_lc_socket_path() -> Option<std::path::PathBuf> {
    use std::path::PathBuf;

    let program_data = std::env::var("ProgramData").ok()?;
    let path = PathBuf::from(program_data)
        .join("limacharlie")
        .join("lc_event.sock");

    if path.exists() {
        Some(path)
    } else {
        None
    }
}

/// Send an HTTP POST to the `lc_sensor` event socket on Windows via `AF_UNIX`.
#[cfg(windows)]
fn post_to_lc_socket(
    socket_path: &std::path::Path,
    ppid: u32,
    event_id: &str,
    body: &str,
) -> Result<()> {
    use windows_sys::Win32::Networking::WinSock::{
        AF_UNIX, INVALID_SOCKET, SOCKADDR, SOCK_STREAM, SOL_SOCKET, SO_RCVTIMEO, SO_SNDTIMEO,
        WSADATA, WSACleanup, WSAStartup, closesocket, connect, recv, send, setsockopt, socket,
    };

    /// `SOCKADDR_UN` is not defined in `windows-sys`; matches the Win32 layout.
    #[repr(C)]
    struct SockaddrUn {
        sun_family: u16,
        sun_path: [u8; 108],
    }

    /// RAII guard: calls `WSACleanup` on drop to pair with `WSAStartup`.
    struct WsaGuard;
    impl Drop for WsaGuard {
        fn drop(&mut self) {
            // SAFETY: paired with a successful WSAStartup call.
            unsafe {
                WSACleanup();
            }
        }
    }

    /// RAII guard: calls `closesocket` on drop.
    struct SocketGuard(windows_sys::Win32::Networking::WinSock::SOCKET);
    impl Drop for SocketGuard {
        fn drop(&mut self) {
            // SAFETY: closing our own valid socket.
            unsafe {
                closesocket(self.0);
            }
        }
    }

    let path_str = socket_path
        .to_str()
        .context("lc_event socket path is not valid UTF-8")?;
    let path_bytes = path_str.as_bytes();
    if path_bytes.len() >= 108 {
        bail!("lc_event socket path exceeds sockaddr_un limit (108 bytes)");
    }

    // SAFETY: WSADATA is a plain data struct, zero-init is valid.
    let mut wsa_data: WSADATA = unsafe { std::mem::zeroed() };
    // SAFETY: standard Winsock initialisation with version 2.2.
    let ret = unsafe { WSAStartup(0x0202, &mut wsa_data) };
    if ret != 0 {
        bail!("WSAStartup failed with error {ret}");
    }
    let _wsa = WsaGuard;

    // SAFETY: creating a standard AF_UNIX stream socket.
    let raw = unsafe { socket(i32::from(AF_UNIX), SOCK_STREAM as i32, 0) };
    if raw == INVALID_SOCKET {
        bail!("socket(AF_UNIX) failed");
    }
    let sock = SocketGuard(raw);

    // Set 5-second send/recv timeouts (DWORD milliseconds on Windows).
    let timeout_ms: u32 = 5000;
    // SAFETY: setting standard socket options with correct buffer and size.
    unsafe {
        setsockopt(
            sock.0,
            SOL_SOCKET,
            SO_SNDTIMEO,
            std::ptr::from_ref(&timeout_ms).cast::<u8>(),
            std::mem::size_of::<u32>() as i32,
        );
        setsockopt(
            sock.0,
            SOL_SOCKET,
            SO_RCVTIMEO,
            std::ptr::from_ref(&timeout_ms).cast::<u8>(),
            std::mem::size_of::<u32>() as i32,
        );
    }

    // SAFETY: SockaddrUn is a plain C struct, zero-init is valid.
    let mut addr: SockaddrUn = unsafe { std::mem::zeroed() };
    addr.sun_family = AF_UNIX;
    addr.sun_path[..path_bytes.len()].copy_from_slice(path_bytes);
    // sun_path is already null-terminated from the zero-init.

    // SAFETY: addr is correctly sized and populated.
    let ret = unsafe {
        connect(
            sock.0,
            std::ptr::from_ref(&addr).cast::<SOCKADDR>(),
            std::mem::size_of::<SockaddrUn>() as i32,
        )
    };
    if ret != 0 {
        bail!("connect to lc_event socket failed");
    }

    let body_bytes: &[u8] = if body.len() > MAX_LC_SOCKET_BODY {
        warn!(
            "EDR event body too large ({} bytes), sending without body",
            body.len()
        );
        &[]
    } else {
        body.as_bytes()
    };

    let header = format!(
        "POST /event?ppid={ppid}&event_id={} HTTP/1.0\r\nContent-Length: {}\r\n\r\n",
        urlencoding::encode(event_id),
        body_bytes.len()
    );

    let header_bytes = header.as_bytes();
    // SAFETY: sending from a valid, immutable byte slice.
    let sent = unsafe { send(sock.0, header_bytes.as_ptr(), header_bytes.len() as i32, 0) };
    if sent < 0 {
        bail!("send header to lc_event socket failed");
    }

    if !body_bytes.is_empty() {
        // SAFETY: sending from a valid, immutable byte slice.
        let sent = unsafe { send(sock.0, body_bytes.as_ptr(), body_bytes.len() as i32, 0) };
        if sent < 0 {
            bail!("send body to lc_event socket failed");
        }
    }

    let mut response = [0u8; 512];
    // SAFETY: receiving into a valid mutable buffer.
    let n = unsafe { recv(sock.0, response.as_mut_ptr(), response.len() as i32, 0) };
    if n < 0 {
        bail!("recv from lc_event socket failed");
    }
    let n = n as usize;
    let response_str = String::from_utf8_lossy(&response[..n]);
    let status_line = response_str.lines().next().unwrap_or("empty response");
    if status_line.contains("200") {
        Ok(())
    } else {
        bail!("lc_event socket returned: {status_line}");
    }
}

/// Forward an event to the local `LimaCharlie` EDR sensor via its event socket.
/// Best-effort: failures are logged but never affect the main webhook flow.
#[cfg(any(unix, windows))]
fn forward_to_edr(ppid: u32, event_id: &str, body: &str) {
    let Some(socket_path) = get_lc_socket_path() else {
        return;
    };

    let start = Instant::now();
    let result = post_to_lc_socket(&socket_path, ppid, event_id, body);
    let latency_ms = start.elapsed().as_millis();

    match result {
        Ok(()) => info!("EDR event forwarded via lc_event socket (rtt={latency_ms}ms)"),
        Err(e) => warn!("Failed to forward to lc_event socket (rtt={latency_ms}ms): {e}"),
    }
}

#[cfg(not(any(unix, windows)))]
fn forward_to_edr(_ppid: u32, _event_id: &str, _body: &str) {}

impl<'a> CloudRequestMeta<'a> {
    pub fn new(
        config: &'a Config,
        session_id: Option<String>,
        source: &'a Providers,
        query_type: CloudQueryType,
    ) -> Result<Self> {
        let ts = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .context("Unable to get current timestamp")?
            .as_millis();

        let installation_id = config.install_id.as_str();
        let request_id = Uuid::new_v4().to_string();

        let hostname = if let Ok(host) = hostname::get() {
            if let Ok(host) = host.into_string() {
                Some(host)
            } else {
                warn!("Unable to get localhostname");
                None
            }
        } else {
            warn!("Unable to get localhostname");
            None
        };

        let version = CloudRequestMetaVersion {
            version: PROJECT_VERSION,
            hash: PROJECT_VERSION_HASH,
        };

        let username = whoami::username().ok();
        let ppid = get_ppid();

        Ok(Self {
            ts,
            installation_id,
            request_id,
            hostname,
            session_id,
            source,
            query_type,
            username,
            ppid,
            version,
        })
    }
}

impl<'a> CloudQuery<'a> {
    pub fn new(config: &'a Config, provider: Providers) -> Result<Self> {
        //
        // bail if we're not actually yet authorized
        //
        if !config.org.authorized() {
            display_authorize_help();
            bail!("Not yet authorized")
        }

        info!("Authorized for oid={}", config.org.oid);

        // Parse the URL and extract the secret from the last path segment
        // URL format: https://{hooks_domain}/{oid}/{adapter_name}/{secret}
        let (url, secret) = Self::extract_secret_from_url(&config.org.url)
            .with_context(|| format!("Unable to get secret from {}", config.org.url))?;

        info!("Using url={url}");

        Ok(Self {
            config,
            url,
            secret,
            provider,
        })
    }

    /// Extract the secret from the webhook URL and return the URL without it.
    /// The secret is sent via header to avoid proxies logging it in access logs.
    fn extract_secret_from_url(full_url: &str) -> Result<(String, String)> {
        let mut parsed = url::Url::parse(full_url).context("Invalid webhook URL format")?;

        // Get path segments and extract the last one as the secret
        let segments: Vec<&str> = parsed
            .path_segments()
            .context("Webhook URL has no path segments")?
            .collect();

        if segments.len() < 3 {
            bail!("Invalid webhook URL format. Expected: https://hooks.domain/oid/name/secret");
        }

        // The last segment is the secret
        let secret = segments
            .last()
            .context("No secret segment in URL")?
            .to_string();

        if secret.is_empty() {
            bail!("Secret segment in webhook URL cannot be empty");
        }

        // Rebuild the path without the secret (we know segments.len() >= 3)
        let path_without_secret: String = segments
            .get(..segments.len().saturating_sub(1))
            .unwrap_or(&[])
            .join("/");
        parsed.set_path(&format!("/{path_without_secret}"));

        Ok((parsed.to_string(), secret))
    }

    pub fn notify(&self, data: Value) -> Result<()> {
        debug!("Preparing notification request to cloud");
        let session_id = mine_session_id(&data);
        debug!("Session ID: {session_id:?}");

        let meta_data = CloudRequestMeta::new(
            self.config,
            session_id,
            &self.provider,
            CloudQueryType::Notify,
        )?;
        let req = CloudRequest {
            meta_data,
            notify: Some(data),
            auth: None,
        };

        // Log the full request being sent to LimaCharlie
        if let Ok(pretty) = serde_json::to_string_pretty(&req) {
            debug!("CLOUD_REQUEST (notify):\n{pretty}");
        }

        // Forward to local EDR sensor if available (best-effort, before the webhook)
        if let Some(ppid) = req.meta_data.ppid
            && let Ok(body) = serde_json::to_string(&req)
        {
            forward_to_edr(ppid, "viberails_notify", &body);
        }

        debug!("Sending notification to: {}", self.url);

        // Measure API round-trip latency
        let start = Instant::now();
        let ret = minreq::post(&self.url)
            .with_timeout(CLOUD_API_TIMEOUT_SECS)
            .with_header("User-Agent", user_agent())
            .with_header("lc-secret", &self.secret)
            .with_json(&req)
            .context("Failed to serialize notification request")?
            .send();
        let latency_ms = start.elapsed().as_millis();

        match &ret {
            Ok(response) => {
                debug!("Notification response: status={}", response.status_code);
                info!(
                    "Notification sent (status={}, rtt={}ms)",
                    response.status_code, latency_ms
                );
            }
            Err(e) => {
                error!("Notification failed (rtt={latency_ms}ms): {e}");
            }
        }

        Ok(())
    }

    pub fn authorize(&self, data: Value) -> Result<CloudVerdict> {
        debug!("Preparing authorization request to cloud");
        let session_id = mine_session_id(&data);
        debug!("Session ID: {session_id:?}");

        let meta_data = CloudRequestMeta::new(
            self.config,
            session_id,
            &self.provider,
            CloudQueryType::Auth,
        )?;

        let req = CloudRequest {
            meta_data,
            auth: Some(data),
            notify: None,
        };

        // Log the full request being sent to LimaCharlie
        if let Ok(pretty) = serde_json::to_string_pretty(&req) {
            debug!("CLOUD_REQUEST (auth):\n{pretty}");
        }

        // Forward to local EDR sensor if available (best-effort, before the webhook)
        if let Some(ppid) = req.meta_data.ppid
            && let Ok(body) = serde_json::to_string(&req)
        {
            forward_to_edr(ppid, "viberails_auth", &body);
        }

        debug!("Sending authorization to: {}", self.url);
        debug!("Timeout: {CLOUD_API_TIMEOUT_SECS}s");

        // Measure API round-trip latency
        let start = Instant::now();
        let res = minreq::post(&self.url)
            .with_timeout(CLOUD_API_TIMEOUT_SECS)
            .with_header("User-Agent", user_agent())
            .with_header("lc-secret", &self.secret)
            .with_json(&req)
            .context("Failed to serialize authorization request")?
            .send()
            .with_context(|| format!("Failed to connect to hook server at {}", self.url))?;
        let latency_ms = start.elapsed().as_millis();

        debug!(
            "Authorization response: status={}, rtt={}ms",
            res.status_code, latency_ms
        );

        if !(200..300).contains(&res.status_code) {
            let error_body = res.as_str().unwrap_or("Unknown error");
            anyhow::bail!(
                "Authorization request failed with status {}: {}",
                res.status_code,
                error_body
            );
        }

        let data = res.as_str()?;
        debug!("Cloud response body: {data}");

        let data: CloudResponse = res
            .json()
            .context("Authorization server returned invalid JSON response")?;

        info!(
            "Authorization result: allow={} reason={:?} (rtt={}ms)",
            data.success, data.reason, latency_ms
        );

        let verdict = if data.success {
            CloudVerdict::Allow
        } else {
            let msg = data.block_message();
            debug!("Block message: {msg}");
            CloudVerdict::Deny(msg)
        };

        Ok(verdict)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_ppid_returns_some() {
        let ppid = get_ppid();
        assert!(ppid.is_some(), "get_ppid() should return Some on Unix/Windows");
        assert!(ppid.is_some_and(|p| p > 0), "ppid should be > 0");
    }

    #[cfg(unix)]
    mod edr_socket_tests {
        #![allow(clippy::unwrap_used)]

        use super::*;

        /// Spin up a throwaway Unix-domain listener that accepts one connection,
        /// captures the raw HTTP request, and replies with the given response bytes.
        fn mock_lc_socket(
            response: &'static [u8],
        ) -> (std::path::PathBuf, std::thread::JoinHandle<String>) {
            let dir = tempfile::tempdir().unwrap();
            let socket_path = dir.keep().join("lc_event.sock");
            let listener = std::os::unix::net::UnixListener::bind(&socket_path).unwrap();

            let path = socket_path.clone();
            let handle = std::thread::spawn(move || {
                use std::io::{Read, Write};

                let (mut stream, _) = listener.accept().unwrap();
                let mut buf = vec![0u8; 65536];
                let n = stream.read(&mut buf).unwrap();
                let _ = stream.write_all(response);
                String::from_utf8_lossy(&buf[..n]).into_owned()
            });

            (path, handle)
        }

        const OK_RESPONSE: &[u8] =
            b"HTTP/1.0 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok";

        #[test]
        fn test_post_sends_correct_http_request() {
            let (path, handle) = mock_lc_socket(OK_RESPONSE);

            let body = r#"{"test":"data"}"#;
            let result = post_to_lc_socket(&path, 1234, "viberails_auth", body);
            assert!(result.is_ok(), "expected Ok, got: {result:?}");

            let request = handle.join().unwrap();
            assert!(
                request.starts_with(
                    "POST /event?ppid=1234&event_id=viberails_auth HTTP/1.0\r\n"
                ),
                "unexpected request line: {request}"
            );
            assert!(
                request.contains("Content-Length: 15\r\n"),
                "missing or wrong Content-Length in: {request}"
            );
            assert!(
                request.contains(body),
                "body not found in request: {request}"
            );
        }

        #[test]
        fn test_post_url_encodes_event_id() {
            let (path, handle) = mock_lc_socket(OK_RESPONSE);

            let result = post_to_lc_socket(&path, 42, "hello world", "x");
            assert!(result.is_ok());

            let request = handle.join().unwrap();
            assert!(
                request.contains("event_id=hello%20world"),
                "event_id not URL-encoded in: {request}"
            );
        }

        #[test]
        fn test_post_drops_body_when_oversized() {
            let (path, handle) = mock_lc_socket(OK_RESPONSE);

            let oversized = "x".repeat(MAX_LC_SOCKET_BODY + 1);
            let result = post_to_lc_socket(&path, 1, "big", &oversized);
            assert!(result.is_ok());

            let request = handle.join().unwrap();
            assert!(
                request.contains("Content-Length: 0\r\n"),
                "oversized body was not dropped: {request}"
            );
        }

        #[test]
        fn test_post_returns_error_on_non_200() {
            let (path, handle) = mock_lc_socket(
                b"HTTP/1.0 400 Bad Request\r\nContent-Length: 22\r\nConnection: close\r\n\r\nmissing ppid parameter",
            );

            // Use empty body to avoid a broken-pipe race: the mock server may
            // close the connection before the client finishes writing a body.
            let result = post_to_lc_socket(&path, 1, "test", "");
            assert!(result.is_err());

            let err_msg = format!("{:#}", result.unwrap_err());
            assert!(
                err_msg.contains("400"),
                "error should mention status code: {err_msg}"
            );

            handle.join().unwrap();
        }

        #[test]
        fn test_post_returns_error_on_connection_failure() {
            let result =
                post_to_lc_socket(std::path::Path::new("/tmp/no_such_socket"), 1, "t", "b");
            assert!(result.is_err());
        }

        #[test]
        fn test_get_lc_socket_path_returns_none_when_absent() {
            // The socket shouldn't exist in a normal test environment.
            // If it does (lc_sensor running), that's fine — just verify no panic.
            let _ = get_lc_socket_path();
        }

        #[test]
        fn test_forward_to_edr_is_noop_when_socket_absent() {
            // Should silently return without error.
            forward_to_edr(1234, "viberails_auth", r#"{"test":true}"#);
        }
    }
}
