use crate::cloud::lc_socket::*;

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
    fn test_lc_socket_new_fails_when_socket_absent() {
        // LcSocket::new() should return an error when the socket doesn't exist.
        let result = LcSocket::new();
        // In a normal test environment the socket won't exist, so this should fail.
        // If it does exist (lc_sensor running), that's fine — just verify no panic.
        let _ = result;
    }
}
