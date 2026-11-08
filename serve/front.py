#!/usr/bin/env python3
"""Front for the OpenJev shim.

The shim (upstream helper/shim.py) serves POST /v1/systemone but has no model
discovery endpoint. OpenClaw's decision provider needs GET /v1/models to learn
which model ids exist before it will offer the decision tool. This process keeps
the shim unmodified: it answers /v1/models itself and proxies everything else.
"""
import json
import os
import sys
import time
import urllib.error
import urllib.request
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

LISTEN_HOST = os.environ.get("OPENJEV_HOST", "127.0.0.1")
LISTEN_PORT = int(os.environ.get("OPENJEV_PORT", "3000"))
UPSTREAM = os.environ.get("OPENJEV_SHIM", "http://127.0.0.1:3010")
MODELS = [m.strip() for m in os.environ.get("OPENJEV_MODELS", "kev-latest,jev-latest,openjev-flash-9b").split(",") if m.strip()]


def models_payload():
    now = int(time.time())
    return {
        "object": "list",
        "data": [
            {
                "id": mid,
                "object": "model",
                "created": now,
                "owned_by": "openjev-local",
                "root": mid,
            }
            for mid in MODELS
        ],
    }


def normalize(payload, request_body):
    """Rewrite fields the OpenClaw adapter validates strictly.

    The shim reports `model` as a long debug banner; the adapter requires
    ^[a-zA-Z0-9._/-]+$ under 128 chars. usage counters must be integers.
    """
    try:
        data = json.loads(payload)
    except Exception:  # noqa: BLE001
        return payload
    if not isinstance(data, dict):
        return payload
    try:
        requested = json.loads(request_body or b"{}").get("model")
    except Exception:  # noqa: BLE001
        requested = None
    model = requested if isinstance(requested, str) and requested else MODELS[0]
    data["model"] = model
    usage = data.get("usage")
    if isinstance(usage, dict):
        for key in ("input_tokens", "output_tokens"):
            try:
                usage[key] = int(usage.get(key) or 0)
            except Exception:  # noqa: BLE001
                usage[key] = 0
    else:
        usage = {"input_tokens": 0, "output_tokens": 0}
    # The adapter's schema forbids unknown keys at every level; rebuild strictly.
    strict = {
        "model": model,
        "answers": data.get("answers") or {},
        "usage": {"input_tokens": usage["input_tokens"], "output_tokens": usage["output_tokens"]},
    }
    return json.dumps(strict).encode()


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *args):  # quiet
        pass

    def _send(self, code, body, ctype="application/json"):
        raw = body if isinstance(body, bytes) else json.dumps(body).encode()
        self.send_response(code)
        self.send_header("Content-Type", ctype)
        self.send_header("Content-Length", str(len(raw)))
        self.end_headers()
        self.wfile.write(raw)

    def do_GET(self):
        path = self.path.split("?")[0].rstrip("/")
        if path in ("/v1/models", "/models"):
            self._send(200, models_payload())
        elif path in ("/health", "/healthz"):
            self._send(200, {"ok": True})
        else:
            self._send(404, {"error": {"code": 404, "message": f"GET {self.path}"}})

    def do_POST(self):
        length = int(self.headers.get("Content-Length") or 0)
        body = self.rfile.read(length) if length else b""
        if self.path.split("?")[0].rstrip("/") not in ("/v1/systemone", "/systemone"):
            self._send(404, {"error": {"code": 404, "message": f"POST {self.path}"}})
            return
        req = urllib.request.Request(
            UPSTREAM.rstrip("/") + "/v1/systemone",
            data=body,
            headers={"Content-Type": "application/json"},
            method="POST",
        )
        logpath = os.environ.get("OPENJEV_LOG")
        try:
            with urllib.request.urlopen(req, timeout=120) as r:
                payload = r.read()
                payload = normalize(payload, body)
                if logpath:
                    try:
                        with open(logpath, "a") as fh:
                            fh.write(json.dumps({"request": json.loads(body or b"{}"), "response": json.loads(payload or b"{}")}) + "\n")
                    except Exception:  # noqa: BLE001
                        pass
                self._send(r.status, payload)
        except urllib.error.HTTPError as e:
            err_body = e.read()
            if logpath:
                try:
                    with open(logpath, "a") as fh:
                        fh.write(json.dumps({"request": json.loads(body or b"{}"), "status": e.code, "error": err_body.decode(errors="replace")[:2000]}) + "\n")
                except Exception:  # noqa: BLE001
                    pass
            self._send(e.code, err_body)
        except Exception as e:  # noqa: BLE001
            print(f"[openjev-front] upstream error: {type(e).__name__}: {e}", file=sys.stderr, flush=True)
            self._send(503, {"error": {"code": 503, "message": "decision backend unavailable"}})


if __name__ == "__main__":
    srv = ThreadingHTTPServer((LISTEN_HOST, LISTEN_PORT), Handler)
    print(f"[openjev-front] {LISTEN_HOST}:{LISTEN_PORT} -> {UPSTREAM}, models={MODELS}", flush=True)
    srv.serve_forever()
