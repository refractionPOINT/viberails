use std::time::Instant;

use anyhow::{Context, Result, bail};
use log::{info, warn};

/// Maximum body size accepted by the `lc_sensor` event socket (60 KB).
pub(crate) const MAX_LC_SOCKET_BODY: usize = 60 * 1024;

/// Return the path to the `LimaCharlie` EDR event socket, if it exists on disk.
///
/// The sensor picks its socket path based on **its own** euid, not ours.
/// In production the sensor runs as root (`/var/run/lc_event.sock`); in
/// debug/dev it may run as a regular user (`$HOME/.local/run/lc_event.sock`).
/// We probe both locations, production first.
#[cfg(unix)]
pub(crate) fn get_lc_socket_path() -> Option<std::path::PathBuf> {
    use std::path::PathBuf;

    // Production: sensor runs as root.
    let root_path = PathBuf::from("/var/run/lc_event.sock");
    if root_path.exists() {
        return Some(root_path);
    }

    // Debug / dev: sensor runs as current user.
    if let Ok(home) = std::env::var("HOME") {
        let user_path = PathBuf::from(home).join(".local/run/lc_event.sock");
        if user_path.exists() {
            return Some(user_path);
        }
    }

    None
}

/// Send an HTTP POST to the `lc_sensor` event socket at the given path.
///
/// Sends `POST /event?ppid=<ppid>&event_id=<event_id>` with the body as the
/// request payload.  If the body exceeds the sensor's 60 KB limit it is dropped
/// and only the metadata (ppid + `event_id`) is sent.
#[cfg(unix)]
pub(crate) fn post_to_lc_socket(
    socket_path: &std::path::Path,
    ppid: u32,
    event_id: &str,
    body: &str,
) -> Result<()> {
    use std::io::{Read, Write};
    use std::os::unix::net::UnixStream;
    use std::time::Duration;

    let mut stream = UnixStream::connect(socket_path).context("connect to lc_event socket")?;
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
    stream.write_all(body_bytes).context("write request body")?;

    let mut response = Vec::new();
    stream.read_to_end(&mut response).context("read response")?;

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
pub(crate) fn get_lc_socket_path() -> Option<std::path::PathBuf> {
    use std::path::PathBuf;

    let program_data = std::env::var("ProgramData").ok()?;
    let path = PathBuf::from(program_data)
        .join("limacharlie")
        .join("lc_event.sock");

    if path.exists() { Some(path) } else { None }
}

/// Send an HTTP POST to the `lc_sensor` event socket on Windows via `AF_UNIX`.
#[cfg(windows)]
pub(crate) fn post_to_lc_socket(
    socket_path: &std::path::Path,
    ppid: u32,
    event_id: &str,
    body: &str,
) -> Result<()> {
    use windows_sys::Win32::Networking::WinSock::{
        AF_UNIX, INVALID_SOCKET, SO_RCVTIMEO, SO_SNDTIMEO, SOCK_STREAM, SOCKADDR, SOL_SOCKET,
        WSACleanup, WSADATA, WSAStartup, closesocket, connect, recv, send, setsockopt, socket,
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

pub fn edr_link_available() -> bool {
    get_lc_socket_path().is_some()
}

/// Forward an event to the local `LimaCharlie` EDR sensor via its event socket.
/// Best-effort: failures are logged but never affect the main webhook flow.
pub fn forward_to_edr(ppid: u32, event_id: &str, body: &str) {
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
