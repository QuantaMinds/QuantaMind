import { useStartOllama } from "../../hooks/useStartOllama";
import { useHostOs } from "../../../../shared/os/useHostOs";
import type { HostOs } from "../../../../shared/ipc/system/os_platform";

/// Per-OS install command shown when Ollama isn't found on this machine
/// (Phase 2 runtime contract: `autoStartSupported === false` now means "not on
/// disk", not "wrong OS"). Windows: winget. Linux: the official install
/// script. macOS: Homebrew. Falls to macOS for null/unknown OS so the copy is
/// never blank while the IPC probe resolves.
function installCmdFor(os: HostOs | null): { label: string; cmd: string } {
  switch (os) {
    case "windows":
      return { label: "Install on Windows", cmd: "winget install Ollama.Ollama" };
    case "linux":
      return {
        label: "Install on Linux",
        cmd: "curl -fsSL https://ollama.com/install.sh | sh",
      };
    case "mac":
    default:
      return { label: "Install on macOS", cmd: "brew install ollama" };
  }
}

export function OllamaEmptyState() {
  const { status, error, autoStartSupported, start, openInstallPage } = useStartOllama();
  const os = useHostOs();
  const busy = status === "starting";

  return (
    <div
      role="alert"
      data-testid="ollama-empty-state"
      data-status={status}
      className="border border-amber-300 bg-amber-50 rounded p-3 text-sm flex flex-col gap-2 flex-1"
    >
      {status === "starting" ? (
        <div className="flex items-center gap-2">
          <Spinner />
          <span>Starting Ollama…</span>
        </div>
      ) : status === "success" ? (
        <div className="text-green-700">Ollama started ✓</div>
      ) : status === "not_installed" ? (
        <>
          <div className="font-medium">Ollama is not installed on this machine</div>
          <div className="text-gray-700">
            Install Ollama, then come back here and click Start.
          </div>
          <button
            type="button"
            onClick={() => void openInstallPage()}
            className="self-start border rounded px-3 py-1 bg-surface hover:bg-gray-50"
            data-testid="ollama-install-button"
          >
            Install Ollama
          </button>
        </>
      ) : status === "error" ? (
        <>
          <div className="font-medium text-red-700">Couldn't start Ollama</div>
          <div className="text-gray-700" data-testid="ollama-error-message">{error}</div>
          <button
            type="button"
            onClick={() => void start()}
            className="self-start border rounded px-3 py-1 bg-surface hover:bg-gray-50"
            data-testid="ollama-retry-button"
          >
            Retry
          </button>
        </>
      ) : status === "manual_start_required" ? (
        <>
          <div className="font-medium">Ollama needs to be started manually on this OS</div>
          <div className="text-gray-700">
            Auto-start isn't supported on Windows/Linux yet. Start Ollama
            yourself, then click Check again.
          </div>
          <div className="flex gap-2">
            <button
              type="button"
              onClick={() => void start()}
              disabled={busy}
              className="border rounded px-3 py-1 bg-blue-600 text-white hover:bg-blue-700 disabled:opacity-60"
              data-testid="ollama-check-again-button"
            >
              Check again
            </button>
            <button
              type="button"
              onClick={() => void openInstallPage()}
              className="text-blue-700 hover:underline text-sm"
              data-testid="ollama-install-link"
            >
              Install Ollama
            </button>
          </div>
        </>
      ) : (
        <>
          <div className="font-medium">Ollama is not running</div>
          {autoStartSupported ? (
            <>
              <div className="text-gray-700">
                QuantaMind needs Ollama to run local AI models. Click below to
                start it, or install Ollama first if you haven't.
              </div>
              <div className="flex gap-2">
                <button
                  type="button"
                  onClick={() => void start()}
                  disabled={busy}
                  className="border rounded px-3 py-1 bg-blue-600 text-white hover:bg-blue-700 disabled:opacity-60"
                  data-testid="ollama-start-button"
                >
                  Start Ollama
                </button>
                <button
                  type="button"
                  onClick={() => void openInstallPage()}
                  className="text-blue-700 hover:underline text-sm"
                  data-testid="ollama-install-link"
                >
                  Install Ollama
                </button>
              </div>
            </>
          ) : (
            <>
              <div className="text-gray-700">
                Ollama isn't installed on this machine. Install it with the
                command below, then click Check again.
              </div>
              <PerOsInstallCmd os={os} />
              <div className="flex gap-2">
                <button
                  type="button"
                  onClick={() => void start()}
                  disabled={busy}
                  className="border rounded px-3 py-1 bg-blue-600 text-white hover:bg-blue-700 disabled:opacity-60"
                  data-testid="ollama-check-again-button"
                >
                  Check again
                </button>
                <button
                  type="button"
                  onClick={() => void openInstallPage()}
                  className="text-blue-700 hover:underline text-sm"
                  data-testid="ollama-install-link"
                >
                  Download page
                </button>
              </div>
            </>
          )}
        </>
      )}
    </div>
  );
}

function Spinner() {
  return (
    <span
      aria-hidden
      className="inline-block w-3 h-3 border-2 border-gray-400 border-t-transparent rounded-full animate-spin"
    />
  );
}

/// Copy-able install command block for the "not installed on this machine"
/// branch. Reads `useHostOs()` once above and passes the resolved OS in so a
/// null (in-flight IPC) renders the macOS default rather than blank.
function PerOsInstallCmd({ os }: { os: HostOs | null }) {
  const { label, cmd } = installCmdFor(os);
  return (
    <div className="flex flex-col gap-1" data-testid="ollama-install-cmd">
      <span className="text-[10px] uppercase tracking-wide text-gray-500">{label}</span>
      <div className="flex items-center gap-2">
        <code className="flex-1 bg-white/70 border rounded px-2 py-1 text-xs break-all">
          {cmd}
        </code>
        <button
          type="button"
          onClick={() => {
            void navigator.clipboard.writeText(cmd).catch(() => {});
          }}
          className="text-[11px] border rounded px-1.5 py-0.5 shrink-0"
          data-testid="ollama-install-copy"
        >
          Copy
        </button>
      </div>
    </div>
  );
}
