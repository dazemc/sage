#!/usr/bin/env bash
# Re-pin the decision model so routing never pays a cold load.
set -u
curl -sf --max-time 120 http://127.0.0.1:11434/api/generate \
  -H 'Content-Type: application/json' \
  -d '{"model":"qwen","prompt":"","stream":false,"keep_alive":-1,"options":{"num_predict":1}}' \
  -o /dev/null
