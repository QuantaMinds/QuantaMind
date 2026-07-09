import { useUpdater } from "../hooks/useUpdater";
import { Markdown } from "../../../shared/markdown";

function pct(downloaded: number, total: number | null): string {
  if (!total || total <= 0) return "downloading…";
  return `${Math.min(100, Math.round((downloaded / total) * 100))}%`;
}

export function UpdateChecker() {
  const { status, currentVersion, update, downloaded, total, error, check, install } = useUpdater();
  const checking = status === "checking";
  const busy = status === "downloading" || status === "installing";

  return (
    <section
      data-testid="update-checker"
      className="border rounded p-3 bg-surface flex flex-col gap-2"
    >
      <div className="flex items-center justify-between gap-2">
        <div>
          <h3 className="text-sm font-semibold">QuantaMind updates</h3>
          <p className="text-xs text-gray-600" data-testid="update-current-version">
            You're on v{currentVersion ?? "…"}.
          </p>
        </div>
        <button
          type="button"
          onClick={() => void check()}
          disabled={checking || busy}
          data-testid="update-check-button"
          className="border rounded px-3 py-1 text-xs hover:bg-gray-50 disabled:opacity-60"
        >
          {checking ? "Checking…" : "Check for updates"}
        </button>
      </div>

      {status === "up_to_date" && (
        <p className="text-xs text-green-700" data-testid="update-up-to-date">
          You're on the latest version.
        </p>
      )}

      {status === "available" && update && (
        <div className="border rounded p-2 bg-blue-50 flex flex-col gap-2" data-testid="update-available">
          <div className="text-xs">
            <strong>v{update.version}</strong> is available
            {update.date && <span className="text-gray-600"> · {update.date}</span>}.
          </div>
          {update.body && (
            <div className="bg-surface border rounded p-2 max-h-48 overflow-auto">
              <Markdown text={update.body} />
            </div>
          )}
          <button
            type="button"
            onClick={() => void install()}
            className="self-start text-xs bg-blue-600 text-white rounded px-3 py-1 hover:bg-blue-700"
            data-testid="update-install-button"
          >
            Download and install
          </button>
        </div>
      )}

      {status === "downloading" && (
        <p className="text-xs text-gray-700" data-testid="update-downloading">
          Downloading… {pct(downloaded, total)}
        </p>
      )}

      {status === "installing" && (
        <p className="text-xs text-gray-700" data-testid="update-installing">
          Installing — the app will relaunch when it's ready.
        </p>
      )}

      {status === "error" && (
        <p role="alert" className="text-xs text-red-600" data-testid="update-error">
          Couldn't complete the update: {error}
        </p>
      )}
    </section>
  );
}
