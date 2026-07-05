#!/usr/bin/env bash
# capture-fixtures.sh
# Run ON the GCP L4 VM (after Docker + NVIDIA runtime are up).
# Captures real vLLM and SGLang responses so you can build the Rust
# foundation OFFLINE against wiremock. One GPU session; then stop the VM.
#
# Usage:
#   VLLM:   ./capture-fixtures.sh vllm
#   SGLANG: ./capture-fixtures.sh sglang
#
# Tiny model on purpose — you are capturing the API ENVELOPE + /metrics
# FORMAT, not doing real inference. 0.6B loads in seconds and fits L4 easily.

set -euo pipefail

ENGINE="${1:?usage: $0 <vllm|sglang>}"
MODEL="${MODEL:-Qwen/Qwen3-0.6B}"
OUT="fixtures/${ENGINE}"
mkdir -p "$OUT"

case "$ENGINE" in
  vllm)   BASE="http://localhost:8000"; METRICS="${BASE}/metrics" ;;
  sglang) BASE="http://localhost:30000"; METRICS="${BASE}/metrics" ;;
  *) echo "unknown engine: $ENGINE" >&2; exit 2 ;;
esac

echo "== waiting for ${ENGINE} at ${BASE} =="
for i in $(seq 1 120); do
  if curl -sf "${BASE}/health" >/dev/null 2>&1 || curl -sf "${BASE}/v1/models" >/dev/null 2>&1; then
    echo "up."; break
  fi
  sleep 2
done

echo "== 1. capabilities: /v1/models =="
curl -s "${BASE}/v1/models" | tee "${OUT}/models.json" >/dev/null

echo "== 2. health =="
curl -s "${BASE}/health" -o "${OUT}/health.txt" -w "http_status=%{http_code}\n" | tee "${OUT}/health.meta" || true

echo "== 3. plain chat completion =="
curl -s "${BASE}/v1/chat/completions" \
  -H 'Content-Type: application/json' \
  -d "{
    \"model\": \"${MODEL}\",
    \"messages\": [{\"role\": \"user\", \"content\": \"Reply with exactly: OK\"}],
    \"max_tokens\": 16, \"temperature\": 0
  }" | tee "${OUT}/chat_plain.json" >/dev/null

echo "== 4. tool call (this is the agentic-readiness signal) =="
curl -s "${BASE}/v1/chat/completions" \
  -H 'Content-Type: application/json' \
  -d "{
    \"model\": \"${MODEL}\",
    \"messages\": [{\"role\": \"user\", \"content\": \"What is the weather in Bangalore?\"}],
    \"tools\": [{
      \"type\": \"function\",
      \"function\": {
        \"name\": \"get_weather\",
        \"description\": \"Get current weather for a city\",
        \"parameters\": {
          \"type\": \"object\",
          \"additionalProperties\": false,
          \"properties\": {\"city\": {\"type\": \"string\"}},
          \"required\": [\"city\"]
        }
      }
    }],
    \"tool_choice\": \"auto\",
    \"max_tokens\": 128, \"temperature\": 0
  }" | tee "${OUT}/chat_toolcall.json" >/dev/null

echo "== 5. structured output (json_schema / guided decoding) =="
curl -s "${BASE}/v1/chat/completions" \
  -H 'Content-Type: application/json' \
  -d "{
    \"model\": \"${MODEL}\",
    \"messages\": [{\"role\": \"user\", \"content\": \"Give a city and its country.\"}],
    \"response_format\": {
      \"type\": \"json_schema\",
      \"json_schema\": {
        \"name\": \"city\",
        \"schema\": {
          \"type\": \"object\",
          \"additionalProperties\": false,
          \"properties\": {\"city\": {\"type\": \"string\"}, \"country\": {\"type\": \"string\"}},
          \"required\": [\"city\", \"country\"]
        }
      }
    },
    \"max_tokens\": 64, \"temperature\": 0
  }" | tee "${OUT}/chat_structured.json" >/dev/null

echo "== 6. metrics (Prometheus text format) =="
# SGLang needs --enable-metrics at launch for this to be populated.
curl -s "${METRICS}" -o "${OUT}/metrics.prom" -w "http_status=%{http_code}\n" || \
  echo "WARN: /metrics not available (SGLang: did you launch with --enable-metrics?)"

echo
echo "== done. fixtures in ${OUT}/ =="
ls -la "${OUT}"
echo
echo "Next: scp these back to your dev machine into crates/qm-backends/tests/fixtures/${ENGINE}/"
