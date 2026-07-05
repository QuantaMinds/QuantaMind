# scripts/ — GPU fixture-capture ops

One command, GPU/zone-independent. Provisions a Spot L4 on GCP, installs Docker +
the NVIDIA Container Toolkit, serves vLLM and SGLang on `Qwen/Qwen3-0.6B`, captures
the golden fixtures, pulls them into `crates/qm-backends/tests/fixtures/`, and runs
the tests. Nothing depends on a specific instance or GPU: the VM is **stopped (not
deleted)** between uses and its zone is auto-discovered, so a preempted/reallotted
Spot VM is rebuilt by the same command.

## Usage

```bash
./scripts/gpu.sh run             # full pipeline: provision -> setup -> serve -> capture -> cargo test
./scripts/gpu.sh resume          # come back later: start the stopped VM + open a shell (no re-setup)
./scripts/gpu.sh serve vllm      # start vLLM (:8000), wait until serving  (stops the other engine first)
./scripts/gpu.sh serve sglang    # start SGLang (:30000, --enable-metrics), wait until serving
./scripts/gpu.sh tunnel vllm     # forward localhost:8000 -> the engine (Ctrl-C to stop)
./scripts/gpu.sh tunnel sglang   # forward localhost:30000
./scripts/gpu.sh unserve         # stop both engine containers (VM stays up)
./scripts/gpu.sh down            # stop the VM (GPU billing off, boot disk kept)
./scripts/gpu.sh status          # VM status + zone
./scripts/gpu.sh delete          # tear the VM down entirely
```

One L4 serves **one engine at a time** (they'd fight for VRAM), so `serve` stops the
other first. Spot `up`/`resume` **retries on transient zone capacity**. To point the
CLI at a served engine: `serve` it, `tunnel` it in another shell, then
`qm-cli run --engine <e> --base http://127.0.0.1:<port> --model <M>`.

Env overrides: `QM_GPU_PROJECT` (default `quantamind-oss`), `QM_GPU_ZONES`
(space-separated fallback list), `QM_GPU_VM`, `QM_GPU_MACHINE`, `QM_GPU_IMAGE_FAMILY`,
`QM_GPU_MODEL` (default `Qwen/Qwen3-0.6B`).

## Files

| File | Runs on | Does |
|---|---|---|
| `gpu.sh` | your Mac | orchestrates gcloud: provision (zone fallback) / serve / capture / stop |
| `vm-setup.sh` | the VM | idempotent Docker + NVIDIA Container Toolkit install (persists on disk) |
| `vm-capture.sh` | the VM | setup, then serve each engine → wait ready → capture → tear down |
| `capture-fixtures.sh` | the VM | curls a running engine for models/health/chat/metrics |

## Requirements

`gcloud` authenticated (`gcloud auth login`) with Compute access on the project,
and a billing account with L4 quota (`quantamind-oss` already has it). Everything
is captured over `localhost` on the VM, so no ingress beyond SSH is opened.
