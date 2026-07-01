import { useStartOllama } from "../../hooks/useStartOllama";

export function OllamaEmptyState() {
  const { status, error, autoStartSupported, start, openInstallPage } = useStartOllama();
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
                Auto-start isn't supported on this OS yet — start Ollama
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
