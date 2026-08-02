import { describe, it, expect } from "vitest";
import { toJson } from "../format/jsonReport";
import { buildReport } from "../format/buildReport";

const FIXED = () => new Date("2026-05-23T14:01:22.000Z");
const UID = () => "01TESTULIDFIXED0000000000A";

describe("toJson", () => {
  it("produces parseable JSON with the analysis-document top-level keys", () => {
    const json = toJson(buildReport({
      prompt: "hi",
      hardwareSnapshot: { total_memory_bytes: 1, available_memory_bytes: 1, is_apple_silicon: false },
      selectedModels: [], rows: [], now: FIXED, uid: UID,
    }));
    const parsed = JSON.parse(json);
    expect(Object.keys(parsed).sort()).toEqual([
      "created_at", "document_id", "document_type", "environment", "findings",
      "models", "prompts", "reproducibility", "runs",
      "schema_version", "title", "verdicts",
    ]);
    expect(parsed.schema_version).toBe("1.0.0");
    expect(parsed.document_type).toBe("bench-report");
  });

  it("indents with 2 spaces", () => {
    const json = toJson(buildReport({
      prompt: "x", hardwareSnapshot: null,
      selectedModels: [], rows: [], now: FIXED, uid: UID,
    }));
    expect(json).toContain('  "schema_version":');
  });
});
