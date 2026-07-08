import { useEffect, useState } from "react";
import {
  getUserSettings,
  setUserSettings,
  type UserSettings,
} from "../../../shared/ipc/settings/userSettings";
import { rawMessage } from "../../../shared/ipc/core/error";
import { useInstalledModelsStore } from "../../models/state/installedModelsStore";

type SaveState = "idle" | "saving" | "saved" | "error";

/// Configure the remote vLLM / SGLang endpoints. These backends run on a remote
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
          vLLM and SGLang run on a remote GPU. Enter each server's base URL (and API
          key if it was started with <code>--api-key</code>).
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
      </div>

      <div className="space-y-2">
        <h3 className="text-xs font-semibold text-ink">SGLang</h3>
        {field("Server URL", "sglang-url", settings.sglang_url, (v) => update({ sglang_url: v }), {
          placeholder: "http://34.10.20.30:30000",
        })}
        {field("API key", "sglang-api-key", settings.sglang_api_key, (v) => update({ sglang_api_key: v }), {
          password: true,
          placeholder: "optional",
        })}
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
