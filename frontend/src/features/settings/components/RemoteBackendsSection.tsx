import { useEffect, useState } from "react";
import {
  getUserSettings,
  setUserSettings,
  type UserSettings,
} from "../../../shared/ipc/settings/userSettings";
import { rawMessage } from "../../../shared/ipc/core/error";
import { useInstalledModelsStore } from "../../models/state/installedModelsStore";
import { useRemoteEndpointsStore } from "../../workspace/state/remoteEndpointsStore";

type SaveState = "idle" | "saving" | "saved" | "error";

/// True when a key is set on a URL that isn't https or loopback — the backend WITHHOLDS the
/// key over plain http (rule 7d, never transmits a credential unencrypted), so we warn the
/// user their key won't be used. Mirrors the backend `credential_allowed` guard. Fail-closed
/// on an unparseable URL (flag it).
function keyWithheldInsecure(url: string | null | undefined, key: string | null | undefined): boolean {
  if (!key) return false;
  try {
    const u = new URL((url ?? "").trim());
    if (u.protocol === "https:") return false;
    const h = u.hostname.replace(/^\[|\]$/g, "");
    return !(h === "localhost" || h === "::1" || /^127\./.test(h));
  } catch {
    return true;
  }
}

/// Honest security note: we DON'T leak the key over http (we withhold it) — but that means it
/// won't authenticate either. Pre-empts the "you're sending keys in cleartext" reaction.
function InsecureKeyNote() {
  return (
    <p data-testid="insecure-key-note" className="text-xs text-amber-600">
      Your API key won't be sent over plain http — credentials are never transmitted over an
      unencrypted connection. Use an <code>https://</code> URL (or a loopback address for local
      testing) so the key is actually used.
    </p>
  );
}

/// Configure the remote vLLM endpoint. This backend runs on a remote
/// GPU (not app-managed), so the app just points its HTTP client at the URL you
/// enter here; the optional API key is sent as `Authorization: Bearer` (set it
/// when the server was launched with `--api-key`). Saved to user_settings.yaml
/// and mirrored into the backend's endpoint resolver on save.
export function RemoteBackendsSection() {
  const [settings, setSettings] = useState<UserSettings | null>(null);
  const [save, setSave] = useState<SaveState>("idle");
  const [saveError, setSaveError] = useState<string | null>(null);

  useEffect(() => {
    getUserSettings()
      .then(setSettings)
      .catch((e) => console.error("settings load failed:", e));
  }, []);

  const update = (patch: Partial<UserSettings>) =>
    setSettings((s) => (s ? { ...s, ...patch } : s));

  const persist = async () => {
    if (!settings) return;
    setSave("saving");
    setSaveError(null);
    try {
      await setUserSettings(settings);
      // Mirror the new endpoints into the reactive store so the health pollers start/stop
      // immediately (a just-configured endpoint begins polling; a cleared one stops).
      useRemoteEndpointsStore.getState().setUrls({ vllmUrl: settings.vllm_url });
      // The saved URL/key just changed which remote models are reachable — refetch
      // so the header picker reflects the new endpoint (health-edge refresh only
      // fires when reachability flips; a same-state URL change wouldn't trigger it).
      await useInstalledModelsStore.getState().refresh();
      setSave("saved");
    } catch (e) {
      console.error("settings save failed:", e);
      // Surface the backend's actionable reason (e.g. the https-only credential
      // guardrail) instead of a bare "Save failed".
      setSaveError(rawMessage(e));
      setSave("error");
    }
  };

  if (!settings) {
    return (
      <p className="text-sm text-gray-500" data-testid="remote-backends-loading">
        Loading…
      </p>
    );
  }

  const field = (
    label: string,
    testid: string,
    value: string | null | undefined,
    onChange: (v: string) => void,
    opts?: { password?: boolean; placeholder?: string },
  ) => (
    <label className="block text-sm">
      <span className="text-gray-500">{label}</span>
      <input
        data-testid={testid}
        type={opts?.password ? "password" : "text"}
        value={value ?? ""}
        placeholder={opts?.placeholder}
        onChange={(e) => {
          onChange(e.target.value);
          setSave("idle");
        }}
        className="mt-0.5 w-full rounded border px-2 py-1 font-mono text-xs bg-surface"
      />
    </label>
  );

  return (
    <div className="max-w-xl space-y-3" data-testid="remote-backends-section">
      <div>
        <h2 className="text-xs font-semibold uppercase tracking-wide text-gray-400 mb-1">
          Remote GPU backends
        </h2>
        <p className="text-xs text-gray-500">
          vLLM runs on a remote GPU. Enter the server's base URL (and API key if it
          was started with <code>--api-key</code>).
        </p>
      </div>

      <div className="space-y-2">
        <h3 className="text-xs font-semibold text-ink">vLLM</h3>
        {field("Server URL", "vllm-url", settings.vllm_url, (v) => update({ vllm_url: v }), {
          placeholder: "http://34.10.20.30:8000",
        })}
        {field("API key", "vllm-api-key", settings.vllm_api_key, (v) => update({ vllm_api_key: v }), {
          password: true,
          placeholder: "optional",
        })}
        {keyWithheldInsecure(settings.vllm_url, settings.vllm_api_key) && <InsecureKeyNote />}
      </div>

      <div className="flex items-center gap-3">
        <button
          type="button"
          data-testid="remote-backends-save"
          onClick={() => void persist()}
          disabled={save === "saving"}
          className="rounded border px-3 py-1 text-sm hover:bg-surface disabled:opacity-50"
        >
          {save === "saving" ? "Saving…" : "Save"}
        </button>
        {save === "saved" && <span className="text-xs text-green-600">Saved</span>}
        {save === "error" && <span className="text-xs text-red-600">Save failed</span>}
      </div>
      {save === "error" && saveError && (
        <p data-testid="remote-backends-error" className="text-xs text-red-600">
          {saveError}
        </p>
      )}
    </div>
  );
}
