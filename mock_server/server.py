#!/usr/bin/env -S uv run --script
# /// script
# dependencies = ["aiohttp"]
# ///
"""
Mock server for testing the viberails client locally.

Routes:
  POST /auth   - Authentication endpoint
  POST /dnr    - Authorization endpoint
  POST /notify - Notification endpoint
"""

import json

from aiohttp import web

HOST = "localhost"
PORT = 8000


async def auth_handler(request: web.Request) -> web.Response:
    """Handle authentication requests."""
    try:
        data = await request.json()
        print(f"[/auth] Received: {json.dumps(data, indent=2)}")
    except json.JSONDecodeError:
        print("[/auth] Received request (no JSON body)")

    return web.json_response({"authenticated": True})


async def authorize_handler(request: web.Request) -> web.Response:
    """
    Handle authorization requests.

    Expected request format:
      {"ts": <timestamp_ms>, "hook_data": <string>}

    Response format:
      {"allow": <bool>, "reason": <string>}
    """
    try:
        data = await request.json()
        print(f"[/dnr] Received: {json.dumps(data, indent=2)}")
        ts = data.get("ts")
        hook_data = data.get("hook_data", "")
        print(f"[/dnr] Timestamp: {ts}, Hook data length: {len(hook_data)}")
    except json.JSONDecodeError:
        print("[/dnr] Received request (no JSON body)")
        return web.json_response({"allow": False, "reason": "Invalid request format"})

    return web.json_response({"allow": True, "reason": ""})


async def notify_handler(request: web.Request) -> web.Response:
    """Handle notification requests."""
    try:
        data = await request.json()
        print(f"[/notify] Received: {json.dumps(data, indent=2)}")
    except json.JSONDecodeError:
        print("[/notify] Received request (no JSON body)")

    return web.json_response({"status": "ok"})


def create_app() -> web.Application:
    app = web.Application()
    app.router.add_post("/auth", auth_handler)
    app.router.add_post("/dnr", authorize_handler)
    app.router.add_post("/notify", notify_handler)
    return app


if __name__ == "__main__":
    print(f"Starting mock server on http://{HOST}:{PORT}")
    print("Routes:")
    print("  POST /auth   - Authentication")
    print("  POST /dnr    - Authorization")
    print("  POST /notify - Notification")
    print()
    app = create_app()
    web.run_app(app, host=HOST, port=PORT)
