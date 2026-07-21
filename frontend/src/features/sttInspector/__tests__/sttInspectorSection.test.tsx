import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen } from "@testing-library/react";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn().mockResolvedValue(undefined) }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn().mockResolvedValue(() => {}) }));

import { InspectorPage } from "../../inspector/components/InspectorPage";
import { useCompareStore } from "../../compare/state/compareStore";
import { useSttResultStore } from "../state/sttResultStore";
import type { Transcript } from "../../../shared/ipc/stt/transcribe";

const TRANSCRIPT: Transcript = {
  id: "clip-1",
  model: "ggml-tiny.en.bin",
  language: "en",
  audio: { sample_rate_hz: 16000, channels: 1, duration_secs: 6 },
  segments: [
    { text: "hello there", start_secs: 0, end_secs: 2, avg_logprob: -0.2, no_speech_prob: 0.01, words: null },
    { text: "muffled bit", start_secs: 2, end_secs: 4, avg_logprob: -1.5, no_speech_prob: 0.02, words: null },
  ],
  complete: true,
  stats: {
    source_duration_secs: 6,
    audio_decoded_secs: 6,
    transcribe_wall_ms: 2500,
    segment_count: 2,
    detected_language: "en",
    received_sample_rate_hz: 16000,
    rtf: 2.4,
  },
  stt_profile: {
    perf: { first_segment_ms: 180, encode_ms: null, decode_ms: null },
    behavioral: { repeat_rate: 0, confidence: { mean: 0.9, low_percentile: 0.6 }, silence_hallucination_rate: 0 },
    vram_bytes: null,
  },
};

beforeEach(() => {
  useCompareStore.setState({ rows: [] });
  useSttResultStore.setState({ result: null });
});

describe("Inspector STT wiring", () => {
  it("shows the Test-run empty hint (no STT section) when nothing has run", () => {
    // The Latency page is Test-run-only now; its empty state comes from EvalRunPanel.
    render(<InspectorPage />);
    expect(screen.getByTestId("eval-run-empty")).toBeInTheDocument();
    expect(screen.queryByTestId("stt-inspector-section")).toBeNull();
  });

  it("renders the STT section after a transcription", () => {
    useSttResultStore.setState({ result: TRANSCRIPT });
    render(<InspectorPage />);
    expect(screen.getByTestId("stt-inspector-section")).toBeInTheDocument();
    expect(screen.getByTestId("stt-phase-bar")).toBeInTheDocument();
    expect(screen.getByTestId("confidence-timeline")).toBeInTheDocument();
    expect(screen.getByTestId("stt-metric-cards")).toBeInTheDocument();
    expect(screen.getByTestId("stt-card-rtf").textContent).toBe("2.40×");
    // The low-confidence segment is flagged red in the timeline.
    expect(screen.getByTestId("conf-bar-low-1")).toBeInTheDocument();
  });
});
