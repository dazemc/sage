#!/usr/bin/env bash
# Serve OpenJev: shim on 3010, model-discovery front on 3000.
set -u

MODEL="${OPENJEV_OLLAMA_MODEL:-qwen}"
OLLAMA="${OLLAMA_URL:-http://127.0.0.1:11434}"
SHIM_PORT="${OPENJEV_SHIM_PORT:-3010}"
FRONT_PORT="${OPENJEV_PORT:-3000}"

# Ollama is socket-activated: do NOT touch it here, or the daemon (and the 5.9 GB
# model) would be started and pinned at boot. Warm on demand with warm.sh.
if [ "${OPENJEV_WARM:-0}" = "1" ]; then
  for _ in $(seq 1 30); do
    if curl -sf --max-time 5 "$OLLAMA/api/tags" >/dev/null; then break; fi
    sleep 2
  done
  curl -s --max-time 120 "$OLLAMA/api/generate" \
    -d "{\"model\":\"$MODEL\",\"prompt\":\"\",\"stream\":false,\"keep_alive\":\"${OPENJEV_KEEP:-5m}\",\"options\":{\"num_predict\":1}}" \
    -o /dev/null || echo "openjev: warm-up failed, continuing" >&2
fi

/home/daze/.local/share/openjev/venv/bin/python \
  /home/daze/.local/share/openjev/helper/shim.py --host 127.0.0.1 --port "$SHIM_PORT" &
SHIM_PID=$!

# Wait for the shim to bind before the front starts forwarding.
for _ in $(seq 1 60); do
  if (exec 3<>/dev/tcp/127.0.0.1/"$SHIM_PORT") 2>/dev/null; then exec 3<&- 3>&-; break; fi
  sleep 0.5
done

OPENJEV_SHIM="http://127.0.0.1:$SHIM_PORT" OPENJEV_PORT="$FRONT_PORT" \
  exec /home/daze/.local/share/openjev/venv/bin/python /home/daze/.local/share/openjev/front.py
