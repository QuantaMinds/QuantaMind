import type { HfInstallState } from "../hooks/useHfInstall";

type Props = {
  state: HfInstallState;
  onCancel: () => void;
  onReset: () => void;
};

export function HfInstallStatus({ state, onCancel, onReset }: Props) {
  if (state.status === "downloading") {
    return (
      <div data-testid="hf-downloading" className="flex items-center gap-2">
        <progress value={state.percent} max={100} className="flex-1 h-2" />
        <span className="text-xs tabular-nums w-10 text-right">{state.percent}%</span>
        <button
          type="button"
          onClick={onCancel}
          className="text-xs border rounded px-2 py-1"
        >
          Cancel
        </button>
      </div>
    );
  }
  if (state.status === "success") {
    return (
      <div
        role="status"
        data-testid="hf-success"
        className="text-green-700 text-xs flex items-center gap-2"
      >
        <span>Installed ✓ — open Workspace or Compare to use it.</span>
        <button type="button" onClick={onReset} className="underline">
          dismiss
        </button>
      </div>
    );
  }
  if (state.status === "error") {
    return (
      <div role="alert" className="text-red-600 text-xs" data-testid="hf-error">
        {state.error}{" "}
        <button type="button" onClick={onReset} className="ml-2 underline">
          dismiss
        </button>
      </div>
    );
  }
  return null;
}
