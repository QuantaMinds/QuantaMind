import { useState } from "react";
import { open as openExternal } from "@tauri-apps/plugin-shell";
import { useNavStore } from "../../../shared/state/navStore";
import { useMlxBackend } from "../hooks/useMlxBackend";
import { useHostOs } from "../../../shared/os/useHostOs";
import type { HostOs } from "../../../shared/ipc/system/os_platform";

type Cmd = { label: string; cmd: string };
type Link = { text: string; href: string };
type Engine = {
  id: string;
  name: string;
  tag: string;
  blurb: string;
  runs: string;
  commands: Cmd[];
  links: Link[];
  steps: string[];
  appleOnly?: boolean;
};

/// Per-OS install command for a given engine. Returns null if the engine
/// isn't natively packaged on that OS (caller falls back to the download
/// link). macOS uses Homebrew; Windows uses winget; Linux is package-manager
/// varied so we point at the official Ollama install script and, for
/// whisper/llama.cpp, direct release downloads.
function installCmdFor(engine: "ollama" | "llama_cpp" | "whisper", os: HostOs | null): Cmd {
  switch (engine) {
    case "ollama":
      switch (os) {
        case "windows":
          return { label: "Install (Windows)", cmd: "winget install Ollama.Ollama" };
        case "linux":
          return {
            label: "Install (Linux)",
            cmd: "curl -fsSL https://ollama.com/install.sh | sh",
          };
        case "mac":
        default:
          return { label: "Install (macOS)", cmd: "brew install ollama" };
      }
    case "llama_cpp":
      switch (os) {
        case "windows":
          return {
            label: "Bundled sidecar (Windows)",
            cmd: "QuantaMind ships llama-server.exe — press ▶ in the header",
          };
        case "linux":
          return {
            label: "Bundled sidecar (Linux)",
            cmd: "QuantaMind ships llama-server — press ▶ in the header",
          };
        case "mac":
        default:
          return { label: "Install (macOS)", cmd: "brew install llama.cpp" };
      }
    case "whisper":
      switch (os) {
        case "windows":
          return {
            label: "Install (Windows)",
            cmd: "winget install ggerganov.whisper.cpp",
          };
        case "linux":
          return {
            label: "Install (Linux — direct download)",
            cmd: "Download whisper-server from github.com/ggerganov/whisper.cpp/releases",
          };
        case "mac":
        default:
          return { label: "Install (macOS)", cmd: "brew install whisper-cpp" };
      }
  }
}

function enginesFor(os: HostOs | null): Engine[] {
  return [
    {
      id: "ollama",
      name: "Ollama",
      tag: "Easiest",
      blurb: "Manages and runs models for you — the best place to start.",
      runs: "Llama 3.2, Qwen 2.5, Phi, Mistral, Gemma… (GGUF)",
      commands: [
        installCmdFor("ollama", os),
        { label: "Then pull a model", cmd: "ollama pull llama3.2:1b" },
      ],
      links: [{ text: "Download for any OS", href: "https://ollama.com/download" }],
      steps: [
        "Install Ollama (command above, or the download link).",
        "Press ▶ in the header to start it — QuantaMind runs it for you.",
        "Pull a model with the command above, or from the Models tab.",
      ],
    },
    {
      id: "llama_cpp",
      name: "llama.cpp",
      tag: "Local GGUF",
      blurb: "Runs a single GGUF directly via llama-server.",
      runs: "Any .gguf file",
      commands: [
        installCmdFor("llama_cpp", os),
        {
          label: "Run your own server (only if not bundled — flags are required)",
          cmd: "llama-server -m your-model.gguf --host 127.0.0.1 --port 8081 --jinja -c 8192",
        },
      ],
      links: [{ text: "llama.cpp project", href: "https://github.com/ggml-org/llama.cpp" }],
      steps: [
        "Easiest: a llama-server ships bundled — just press ▶ and QuantaMind runs it for you (it adds --jinja and the right port automatically).",
        "Download a GGUF in Models → Hugging Face (or Local File).",
        "Pick llama.cpp + your model in the header, then press ▶.",
        "Running your own server instead (e.g. it isn't bundled for your platform)? Use the command above exactly — QuantaMind talks to it on port 8081, and the --jinja flag is required or generations loop instead of stopping.",
      ],
    },
    {
      id: "mlx",
      name: "MLX",
      tag: "Apple Silicon",
      blurb: "Apple-Silicon-native inference via mlx_lm.server.",
      runs: "mlx-community models (4-bit / 8-bit)",
      appleOnly: true,
      commands: [
        { label: "Create a virtual env", cmd: "python3 -m venv ~/mlx-env" },
        { label: "Activate it", cmd: "source ~/mlx-env/bin/activate" },
        { label: "Install mlx-lm", cmd: "pip install -U mlx-lm" },
      ],
      links: [{ text: "mlx-lm project", href: "https://github.com/ml-explore/mlx-lm" }],
      steps: [
        "Run the three commands above to install mlx-lm in ~/mlx-env.",
        "Download an MLX model in Models → Hugging Face.",
        "Pick MLX + your model in the header, then press ▶.",
      ],
    },
    {
      id: "whisper",
      name: "whisper.cpp",
      tag: "Speech-to-Text",
      blurb: "Local speech-to-text — its own engine, runs alongside an LLM.",
      runs: "Whisper tiny / base / small / medium / large-v3",
      commands: [installCmdFor("whisper", os)],
      links: [
        { text: "whisper.cpp project", href: "https://github.com/ggml-org/whisper.cpp" },
      ],
      steps: [
        "Install whisper.cpp with the command above.",
        "Download a model in Models → Speech-to-Text.",
        "Pick a model in the STT header group, then press ▶.",
      ],
    },
  ];
}

function CommandRow({ command, testid }: { command: Cmd; testid: string }) {
  const [copied, setCopied] = useState(false);
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(command.cmd);
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    } catch {
      /* clipboard unavailable — the command is shown inline */
    }
  };
  return (
    <div className="flex flex-col gap-0.5">
      <span className="text-[10px] uppercase tracking-wide text-gray-400">{command.label}</span>
      <div className="flex items-center gap-2">
        <code className="flex-1 bg-gray-50 border rounded px-2 py-1 text-xs break-all">{command.cmd}</code>
        <button
          type="button"
          onClick={copy}
          className="text-[11px] border rounded px-1.5 py-0.5 shrink-0"
          data-testid={testid}
        >
          {copied ? "Copied" : "Copy"}
        </button>
      </div>
    </div>
  );
}

function EngineCard({ engine }: { engine: Engine }) {
  return (
    <div data-testid={`setup-engine-${engine.id}`} className="border rounded-lg p-4 flex flex-col gap-2 bg-surface">
      <div className="flex items-center gap-2">
        <span className="font-semibold">{engine.name}</span>
        <span className="text-[10px] uppercase tracking-wide text-gray-500 border rounded px-1.5 py-0.5">{engine.tag}</span>
      </div>
      <p className="text-xs text-gray-600">{engine.blurb}</p>
      <p className="text-xs text-gray-500">
        <span className="text-gray-400">Runs:</span> {engine.runs}
      </p>
      {engine.commands.map((c, i) => (
        <CommandRow key={c.cmd} command={c} testid={`setup-copy-${engine.id}-${i}`} />
      ))}
      <ol className="text-xs text-gray-700 list-decimal pl-4 flex flex-col gap-0.5 mt-1">
        {engine.steps.map((s) => (
          <li key={s}>{s}</li>
        ))}
      </ol>
      <div className="flex flex-wrap gap-2 mt-1">
        {engine.links.map((l) => (
          <button
            key={l.href}
            type="button"
            onClick={() => void openExternal(l.href)}
            className="text-xs text-blue-700 hover:underline"
          >
            {l.text} ↗
          </button>
        ))}
      </div>
    </div>
  );
}

/// Shown in the workspace when no LLM backend is running: a step-by-step guide to
/// install/start each engine (Ollama, llama.cpp, MLX, whisper.cpp) with copy-able
/// install commands, links, and what each runs. The moment a server comes up, the
/// workspace switches to the prompt UI (the StatusBar / header health poll drives that).
export function BackendSetupGuide() {
  const goToModels = useNavStore((s) => s.setTopView);
  const { appleSilicon } = useMlxBackend();
  const os = useHostOs();
  const engines = enginesFor(os).filter((e) => !e.appleOnly || appleSilicon);

  return (
    <div data-testid="backend-setup-guide" className="flex flex-col gap-4 px-2 py-4">
      <div>
        <h2 className="text-lg font-semibold">Connect a backend to start running models</h2>
        <p className="text-sm text-gray-600">
          QuantaMind runs models through a local server. Install one below (copy the command),
          then start it — this page switches to the prompt editor the moment a server is running.
        </p>
      </div>
      <div className="grid grid-cols-1 md:grid-cols-2 gap-3">
        {engines.map((e) => (
          <EngineCard key={e.id} engine={e} />
        ))}
      </div>
      <button
        type="button"
        onClick={() => goToModels("models")}
        data-testid="setup-open-models"
        className="self-start text-sm border rounded px-3 py-1 hover:bg-gray-50"
      >
        Open the Models tab to download models →
      </button>
    </div>
  );
}
