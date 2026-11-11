#!/usr/bin/env bash
# Stop the on-demand Ollama daemon once no model is resident.
# ollama.socket keeps listening on 11434, so the next client request starts
# ollama.service again. Stopping the proxy first lets ollama.service's
# StopWhenUnneeded fire.
set -u

OLLAMA="${OLLAMA_URL:-http://127.0.0.1:11435}"
# Query the daemon directly on 11435: probing 11434 would wake the socket.
if ! curl -sf --max-time 5 "$OLLAMA/api/ps" -o /tmp/ollama-ps.$$ 2>/dev/null; then
  rm -f /tmp/ollama-ps.$$
  exit 0  # daemon not running: nothing to do
fi

loaded=$(python3 -c 'import json,sys;print(len(json.load(open(sys.argv[1])).get("models") or []))' /tmp/ollama-ps.$$ 2>/dev/null || echo 0)
rm -f /tmp/ollama-ps.$$
[ "$loaded" = "0" ] || exit 0

systemctl --user stop ollama-proxy.service 2>/dev/null || true
systemctl --user stop ollama.service 2>/dev/null || true
