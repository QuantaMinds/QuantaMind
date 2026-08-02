import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";

vi.mock("@tauri-apps/plugin-shell", () => ({ open: vi.fn() }));
vi.mock("../../../shared/ipc/publish/publish", () => ({ publishToBoard: vi.fn(), startLogin: vi.fn() }));
vi.mock("../../../shared/ipc/publish/preview", () => ({ previewPublishPayload: vi.fn() }));

import { open as openUrl } from "@tauri-apps/plugin-shell";
import { publishToBoard, startLogin } from "../../../shared/ipc/publish/publish";
import { previewPublishPayload } from "../../../shared/ipc/publish/preview";
import { PublishButton } from "../PublishButton";
import { ToastHost } from "../../../shared/ui/Toast";
import type { ModelVerdict } from "../../../shared/ipc/eval/readiness";

const VERDICTS: ModelVerdict[] = [
  { model: "qwen", backend: "llama_cpp", verdict: { status: "ready", blocking: [], conditions: [], path: "native_fc" }, pass_k: 0.9 },
];

const PREVIEW = {
  rows: [{
    model: "qwen", quant: "Q4_K_M", cohort_key: "c", tool_version: "0.2.0", metrics: { pass_k: 0.9 }, params: {},
    status: "ready" as const, eval_method: "native_fc" as const, hardware_class: "mainstream" as const, recommended_tier: "medium" as const, by_tier: [],
    failure_distribution: { infinite_loop: 0, hallucinated: 0, malformed_json: 0, schema_unrecovered: 0, unknown_tool_calls: 0, forbidden_calls: 0, turn_timeouts: 0, reported_in_prose: 0 },
    is_thinking: false, think_preset: "standard" as const, cpu_offloaded: false,
    collection_name: "easy-coding", collection_hash: "abc", schema_version: 2, engine_version: "0.2.0", build_hash: "testhash",
  }],
  canonical_json: '[{"model":"qwen"}]', hash: "h", cohort_key: "c", excluded_count: 0, invalid: null,
};

async function openDialogAndAgree() {
  fireEvent.click(screen.getByTestId("publish-open"));
  fireEvent.click(await screen.findByTestId("publish-optin"));
  fireEvent.click(screen.getByTestId("publish-confirm"));
}

beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(previewPublishPayload).mockResolvedValue(PREVIEW);
  vi.mocked(openUrl).mockResolvedValue(undefined);
  vi.mocked(startLogin).mockResolvedValue(true);
});

describe("PublishButton", () => {
  it("opens the privacy dialog on click", async () => {
    render(<><PublishButton verdicts={VERDICTS} collectionId="easy-coding" collectionHash="abc" runParams={{ temperature: 0.2 }} /><ToastHost /></>);
    fireEvent.click(screen.getByTestId("publish-open"));
    expect(await screen.findByTestId("publish-dialog")).toBeInTheDocument();
  });

  it("toasts success and opens the board url on ok", async () => {
    vi.mocked(publishToBoard).mockResolvedValue({ kind: "ok", board_url: "https://quantamind.co/b/1" });
    render(<><PublishButton verdicts={VERDICTS} collectionId="easy-coding" collectionHash="abc" runParams={{ temperature: 0.2 }} /><ToastHost /></>);
    await openDialogAndAgree();
    await waitFor(() => expect(screen.getByTestId("toast")).toHaveTextContent("Published"));
    expect(openUrl).toHaveBeenCalledWith("https://quantamind.co/b/1");
    // The RUN-stamped params (threaded from the batch report) are what get published.
    expect(publishToBoard).toHaveBeenCalledWith(VERDICTS, { temperature: 0.2 }, "easy-coding", "abc", "");
  });

  it("surfaces the failing row index on invalid, without crashing", async () => {
    vi.mocked(publishToBoard).mockResolvedValue({ kind: "invalid", index: 2 });
    render(<><PublishButton verdicts={VERDICTS} collectionId="easy-coding" collectionHash="abc" runParams={{ temperature: 0.2 }} /><ToastHost /></>);
    await openDialogAndAgree();
    await waitFor(() => expect(screen.getByTestId("toast")).toHaveTextContent("Row 2"));
  });

  it("signs in then auto-publishes on needs_auth, without a second click", async () => {
    vi.mocked(publishToBoard)
      .mockResolvedValueOnce({ kind: "needs_auth" })
      .mockResolvedValueOnce({ kind: "ok", board_url: "https://quantamind.co/b/2" });
    render(<><PublishButton verdicts={VERDICTS} collectionId="easy-coding" collectionHash="abc" runParams={{ temperature: 0.2 }} /><ToastHost /></>);
    await openDialogAndAgree();
    await waitFor(() => expect(startLogin).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(screen.getByTestId("toast")).toHaveTextContent("Published"));
    expect(publishToBoard).toHaveBeenCalledTimes(2);
    expect(openUrl).toHaveBeenCalledWith("https://quantamind.co/b/2");
  });

  it("warns when sign-in only persisted for the session (keychain denied), then still publishes", async () => {
    vi.mocked(startLogin).mockResolvedValue(false);
    vi.mocked(publishToBoard)
      .mockResolvedValueOnce({ kind: "needs_auth" })
      .mockResolvedValueOnce({ kind: "ok", board_url: "https://quantamind.co/b/3" });
    render(<><PublishButton verdicts={VERDICTS} collectionId="easy-coding" collectionHash="abc" runParams={{ temperature: 0.2 }} /><ToastHost /></>);
    await openDialogAndAgree();
    await waitFor(() => expect(screen.getByTestId("toast")).toHaveTextContent("this session"));
    await waitFor(() => expect(screen.getByTestId("toast")).toHaveTextContent("Published"));
  });

  it("stops after one retry if sign-in still leaves needs_auth", async () => {
    vi.mocked(publishToBoard).mockResolvedValue({ kind: "needs_auth" });
    render(<><PublishButton verdicts={VERDICTS} collectionId="easy-coding" collectionHash="abc" runParams={{ temperature: 0.2 }} /><ToastHost /></>);
    await openDialogAndAgree();
    await waitFor(() => expect(screen.getByTestId("toast")).toHaveTextContent("didn't complete"));
    expect(startLogin).toHaveBeenCalledTimes(1);
    expect(publishToBoard).toHaveBeenCalledTimes(2);
  });

  it("toasts the login error when sign-in itself fails", async () => {
    vi.mocked(publishToBoard).mockResolvedValue({ kind: "needs_auth" });
    vi.mocked(startLogin).mockRejectedValue(new Error("browser closed"));
    render(<><PublishButton verdicts={VERDICTS} collectionId="easy-coding" collectionHash="abc" runParams={{ temperature: 0.2 }} /><ToastHost /></>);
    await openDialogAndAgree();
    await waitFor(() => expect(screen.getByTestId("toast")).toHaveTextContent("browser closed"));
    expect(publishToBoard).toHaveBeenCalledTimes(1);
  });

  it("toasts a friendly message on rate_limited", async () => {
    vi.mocked(publishToBoard).mockResolvedValue({ kind: "rate_limited" });
    render(<><PublishButton verdicts={VERDICTS} collectionId="easy-coding" collectionHash="abc" runParams={{ temperature: 0.2 }} /><ToastHost /></>);
    await openDialogAndAgree();
    await waitFor(() => expect(screen.getByTestId("toast")).toHaveTextContent("try again shortly"));
  });

  it("never throws when the IPC itself rejects", async () => {
    vi.mocked(publishToBoard).mockRejectedValue(new Error("backend down"));
    render(<><PublishButton verdicts={VERDICTS} collectionId="easy-coding" collectionHash="abc" runParams={{ temperature: 0.2 }} /><ToastHost /></>);
    await openDialogAndAgree();
    await waitFor(() => expect(screen.getByTestId("toast")).toHaveTextContent("Publish failed"));
  });
});
