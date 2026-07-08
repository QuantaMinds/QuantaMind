# security-testing.md — QuantaMind

Manual + automated test scenarios for the security controls described in
[`security.md`](security.md). Grouped by what needs a human running the app (live), adversarial
"prove the guard blocks the bad thing" cases, the automated suite, and CI.

Paths below are macOS (`~/Library/Application Support/dev.quantamind.app/` is the app config
dir; weights live under `~/.quantamind/`). On Linux the config dir is
`~/.config/dev.quantamind.app/`; on Windows it's `%APPDATA%\dev.quantamind.app\`.

---

## A. Must verify live (can't be unit-tested)

### A1 — API keys never hit disk in plaintext (rule 7a)
1. Settings → Remote Backends → vLLM URL `https://box:8000`, API key `sk-CANARY-123`, **Save**.
2. Confirm it is **not** in the YAML:
   `grep -r "sk-CANARY-123" ~/Library/Application\ Support/dev.quantamind.app/` → **no matches.**
3. Confirm it **is** in the keychain:
   `security find-generic-password -s quantamind -a vllm-api-key -w` → prints `sk-CANARY-123`.
4. **Migration:** quit → hand-add `vllm_api_key: sk-OLD-999` to `user_settings.yaml` → relaunch →
   re-grep. **Expect:** `sk-OLD-999` gone from YAML, now in the keychain.

### A2 — https-only credential guard + popup (rule 7d)
Save each in Settings → Remote Backends and observe:

| URL + key | Expect |
|---|---|
| `http://34.10.20.30:8000` + a key | ❌ red popup: "…not HTTPS…use an https:// URL, or clear the key" |
| `http://127.0.0.1:8000` + a key | ✅ saves (loopback is fine) |
| `https://box:8000` + a key | ✅ saves |
| `http://34.10.20.30:8000` + **no** key | ✅ saves (nothing to leak) |

### A3 — CSP + self-hosted Monaco: works offline, no remote calls (rule 7c)
1. **Turn off Wi-Fi.** Launch, open a prompt editor. **Expect:** Monaco renders + edits normally
   (previously it fetched from a CDN → blank offline).
2. Wi-Fi on → devtools → **Network** → reload → open the editor. **Expect:** zero requests to
   `cdn.jsdelivr.net` or `fonts.googleapis.com`.
3. devtools → **Console.** **Expect:** no red `Content-Security-Policy` violations. *(If any appear,
   capture them — that's what's needed to tighten the policy.)*

### A4 — no machine identity in the publish payload / logs (rule 7f)
1. Publish dialog → the "what's shared" preview. **Expect:** no `/Users/<you>`, no macOS username,
   no hostname.
2. Trigger the orphan-reap log (relaunch while a sidecar runs) → check stderr. **Expect:** the
   killed-server line shows `~/.quantamind/...`, **not** `/Users/<you>/...`.

---

## B. Adversarial scenarios (prove the guard blocks the bad thing)

### B1 — symlink path traversal is rejected (rule 7b)
```
cd <your workspace folder>
ln -s /etc/passwd evil.quantamind.yaml
```
Open it in the app's workspace. **Expect:** rejected ("path escapes workspace"); `/etc/passwd`
not shown. Clean up: `rm evil.quantamind.yaml`.

### B2 — readiness export won't write through a symlink (rule 7b)
```
ln -s ~/.zshrc /tmp/qm-export.png
```
Export a readiness card to `/tmp/qm-export.png`. **Expect:** refused; `~/.zshrc` untouched.
Exporting to `report.txt` is also refused (`.png` only).

### B3 — truncated/corrupted download is caught
Interrupt a model download partway (kill Wi-Fi mid-pull), then resume. **Expect:** either a clean
resume to the verified full hash, or a "download truncated" / "integrity check FAILED" error with
the `.partial` kept — never a short file promoted to "installed."

---

## C. Automated coverage (fast, deterministic)

```
cd backend && cargo test --lib -- \
  secrets remote_guard fs_guard redact publish::row hf_download pkce mlx_locate at_rest audit
```
All green. Key ones and what they prove:

- `published_row_carries_no_machine_identity` / `serialized_row_has_only_allowlisted_fields` —
  the publish payload can't leak identity or gain a field silently.
- `rejects_symlink_final_component_escaping_root` / `rejects_dangling_symlink` — the traversal fix.
- `await_redirect_rejects_state_mismatch` / `await_redirect_rejects_non_loopback_host` — the OAuth
  CSRF + DNS-rebinding guards (not manually reachable without the auth server).
- `api_keys_are_never_written_to_disk` — the disk boundary strips secrets.
- `passthrough_round_trips_unchanged` + the transcript round-trip — the no-op seams changed nothing.

Frontend guard: `cd frontend && pnpm test RemoteBackendsSection` — proves the https-only popup
surfaces the backend's real message.

---

## D. CI (on push)

Pushing the branch runs the **Security** workflow (`.github/workflows/security.yml`). **Expect:**
gitleaks passes (no secrets in history), `pnpm audit --prod --audit-level high` passes, and a
CycloneDX SBOM artifact is uploaded. The full `pnpm audit` / `cargo audit` steps are report-only,
so advisories surface without failing the run.
