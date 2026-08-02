import { HfPhaseSchema } from "../../../shared/ipc/models/hf_install";
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
  const { bytes_completed: done, bytes_total: total } = p.data;
  upsertDownload({
    id: activeHfName, source: "huggingface", name: activeHfName,
    status: "downloading", percent: pct(done, total),
    bytesCompleted: done, bytesTotal: total, phaseLabel: "Downloading",
  });
}
