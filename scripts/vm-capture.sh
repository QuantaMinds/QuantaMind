#!/usr/bin/env bash
# Runs ON the GPU VM. Ensures Docker+toolkit, then for each engine:
# serve (detached) -> wait ready -> capture via ~/capture-fixtures.sh -> tear down.
# Writes ~/capture-done on completion. No `set -e`: one engine failing must not
# abort the other.
set -uo pipefail
cd "$HOME"
exec > >(tee -a "$HOME/capture.log") 2>&1
echo "=== capture start $(date -u) ==="

# Start from a clean slate so re-captures never carry stale/renamed fixtures or
# stale failure records.
rm -rf "$HOME/fixtures" "$HOME/capture-failures.txt" "$HOME/capture-failed"

# One-time (idempotent) Docker + NVIDIA Container Toolkit install.
bash "$HOME/vm-setup.sh"

# Error surfacing (mandate: never swallow). On a startup failure, classify WHY from
# the container logs and record what/why/how to capture-failures.txt so gpu.sh can
# highlight it to the user; the run then fails loudly instead of shipping nothing.
record_failure () {
  local engine="$1"; shift
  local logs; logs="$(sudo docker logs "$engine" 2>&1 | tail -40)"
  local why="unknown startup failure — read the logs below"
  if   grep -qiE "out of memory|CUDA out of memory|OutOfMemory" <<<"$logs"; then
    why="GPU out of memory — model or --max-model-len too large for this GPU"
  elif grep -qiE "no space left on device" <<<"$logs"; then
    why="disk full on the VM boot disk"
  elif grep -qiE "address already in use|bind.*in use" <<<"$logs"; then
    why="port :$engine already in use — a previous container didn't stop"
  elif grep -qiE "manifest.*not found|pull access denied|error pulling image|toomanyrequests|failed to resolve" <<<"$logs"; then
    why="Docker image pull failed — bad tag or registry rate-limit"
  elif grep -qiE "Repository Not Found|gated repo|401 Client Error|is not a valid model|does not appear to have" <<<"$logs"; then
    why="model weights unavailable — bad model id or gated repo needing HF_TOKEN"
  fi
  {
    echo "=========================================="
    echo "ENGINE: $engine"
    echo "WHAT:   $engine never became ready; no fixtures captured for it."
    echo "WHY:    $why"
    echo "HOW:    sudo docker run --gpus all -d --name $engine $*"
    echo "LOGS (last 40 lines):"
    echo "$logs"
    echo "=========================================="
  } >> "$HOME/capture-failures.txt"
  touch "$HOME/capture-failed"
  echo "!!! $engine FAILED: $why (recorded to capture-failures.txt)"
}

serve_capture () {
  local engine="$1"; local port="$2"; shift 2
  echo "--- $engine: (re)starting container ---"
  sudo docker rm -f "$engine" >/dev/null 2>&1 || true
  sudo docker run --gpus all -d --name "$engine" "$@"
  echo "--- $engine: waiting for readiness on :$port (up to ~15 min) ---"
  local ready=0
  for _ in $(seq 1 180); do
    if curl -sf "localhost:$port/v1/models" >/dev/null 2>&1; then ready=1; break; fi
    sleep 5
  done
  if [ "$ready" -eq 1 ]; then
    "$HOME/capture-fixtures.sh" "$engine"
  else
    record_failure "$engine" "$@"
  fi
  sudo docker rm -f "$engine" >/dev/null 2>&1 || true
}

serve_capture vllm 8000 \
  -p 8000:8000 vllm/vllm-openai:latest \
  --model Qwen/Qwen3-0.6B --dtype auto --max-model-len 8192 \
  --enable-auto-tool-choice --tool-call-parser hermes

serve_capture sglang 30000 \
  -p 30000:30000 lmsysorg/sglang:latest \
  python3 -m sglang.launch_server --model-path Qwen/Qwen3-0.6B \
  --host 0.0.0.0 --port 30000 --enable-metrics

echo "=== capture done $(date -u) ==="
touch "$HOME/capture-done"
