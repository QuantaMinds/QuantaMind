import { useHostOs } from "../../../shared/os/useHostOs";
import type { HostOs } from "../../../shared/ipc/system/os_platform";

/// Per-OS install / reinstall guidance line. Falls back to macOS Homebrew when
/// the OS is `unknown` (dev without Tauri IPC).
function installStep(os: HostOs | null, reinstall: boolean): string {
  switch (os) {
    case "windows":
      return reinstall
        ? "Run `winget upgrade --id ggerganov.whisper.cpp`, then click Re-check."
        : "Run `winget install ggerganov.whisper.cpp`, then click Re-check.";
    case "linux":
      return "Download whisper.cpp from https://github.com/ggerganov/whisper.cpp/releases and drop `whisper-server` into ~/.local/bin (or /usr/local/bin), then click Re-check.";
    case "mac":
    default:
      return reinstall
        ? "Run `brew reinstall whisper-cpp`, then click Re-check."
        : "On macOS, run `brew install whisper-cpp`, then click Re-check.";
  }
}

/// Turn a raw STT error into plain-language guidance with concrete next steps —
/// so a failure reads as "here's how to fix it", never "the app is broken".
/// Returns null when we have no specific advice (caller shows the raw message).
/// Mirrors `importGuidance` in models/components/LocalFilePreview.tsx.
///
/// `os` is optional so tests can call with a known host; the React component
/// wraps this with `useHostOs()`.
export function sttGuidance(msg: string, os: HostOs | null = "mac"): { title: string; steps: string[] } | null {
  const m = msg.toLowerCase();
  if (m.includes("library not loaded") || m.includes("dyld") || m.includes("can't run")) {
    return {
      title: "whisper.cpp is installed but can't run",
      steps: [
        "The engine is present but its libraries are missing or mismatched.",
        installStep(os, true),
      ],
    };
  }
  if (m.includes("isn't installed") || m.includes("not installed") || m.includes("not bundled")) {
    return {
      title: "whisper.cpp isn't installed",
      steps: [
        "Speech-to-text needs the whisper.cpp engine.",
        installStep(os, false),
      ],
    };
  }
  if (m.includes("vad") && m.includes("missing")) {
    return {
      title: "The VAD model is missing",
      steps: [
        "The silero VAD ships together with each speech model.",
        "Re-download the model from the list above — speech-to-text stays off without the VAD.",
      ],
    };
  }
  if ((m.includes("model") && m.includes("missing")) || m.includes("download it first")) {
    return {
      title: "The speech model isn't downloaded",
      steps: ["Download a model from the list above first, then start the server."],
    };
  }
  if (m.includes("port") && (m.includes("8093") || m.includes("already") || m.includes("in use"))) {
    return {
      title: "Port 8093 is already in use",
      steps: [
        "Another process holds the speech-to-text port.",
        "Stop it and retry — QuantaMind won't take over a process it didn't start.",
      ],
    };
  }
  if (m.includes("can't reach") || m.includes("isn't responding")) {
    return {
      title: "The speech-to-text server isn't responding",
      steps: [
        "It runs locally only and never reaches the cloud.",
        "Start it again from the panel above.",
      ],
    };
  }
  if (m.includes("too small") || m.includes("incomplete") || m.includes("truncated") || m.includes("isn't a") || m.includes("not a ggml")) {
    return {
      title: "The model download is incomplete or invalid",
      steps: [
        "The download was cut off or isn't a real whisper model.",
        "Re-download it — a real model is tens of MB to ~3 GB.",
      ],
    };
  }
  if (m.includes("couldn't start") || m.includes("didn't report a loaded model")) {
    return {
      title: "whisper-server couldn't start",
      steps: [
        "Check that you have free RAM and disk space.",
        "If the model may be corrupt, re-download it.",
        "The technical detail is below — restart and try again.",
      ],
    };
  }
  return null;
}

/// Keep only the last 8 non-empty lines of a verbose stderr tail — the root
/// cause of an init failure is in the final lines, so this keeps the guidance
/// on-screen without the startup banner pushing it away.
export function lastLines(text: string, n = 8): string {
  return text.split("\n").filter((l) => l.trim().length > 0).slice(-n).join("\n");
}

/// STT errors as actionable guidance, not a raw dump. Unknown errors fall back
/// to the plain message. Mirrors `ImportError`.
export function SttError({ message, testid = "stt-error" }: { message: string; testid?: string }) {
  const os = useHostOs();
  const g = sttGuidance(message, os);
  if (!g) {
    return (
      <div role="alert" data-testid={testid} className="text-red-600 text-xs">
        {message}
      </div>
    );
  }
  return (
    <div
      role="alert"
      data-testid={testid}
      className="text-xs text-amber-800 bg-amber-50 border border-amber-200 rounded p-3 flex flex-col gap-1"
    >
      <div className="font-semibold">{g.title}</div>
      <ul className="list-disc pl-4 space-y-0.5">
        {g.steps.map((s) => (
          <li key={s}>{s}</li>
        ))}
      </ul>
      <div className="text-[10px] text-amber-700 pt-1 break-all whitespace-pre-wrap">
        Details: {lastLines(message)}
      </div>
    </div>
  );
}
