import { useEffect, useState } from "react";
import type { Update } from "@tauri-apps/plugin-updater";
import { checkForUpdate, downloadAndInstall } from "../../../shared/ipc/system/updater";
import { getUserSettings, setUserSettings } from "../../../shared/ipc/settings/userSettings";
import { shouldCheck } from "../updateSchedule";
import { Markdown } from "../../../shared/markdown";

/// On launch, runs a background update check at most once per 24h. If a
/// newer version is found, shows a banner with explicit consent — never
/// auto-installs. "Remind me later" defers (the 24h stamp is already set).
export function StartupUpdate() {
  const [update, setUpdate] = useState<Update | null>(null);
  const [installing, setInstalling] = useState(false);

  useEffect(() => {
    void (async () => {
      try {
        const s = await getUserSettings();
        if (!shouldCheck(s.last_update_check_at, Date.now())) return;
        const found = await checkForUpdate();
        await setUserSettings({ ...s, last_update_check_at: new Date().toISOString() });
        if (found) setUpdate(found);
      } catch (e) {
        console.error("startup update check failed:", e);
      }
    })();
  }, []);

  if (!update) return null;
  const install = async () => {
    setInstalling(true);
    try { await downloadAndInstall(update); }
    catch (e) { console.error("update install failed:", e); setInstalling(false); }
  };

  return (
    <div
      data-testid="update-banner"
      className="fixed bottom-4 left-1/2 -translate-x-1/2 z-50 w-[28rem] max-w-[90vw] bg-surface border rounded-lg shadow-lg p-3"
    >
      <div className="text-sm font-semibold mb-1">QuantaMind v{update.version} is available</div>
      {update.body && (
        <div className="max-h-40 overflow-auto border rounded p-2 mb-2 bg-gray-50">
          <Markdown text={update.body} />
        </div>
      )}
      <div className="flex gap-2 justify-end">
        <button
          type="button"
          onClick={() => setUpdate(null)}
          disabled={installing}
          className="text-xs px-3 py-1 border rounded hover:bg-gray-50 disabled:opacity-60"
          data-testid="update-later"
        >
          Remind me later
        </button>
        <button
          type="button"
          onClick={() => void install()}
          disabled={installing}
          className="text-xs px-3 py-1 bg-blue-600 text-white rounded hover:bg-blue-700 disabled:opacity-60"
          data-testid="update-now"
        >
          {installing ? "Installing…" : "Install now"}
        </button>
      </div>
    </div>
  );
}
