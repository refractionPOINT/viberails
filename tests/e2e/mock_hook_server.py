#!/usr/bin/env python3
"""Minimal mock hook server for e2e tests.

Parameters:
  --port: Local TCP port to bind on 127.0.0.1.
  --capture-file: File path to write the raw HTTP request body.

Return:
  Process exit code (0 on graceful stop, non-zero on startup/runtime errors).
"""

import argparse
import json
from http.server import BaseHTTPRequestHandler, HTTPServer


def parse_args() -> argparse.Namespace:
    """Parse CLI arguments for mock server startup.

    Parameters:
      None

    Return:
      Parsed argparse.Namespace with `port` and `capture_file`.
    """
    parser = argparse.ArgumentParser(description="Mock hook server for viberails e2e tests")
    parser.add_argument("--port", type=int, required=True, help="Port to bind")
    parser.add_argument(
        "--capture-file",
        type=str,
        required=True,
        help="File path where request body is written",
    )
    return parser.parse_args()


def make_handler(capture_file: str):
    """Build a request handler that captures POST bodies.

    Parameters:
      capture_file: Destination file path for captured JSON bytes.

    Return:
      BaseHTTPRequestHandler subclass instance type.
    """

    class Handler(BaseHTTPRequestHandler):
        """Request handler for a simple JSON hook endpoint.

        Parameters:
          Inherited from BaseHTTPRequestHandler.

        Return:
          Standard BaseHTTPRequestHandler behavior.
        """

        def log_message(self, _format: str, *_args) -> None:
            """Suppress default request logging noise in test output.

            Parameters:
              _format: Unused log format string.
              _args: Unused formatting args.

            Return:
              None
            """

        def do_POST(self) -> None:  # noqa: N802 - BaseHTTPRequestHandler method name
            """Handle POST by saving body and returning success JSON.

            Parameters:
              None (uses request stream from self.rfile).

            Return:
              None
            """
            length_header = self.headers.get("Content-Length", "0")
            content_length = int(length_header) if length_header.isdigit() else 0
            body = self.rfile.read(content_length)

            with open(capture_file, "wb") as output:
                output.write(body)

            response = json.dumps({"success": True}).encode("utf-8")
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(response)))
            self.end_headers()
            self.wfile.write(response)

    return Handler


def main() -> None:
    """Start the mock HTTP server and run forever.

    Parameters:
      None

    Return:
      None
    """
    args = parse_args()
    handler = make_handler(args.capture_file)
    server = HTTPServer(("127.0.0.1", args.port), handler)
    server.serve_forever()


if __name__ == "__main__":
    main()
