#!/usr/bin/env bash
# GPU ops for QuantaMind fixture capture (Track 1).
#
# One command, GPU/zone-independent: provisions ANY available Spot GPU (L4/T4/A100/V100/P100), installs
# Docker + NVIDIA toolkit, serves vLLM, captures fixtures, runs the tests.
# The VM is STOPPED (not deleted) between uses, so restarting is one command with
# no re-setup — the boot disk keeps the driver + Docker images. If the Spot VM is
# preempted/reallotted, the same command rebuilds it; nothing depends on a
# specific instance or zone.
#
#   ./scripts/gpu.sh run       FULL pipeline: provision -> setup -> serve -> capture -> cargo test
#   ./scripts/gpu.sh find vllm                 probe EVERY GPU type x zone for Spot capacity
#                                              (L4/T4/A100/V100/P100), provision where it's free
#                                              (fresh disk -> auto Docker setup + right dtype), serve
#   ./scripts/gpu.sh up        create if missing (zone fallback), else start; wait SSH-ready
#   ./scripts/gpu.sh capture   serve both engines, capture fixtures, pull into the repo
#   ./scripts/gpu.sh ssh       interactive shell on the VM
#   ./scripts/gpu.sh resume    up + ssh   (the "come back later" one-liner)
#   ./scripts/gpu.sh serve vllm                start the engine container, wait until serving
#   ./scripts/gpu.sh unserve [vllm|all]        stop the engine container (VM stays up)
#   ./scripts/gpu.sh vram      GPU memory used,total (MiB) via nvidia-smi
#   ./scripts/gpu.sh tunnel vllm               forward localhost:<port> to it (Ctrl-C to stop)
#   ./scripts/gpu.sh down      STOP the VM (GPU billing off, disk kept)
#   ./scripts/gpu.sh delete    delete the VM entirely (only when fully done)
#   ./scripts/gpu.sh status    show VM status + zone
#
# Engines are served one at a time (one L4); `serve` stops the other first so they
# don't fight for VRAM. vLLM on :8000.
#
# Env overrides: QM_GPU_PROJECT, QM_GPU_ZONES (space-separated), QM_GPU_VM,
#                QM_GPU_MACHINE, QM_GPU_IMAGE_FAMILY, QM_GPU_PROFILES,
#                QM_GPU_MODEL = hf-repo (default) | gs://bucket/model (GCS, via gcsfuse)
#                             | s3://bucket/model (vLLM Run:ai streamer) | /local/path.
set -uo pipefail

export PATH="/opt/homebrew/share/google-cloud-sdk/bin:$PATH"
PROJECT="${QM_GPU_PROJECT:-quantamind-oss}"
ZONES="${QM_GPU_ZONES:-us-central1-a us-central1-b us-central1-c us-east1-b us-east1-c us-east1-d us-east4-a us-east4-b us-east4-c us-east5-a us-west1-a us-west1-b us-west4-a us-south1-a}"
# GPU profiles to try, best/most-capable first — each "NAME|MACHINE|EXTRA-create-flags".
# L4 & A100 are built into g2/a2 machines; T4/V100/P100 attach via --accelerator on n1.
# Override with QM_GPU_PROFILES (space-separated; no field may contain a space).
if [ -n "${QM_GPU_PROFILES:-}" ]; then
  # shellcheck disable=SC2206
  GPU_PROFILES=($QM_GPU_PROFILES)
else
  GPU_PROFILES=(
    "L4|g2-standard-4|"
    "T4|n1-standard-4|--accelerator=type=nvidia-tesla-t4,count=1"
    "A100|a2-highgpu-1g|"
    "V100|n1-standard-4|--accelerator=type=nvidia-tesla-v100,count=1"
    "P100|n1-standard-4|--accelerator=type=nvidia-tesla-p100,count=1"
  )
fi
VM="${QM_GPU_VM:-qm-gpu-test}"
MACHINE="${QM_GPU_MACHINE:-g2-standard-4}"
IMAGE_FAMILY="${QM_GPU_IMAGE_FAMILY:-common-cu129-ubuntu-2204-nvidia-580}"
MODEL="${QM_GPU_MODEL:-Qwen/Qwen3-0.6B}"
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
G=(gcloud --project "$PROJECT")

# Zone the VM actually lives in (empty if it doesn't exist yet) — makes every
# command work regardless of which zone the GPU was allotted in.
find_zone () { "${G[@]}" compute instances list --filter="name=$VM" --format="value(zone)" 2>/dev/null | head -1; }
require_zone () {
  local z; z="$(find_zone)"
  [ -n "$z" ] || { echo "VM $VM not found — run: ./scripts/gpu.sh up" >&2; exit 1; }
  printf '%s' "$z"
}

# Fail fast + clearly if gcloud creds expired — otherwise every command dies mid-way
# with a confusing non-interactive reauth error (never swallow: what + how to fix).
require_auth () {
  if ! "${G[@]}" auth print-access-token >/dev/null 2>&1; then
    {
      echo "gcloud credentials expired or missing — cannot reach GCP."
      echo "  FIX: run  gcloud auth login  (interactive), then re-run this command."
      echo "  In this Claude Code session, type:  ! gcloud auth login"
    } >&2
    exit 1
  fi
}

ensure_network () {
  "${G[@]}" compute networks describe default >/dev/null 2>&1 || {
    echo "creating 'default' auto network…"; "${G[@]}" compute networks create default --subnet-mode=auto; }
  "${G[@]}" compute firewall-rules describe default-allow-ssh >/dev/null 2>&1 || {
    echo "adding SSH firewall rule (tcp:22)…"
    "${G[@]}" compute firewall-rules create default-allow-ssh --network=default --allow=tcp:22 --source-ranges=0.0.0.0/0; }
}

create_vm () {
  local z
  for z in $ZONES; do
    echo "trying Spot L4 in ${z}…"
    if "${G[@]}" compute instances create "$VM" --zone "$z" \
        --machine-type="$MACHINE" \
        --provisioning-model=SPOT --instance-termination-action=STOP \
        --maintenance-policy=TERMINATE \
        --image-family="$IMAGE_FAMILY" --image-project=deeplearning-platform-release \
        --boot-disk-size=150GB 2>/tmp/qm-create-err; then
      echo "created in $z"; return 0
    fi
    if grep -qiE "quota|permission|not found|invalid" /tmp/qm-create-err; then
      cat /tmp/qm-create-err >&2; echo "non-capacity error — stopping." >&2; return 1
    fi
    echo "  $z unavailable (capacity); next…"
  done
  echo "no zone had Spot L4 capacity." >&2; return 1
}

# Provision the first available Spot GPU of ANY type: every (GPU profile x zone) until
# one has capacity + quota. Stops only on a fatal auth/permission/billing error.
create_any () {
  local profile name machine extra z
  for profile in "${GPU_PROFILES[@]}"; do
    IFS='|' read -r name machine extra <<<"$profile"
    for z in $ZONES; do
      echo "trying Spot $name ($machine) in ${z}…"
      # shellcheck disable=SC2086
      if "${G[@]}" compute instances create "$VM" --zone "$z" \
          --machine-type="$machine" \
          --provisioning-model=SPOT --instance-termination-action=STOP \
          --maintenance-policy=TERMINATE \
          $extra \
          --image-family="$IMAGE_FAMILY" --image-project=deeplearning-platform-release \
          --boot-disk-size=150GB 2>/tmp/qm-create-err; then
        echo "✓ created Spot $name in $z"; return 0
      fi
      if grep -qiE "permission|billing|forbidden|not authorized" /tmp/qm-create-err; then
        cat /tmp/qm-create-err >&2; echo "auth/permission error — stopping." >&2; return 2
      fi
      echo "  $name/$z: $(grep -oiE '(quota|capacity|does not (exist|have)|not available|no available|resource[^.]*)' /tmp/qm-create-err | head -1 || echo unavailable)"
    done
  done
  echo "no GPU type (L4/T4/A100/V100/P100) had Spot capacity + quota in any zone right now." >&2
  return 1
}

wait_ssh () {
  echo "waiting for SSH…"
  until "${G[@]}" compute ssh "$VM" --zone "$1" --quiet --command=true >/dev/null 2>&1; do sleep 8; done
  echo "VM ready in $1."
}

# Spot start can fail on transient zone capacity — retry a few times before giving up.
start_with_retry () {
  local z="$1" i
  for i in 1 2 3 4 5 6; do
    if "${G[@]}" compute instances start "$VM" --zone "$z" 2>/tmp/qm-start-err; then return 0; fi
    if grep -qiE "resource|capacity|exhausted|fulfill the request" /tmp/qm-start-err; then
      echo "  Spot capacity unavailable in $z (attempt $i) — retrying in 20s…"; sleep 20; continue
    fi
    cat /tmp/qm-start-err >&2; return 1
  done
  {
    echo ""
    echo "########################################################"
    echo "  ⚠  GPU START FAILED — Spot L4 capacity unavailable"
    echo "########################################################"
    echo "  WHAT: could not start $VM (Spot) in $z after 6 retries."
    echo "  WHY:  the zone has no spare preemptible L4 right now (Spot capacity fluctuates)."
    echo "  FIX:"
    echo "   • wait a few minutes, then retry:  ./scripts/gpu.sh serve vllm"
    echo "   • or move to another zone (loses cached images → re-setup, ~30 min):"
    echo "       ./scripts/gpu.sh delete && QM_GPU_ZONES='us-west1-a us-east1-c us-central1-b' ./scripts/gpu.sh serve vllm"
    echo "   • or switch this VM to on-demand (drop --provisioning-model=SPOT in create_vm)."
    echo "########################################################"
  } >&2
  return 1
}

# Fast start of the EXISTING VM in its zone (2 tries, then bail so `find` can move
# to other zones instead of burning 2 min on a dead zone).
quick_start () {
  local z="$1" i
  for i in 1 2; do
    if "${G[@]}" compute instances start "$VM" --zone "$z" 2>/tmp/qm-start-err; then return 0; fi
    grep -qiE "resource|capacity|exhausted|fulfill the request" /tmp/qm-start-err \
      || { cat /tmp/qm-start-err >&2; return 1; }
    [ "$i" = 1 ] && { echo "  $z: no Spot capacity (one more try in 15s)…"; sleep 15; }
  done
  return 1
}

# Docker + NVIDIA toolkit — idempotent. Fast no-op on a cached VM; installs on a
# fresh-zone disk so `serve` works there without a manual setup step.
ensure_docker () {
  local z="$1"
  if "${G[@]}" compute ssh "$VM" --zone "$z" --quiet \
       --command="command -v docker >/dev/null && sudo docker info >/dev/null 2>&1" >/dev/null 2>&1; then
    echo "Docker + GPU runtime present."; return 0
  fi
  echo "fresh disk — installing Docker + NVIDIA container toolkit…"
  "${G[@]}" compute scp "$REPO_ROOT/scripts/vm-setup.sh" "$VM:~/" --zone "$z" --quiet
  "${G[@]}" compute ssh "$VM" --zone "$z" --quiet --command="chmod +x ~/vm-setup.sh && ~/vm-setup.sh"
}

# "Test all available GPUs, then run": try the existing VM's zone first (cached), and
# if it's out of capacity, probe every GPU type x zone and provision the first available
# one (any of L4/T4/A100/V100/P100) — then set up + serve. Never stuck on one zone or GPU.
find_gpu () {
  local engine="${1:-vllm}" z s
  case "$engine" in vllm) ;; *) echo "usage: ./scripts/gpu.sh find vllm" >&2; return 2 ;; esac
  require_auth
  ensure_network
  z="$(find_zone)"
  if [ -n "$z" ]; then
    s="$("${G[@]}" compute instances describe "$VM" --zone "$z" --format='value(status)' 2>/dev/null)"
    if [ "$s" = RUNNING ]; then
      echo "VM already running in $z."
      wait_ssh "$z"; ensure_docker "$z"; serve "$engine"; return $?
    fi
    echo "existing VM in $z ($s) — trying its (cached) zone first…"
    if quick_start "$z"; then
      wait_ssh "$z"; ensure_docker "$z"; serve "$engine"; return $?
    fi
    echo "  $z has no Spot capacity right now."
    echo "  → recreating on any available GPU/zone. NOTE: new VM = fresh disk (Docker + image re-pull on first serve)."
    "${G[@]}" compute instances delete "$VM" --zone "$z" --quiet
  fi
  echo "probing every GPU type (L4/T4/A100/V100/P100) x zone for Spot capacity…"
  local rc
  create_any; rc=$?
  if [ "$rc" -ne 0 ]; then
    {
      echo ""
      echo "couldn't provision any Spot GPU right now."
      echo "  • retry shortly (Spot capacity fluctuates), or"
      echo "  • widen zones:    QM_GPU_ZONES='<zone> …' ./scripts/gpu.sh find $engine"
      echo "  • widen/limit GPU: QM_GPU_PROFILES='T4|n1-standard-4|--accelerator=type=nvidia-tesla-t4,count=1' ./scripts/gpu.sh find $engine"
    } >&2
    return 1
  fi
  z="$(find_zone)"; wait_ssh "$z"; ensure_docker "$z"; serve "$engine"
}

up () {
  require_auth
  ensure_network
  local z; z="$(find_zone)"
  if [ -n "$z" ]; then
    local s; s="$("${G[@]}" compute instances describe "$VM" --zone "$z" --format='value(status)')"
    [ "$s" = RUNNING ] || { echo "starting VM in $z (was $s)…"; start_with_retry "$z" || exit 1; }
  else
    create_vm || exit 1
    z="$(find_zone)"
  fi
  wait_ssh "$z"
}

# --- engine serve/stop (one GPU → one engine at a time) ---
serve () {
  local engine="${1:-}" z port cmd dtype cc
  z="$(require_zone)"
  # bfloat16 needs Ampere+ (compute capability >= 8.0). L4/A100 qualify; T4/V100/P100
  # do not — vLLM aborts on bf16 there, so fall back to float16. Detect, don't assume.
  cc="$("${G[@]}" compute ssh "$VM" --zone "$z" --quiet --command="nvidia-smi --query-gpu=compute_cap --format=csv,noheader 2>/dev/null | head -1" 2>/dev/null | tr -d '[:space:]')"
  if [ -n "$cc" ] && awk "BEGIN{exit !(${cc:-0}+0 >= 8.0)}" 2>/dev/null; then dtype=bfloat16; else dtype=float16; fi
  echo "GPU compute capability ${cc:-unknown} → --dtype $dtype"
  # Model-source seam: QM_GPU_MODEL = hf-repo | gs://… | s3://… | /local. Adding a store
  # later = one more arm here (matches the UI's ModelSource kinds). The customer points
  # QM_GPU_MODEL at their own bucket; nothing else changes.
  local model_arg="$MODEL" mount_flag="" mount_cmd="" extra=""
  case "$MODEL" in
    gs://*)
      local rest="${MODEL#gs://}" bkt sub
      bkt="${rest%%/*}"; sub="${rest#"$bkt"}"; sub="${sub#/}"
      mount_cmd="command -v gcsfuse >/dev/null || { echo 'gcsfuse missing on the VM'; exit 3; }; sudo mkdir -p /mnt/qm-model; mountpoint -q /mnt/qm-model || sudo gcsfuse --implicit-dirs $bkt /mnt/qm-model; "
      model_arg="/model/$sub"; mount_flag="-v /mnt/qm-model:/model:ro"
      echo "model source: GCS $MODEL → gcsfuse /mnt/qm-model → --model $model_arg" ;;
    s3://*)
      extra="--load-format runai_streamer"
      echo "model source: S3 $MODEL (vLLM Run:ai streamer)" ;;
    *) : ;; # hf repo or /local path → passthrough
  esac
  case "$engine" in
    vllm)
      port=8000
      cmd="${mount_cmd}sudo docker rm -f vllm >/dev/null 2>&1 || true; sudo docker run --gpus all -d --name vllm -p 8000:8000 $mount_flag vllm/vllm-openai:latest --model $model_arg --dtype $dtype $extra --max-model-len 8192 --enable-auto-tool-choice --tool-call-parser hermes >/dev/null" ;;
    *) echo "usage: ./scripts/gpu.sh serve vllm" >&2; return 2 ;;
  esac
  echo "serving $engine ($MODEL) on :$port … (first serve on a fresh zone pulls the image — up to ~25 min)"
  "${G[@]}" compute ssh "$VM" --zone "$z" --quiet --command="$cmd; for _ in \$(seq 1 300); do curl -sf localhost:$port/v1/models >/dev/null 2>&1 && break; sleep 5; done; if curl -sf localhost:$port/v1/models >/dev/null 2>&1; then echo \"$engine READY on :$port\"; else echo \"$engine FAILED\"; sudo docker logs $engine 2>&1 | tail -20; fi"
}

unserve () {
  local z names; z="$(require_zone)"
  case "${1:-all}" in
    vllm|all) names=vllm ;;
    *) echo "usage: ./scripts/gpu.sh unserve [vllm|all]" >&2; return 2 ;;
  esac
  "${G[@]}" compute ssh "$VM" --zone "$z" --quiet --command="for n in $names; do sudo docker rm -f \$n >/dev/null 2>&1 && echo stopped \$n || true; done"
}

tunnel () {
  local engine="${1:-vllm}" z port
  z="$(require_zone)"
  case "$engine" in vllm) port=8000 ;; *) echo "usage: ./scripts/gpu.sh tunnel vllm" >&2; return 2 ;; esac
  echo "forwarding localhost:$port -> $VM:$port (auto-reconnect; Ctrl-C to stop)…"
  # keepalive (ServerAlive*) so a dead link is detected fast instead of lingering as a stale
  # process serving a broken tunnel; ExitOnForwardFailure fails loud if the port is taken;
  # the loop reconnects on any drop (Spot blips, idle SSH timeouts) so localhost:$port stays up.
  while true; do
    "${G[@]}" compute ssh "$VM" --zone "$z" --quiet -- \
      -o ServerAliveInterval=15 -o ServerAliveCountMax=3 -o ExitOnForwardFailure=yes \
      -L "$port:localhost:$port" -N || true
    echo "tunnel dropped — reconnecting in 3s…" >&2
    sleep 3
  done
}

# GPU memory (used, total in MiB) via nvidia-smi on the host — the real VRAM signal the
# UI shows when engine control is configured. Prints "used, total" or nothing if unreachable.
vram () {
  local z; z="$(require_zone)"
  "${G[@]}" compute ssh "$VM" --zone "$z" --quiet \
    --command="nvidia-smi --query-gpu=memory.used,memory.total --format=csv,noheader,nounits 2>/dev/null | head -1"
}

capture () {
  local z; z="$(require_zone)"
  echo "uploading capture scripts…"
  "${G[@]}" compute scp "$REPO_ROOT/scripts/capture-fixtures.sh" "$REPO_ROOT/scripts/vm-setup.sh" \
    "$REPO_ROOT/scripts/vm-capture.sh" "$VM:~/" --zone "$z" --quiet
  echo "launching capture on VM (background)…"
  "${G[@]}" compute ssh "$VM" --zone "$z" --quiet --command=\
"chmod +x ~/capture-fixtures.sh ~/vm-setup.sh ~/vm-capture.sh; rm -f ~/capture-done; nohup ~/vm-capture.sh >/dev/null 2>&1 & echo launched"
  echo "waiting for capture (first-run Docker pulls can take 20-40 min)…"
  until "${G[@]}" compute ssh "$VM" --zone "$z" --quiet --command="test -f ~/capture-done" >/dev/null 2>&1; do sleep 20; done
  echo "pulling fixtures into the repo…"
  "${G[@]}" compute scp --recurse "$VM:~/fixtures/vllm" \
    "$REPO_ROOT/crates/qm-backends/tests/fixtures/" --zone "$z" --quiet || true
  echo "fixtures in crates/qm-backends/tests/fixtures/ — remember: ./scripts/gpu.sh down"

  # Error surfacing (mandate: never swallow). If any engine failed to start,
  # highlight what/why/how + remediation and fail loudly.
  if "${G[@]}" compute ssh "$VM" --zone "$z" --quiet --command="test -f ~/capture-failed" >/dev/null 2>&1; then
    echo ""
    echo "########################################################"
    echo "  ⚠  GPU RUN ERRORS — one or more engines failed to start"
    echo "########################################################"
    "${G[@]}" compute ssh "$VM" --zone "$z" --quiet --command="cat ~/capture-failures.txt" 2>/dev/null
    echo "--------------------------------------------------------"
    echo "  REMEDIATION: read each WHY above. Common fixes:"
    echo "   • GPU OOM      → smaller model or lower --max-model-len"
    echo "   • disk full    → bigger --boot-disk-size / free space"
    echo "   • image pull   → fix the tag / retry (registry rate-limit)"
    echo "   • gated weights→ set HF_TOKEN in vm-capture.sh"
    echo "  VM is still up: './scripts/gpu.sh ssh' to inspect,"
    echo "  './scripts/gpu.sh resume' to retry after a fix."
    echo "########################################################"
    return 1
  fi
  return 0
}

case "${1:-}" in
  run)     up; if capture; then echo "=== cargo test --workspace ==="; ( cd "$REPO_ROOT" && cargo test --workspace ); else echo "gpu.sh: capture had errors — skipping cargo test."; exit 1; fi ;;
  find)    find_gpu "${2:-vllm}" ;;
  up)      up ;;
  capture) up; capture ;;
  ssh)     "${G[@]}" compute ssh "$VM" --zone "$(require_zone)" ;;
  resume)  up; "${G[@]}" compute ssh "$VM" --zone "$(require_zone)" ;;
  serve)   up; serve "${2:-}" ;;
  unserve) unserve "${2:-all}" ;;
  vram)    vram ;;
  tunnel)  tunnel "${2:-vllm}" ;;
  down)    "${G[@]}" compute instances stop "$VM" --zone "$(require_zone)"; echo "stopped (GPU billing off; disk kept)." ;;
  delete)  "${G[@]}" compute instances delete "$VM" --zone "$(require_zone)" --quiet; echo "deleted." ;;
  status)  z="$(find_zone)"; [ -n "$z" ] && echo "$VM: $("${G[@]}" compute instances describe "$VM" --zone "$z" --format='value(status)') in $z" || echo "absent" ;;
  *) grep '^#' "$0" | sed 's/^#\{0,1\} \{0,1\}//'; exit 2 ;;
esac
