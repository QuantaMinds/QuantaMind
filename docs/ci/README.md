# CI integration — the `qm-eval` GitHub Action

Gate your pipeline on model readiness: the action wraps [`qm run`](../cli/README.md#run--the-readiness-verdict),
produces a JUnit report + a JSON report, uploads both as an artifact, and lets `qm`'s exit code
(subject to `fail-on`) pass or fail the job.

The action lives at `.github/actions/qm-eval`; a runnable example is `.github/workflows/eval-example.yml`.

## Usage

```yaml
- uses: actions/checkout@v4            # the action builds qm from the QuantaMind source
- uses: QuantaMinds/QuantaMind/.github/actions/qm-eval@main
  with:
    backend: vllm
    base-url: ${{ secrets.VLLM_URL }}  # OpenAI-compatible endpoint; sets QM_BASE
    model: qwen3-32b
    collection: easy-coding
    fail-on: notready                  # your team's policy, not ours
    ci-profile: fast
  env:
    QM_API_KEY: ${{ secrets.QM_API_KEY }}   # NEVER an input — inputs are logged
```

GitHub's hosted runners can't host a local model, so the action targets a **remote** vLLM/SGLang
endpoint reachable from CI. `base-url` is an input; the **API key is passed as the `QM_API_KEY` env
from a secret**, never as an action input (inputs appear in logs). `qm` transmits a key only over
`https`/loopback.

## Inputs (the ones you'll usually set)

| Input | Meaning | Default |
|---|---|---|
| `backend` | `ollama` / `llama_cpp` / `mlx` / `vllm` / `sglang` | `vllm` |
| `base-url` | endpoint URL (→ `QM_BASE`) | **required** |
| `model` | model to evaluate | **required** |
| `collection` | built-in collection id | `easy-coding` |
| `mode` | `prompt_based` / `native` / `both` | `prompt_based` |
| `tier` / `thinking` / `k` | difficulty / reasoning preset / pass^k override | collection defaults |
| `profile` | readiness profile (`general-agent` / `rag-assistant` / `coding-agent`) | `general-agent` |
| `fail-on` | which verdict fails the job (`conditional` / `notready` / `never`) | `conditional` |
| `ci-profile` | `fast` (k=1, PR gate) or `full` (tier-default k, nightly) | `fast` |

## Two profiles

- **`fast`** — `k=1`, quick; wire it to the **PR gate** (`on: pull_request`) so a regression is caught
  before merge without a long sweep.
- **`full`** — the tier's default pass^k; wire it to a **nightly `schedule`** for the stricter sweep.

## The gate (exit codes)

`qm` exits with the documented contract; the action's step fails (and so the job) on a non-zero exit,
which `fail-on` controls:

| `fail-on` | Ready (0) | Conditional (10) | NotReady (20) | Inconclusive (11) |
|---|---|---|---|---|
| `conditional` (default) | pass | **fail** | **fail** | **fail** |
| `notready` | pass | pass | **fail** | **fail** |
| `never` | pass | pass | pass | pass |

`2` (bad args / capability mismatch) and `3` (unreachable / bad credential / model not served) always
fail — they're setup errors, not verdicts. `11` (inconclusive: the run couldn't measure anything) fails
under any policy except `never`; it means **retry**, not "the model is bad".

## Secrets — vault / OIDC

Inject the endpoint + key at run time; don't store a long-lived key as a plaintext repo variable.

- **HashiCorp Vault:** `hashicorp/vault-action` → export `QM_BASE` / `QM_API_KEY` into the job env.
- **Cloud OIDC:** the runner federates to your cloud, fetches a short-lived key from the secrets
  manager, exports it as `QM_API_KEY`. No static secret in GitHub at all.

## The test panel

The action writes `qm-junit.xml` and uploads it (with `qm-report.json`) as an artifact. To render it in
GitHub's checks UI, add a JUnit reporter step (the example uses `mikepenz/action-junit-report`) — a Ready
run shows green; a NotReady run shows red with the failing tier and its failure taxonomy named.

## Note on build cost

`qm` currently links the full QuantaMind library (Tauri deps), so the action builds those too — the
first run is slow; `Swatinem/rust-cache` makes repeats fast. A leaner CLI-only build is a future
improvement (a prebuilt `qm` binary distribution would remove the build step entirely).
