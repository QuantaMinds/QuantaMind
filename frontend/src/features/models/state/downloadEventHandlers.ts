import { HfPhaseSchema } from "../../../shared/ipc/models/hf_install";
import { LocalInstallPhaseSchema } from "../../../shared/ipc/models/local_install";
import { PullProgressEventSchema } from "../../../shared/ipc/events/pull_events";
import { useModelStore } from "./modelStore";

const pct = (done: number, total: number) =>
  total > 0 ? Math.min(100, Math.round((done / total) * 100)) : 0;

export function onHf(payload: unknown) {
  const p = HfPhaseSchema.safeParse(payload);
  if (!p.success) {
    console.error("invalid hf-progress payload", p.error.issues);
    return;
  }
  const { activeHfName, upsertDownload } = useModelStore.getState();
  if (!activeHfName) return;
  const base = { id: activeHfName, source: "huggingface" as const, name: activeHfName };
  if (p.data.phase === "downloading") {
    const { bytes_completed: done, bytes_total: total } = p.data;
    upsertDownload({ ...base, status: "downloading", percent: pct(done, total),
      bytesCompleted: done, bytesTotal: total, phaseLabel: "Downloading" });
    return;
  }
  if (p.data.phase === "hashing" || p.data.phase === "uploading") {
    const { bytes_completed, bytes_total } = p.data;
    const phaseLabel = p.data.phase === "hashing" ? "Hashing" : "Uploading to Ollama";
    upsertDownload({ ...base, status: "installing", percent: pct(bytes_completed, bytes_total),
      bytesCompleted: bytes_completed, bytesTotal: bytes_total, phaseLabel });
    return;
  }
  upsertDownload({ ...base, status: "installing", percent: 100, phaseLabel: "Creating model" });
}

export function onPull(payload: unknown) {
  const p = PullProgressEventSchema.safeParse(payload);
  if (!p.success) {
    console.error("invalid pull-progress payload", p.error.issues);
    return;
  }
  const { pullNames, upsertDownload } = useModelStore.getState();
  const name = pullNames[p.data.pull_id] ?? p.data.name;
  if (!name) return;
  const prog = p.data.progress;
  if (prog.phase === "failed") {
    upsertDownload({ id: name, source: "ollama", name, status: "error",
      percent: 0, error: prog.message, pullId: p.data.pull_id });
    return;
  }
  const isSuccess = prog.phase === "success";
  const percent = prog.phase === "downloading" ? pct(prog.completed, prog.total) : isSuccess ? 100 : 0;
  const bytesCompleted = prog.phase === "downloading" ? prog.completed : undefined;
  const bytesTotal = prog.phase === "downloading" ? prog.total : undefined;
  upsertDownload({ id: name, source: "ollama", name,
    status: isSuccess ? "success" : "downloading",
    percent, bytesCompleted, bytesTotal, pullId: p.data.pull_id });
}

export function onLocal(payload: unknown) {
  const p = LocalInstallPhaseSchema.safeParse(payload);
  if (!p.success) {
    console.error("invalid local-install-progress payload", p.error.issues);
    return;
  }
  const { activeLocalName, upsertDownload } = useModelStore.getState();
  if (!activeLocalName) return;
  const base = { id: activeLocalName, source: "local" as const, name: activeLocalName };
  if (p.data.phase === "hashing" || p.data.phase === "uploading") {
    const { bytes_completed, bytes_total } = p.data;
    const phaseLabel = p.data.phase === "hashing" ? "Hashing" : "Uploading to Ollama";
    upsertDownload({ ...base, status: "installing", percent: pct(bytes_completed, bytes_total),
      bytesCompleted: bytes_completed, bytesTotal: bytes_total, phaseLabel });
    return;
  }
  upsertDownload({ ...base, status: "installing", percent: 100, phaseLabel: "Creating model" });
}
