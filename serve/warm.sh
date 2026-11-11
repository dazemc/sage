#!/usr/bin/env bash
# Warm the OpenJev decision model now, for a bounded time.
# Ollama is socket-activated and idle-exits, so this is opt-in: run it before a
# burst of routing (e.g. a long agent session) and the first decision is fast.
set -u

MODEL="${OPENJEV_OLLAMA_MODEL:-qwen}"
OLLAMA="${OLLAMA_URL:-http://127.0.0.1:11434}"
KEEP="${OPENJEV_KEEP:-5m}"

curl -sf --max-time 120 "$OLLAMA/api/generate" \
  -H 'Content-Type: application/json' \
  -d "{\"model\":\"$MODEL\",\"prompt\":\"\",\"stream\":false,\"keep_alive\":\"$KEEP\",\"options\":{\"num_predict\":1}}" \
  -o /dev/null && echo "openjev: $MODEL warmed for $KEEP"
