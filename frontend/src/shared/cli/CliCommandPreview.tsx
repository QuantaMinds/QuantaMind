import { useState } from "react";
import { useToast } from "../ui/Toast";
import type { QmCommand } from "./qmCommand";

/// The "Equivalent CLI command" disclosure — shown under a Run button, it renders
/// the exact `qm` command for the CURRENT selections (GCP-console pattern), so users
/// can copy it or learn the flags. Collapsed by default (least intrusive), remembers
/// its open state, live-updates as `cmd` changes on re-render.
const LS_KEY = "qm.cliPreview.open";

export function CliCommandPreview({ cmd, testId = "cli-preview" }: { cmd: QmCommand; testId?: string }) {
  // Default OPEN so the command is visible the moment the user selects things; an
  // explicit collapse persists "0" and is respected on return.
  const [open, setOpen] = useState<boolean>(() => {
    try {
      return localStorage.getItem(LS_KEY) !== "0";
    } catch {
      return true;
    }
  });
  const [copied, setCopied] = useState(false);
  const toast = useToast();

  const toggle = () => {
    const next = !open;
    setOpen(next);
    try {
      localStorage.setItem(LS_KEY, next ? "1" : "0");
    } catch {
      /* private mode — non-fatal */
    }
  };

  const copy = async () => {
    try {
      await navigator.clipboard.writeText(cmd.command);
      setCopied(true);
      toast("Command copied");
      setTimeout(() => setCopied(false), 1500);
    } catch {
      /* clipboard unavailable — the command is shown inline to copy by hand */
    }
  };

  return (
    <div className="text-xs" data-testid={testId} style={{ fontFamily: "Inter, sans-serif" }}>
      <button
        type="button"
        onClick={toggle}
        aria-expanded={open}
        data-testid={`${testId}-toggle`}
        className="flex items-center gap-1.5 text-slate-500 hover:text-slate-700 select-none"
        style={{ background: "none", border: "none", padding: 0, cursor: "pointer", font: "inherit" }}
      >
        <span style={{ display: "inline-block", transform: open ? "rotate(90deg)" : "none", transition: "transform 120ms" }}>▸</span>
        <span className="font-semibold tracking-wide uppercase text-[10px]">⌘ Equivalent CLI command</span>
      </button>

      {open && (
        <div className="mt-1.5 flex items-start gap-2">
          <code
            data-testid={`${testId}-command`}
            className="flex-1 bg-gray-50 border border-slate-200 rounded px-2 py-1.5 text-[11px] break-all font-mono text-slate-700"
          >
            {cmd.command}
          </code>
          <button
            type="button"
            onClick={copy}
            data-testid={`${testId}-copy`}
            className="text-[11px] border border-slate-300 rounded px-2 py-1 shrink-0 text-slate-600 hover:bg-slate-50"
          >
            {copied ? "Copied" : "Copy"}
          </button>
        </div>
      )}
      {open && (cmd.note || cmd.incomplete) && (
        <div className="mt-1 text-[10px] text-slate-400" data-testid={`${testId}-note`}>
          {cmd.incomplete ? "Pick a model to complete this command. " : ""}
          {cmd.note ?? ""}
        </div>
      )}
    </div>
  );
}
