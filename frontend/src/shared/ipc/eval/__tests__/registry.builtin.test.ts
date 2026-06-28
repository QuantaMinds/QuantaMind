import { describe, it, expect, vi, beforeEach } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

import { invoke } from "@tauri-apps/api/core";
import { listBuiltinCollections } from "../registry";

const valid = { id: "easy-coding", label: "Coding", domain: "coding", tier: "easy" };
const badTier = { id: "weird-x", label: "X", domain: "x", tier: "expert" }; // tier not in the enum

beforeEach(() => vi.clearAllMocks());

describe("listBuiltinCollections — per-row resilience", () => {
  it("drops a malformed row (and WARNS) instead of throwing the whole list", async () => {
    // One bad scenario must not blank the entire picker (the c7a697a incident) — and the
    // drop must be loud, not silent, so a future bad scenario stays diagnosable.
    vi.mocked(invoke).mockResolvedValue([valid, badTier]);
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    const out = await listBuiltinCollections();
    expect(out).toEqual([valid]); // the valid preset still loads
    expect(warn).toHaveBeenCalled(); // never silent
    expect(warn.mock.calls[0].join(" ")).toContain("weird-x"); // names the offending id
    warn.mockRestore();
  });

  it("returns every row when all are valid, with no spurious warning", async () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    vi.mocked(invoke).mockResolvedValue([valid, { ...valid, id: "hard-coding", tier: "hard" }]);
    const out = await listBuiltinCollections();
    expect(out).toHaveLength(2);
    expect(warn).not.toHaveBeenCalled();
    warn.mockRestore();
  });
});
