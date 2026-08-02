import { describe, it, expect } from "vitest";
import { dedupeByDigest } from "../dedupeDigest";

const m = (name: string, digest?: string) => ({ name, digest });

describe("dedupeByDigest", () => {
  it("collapses the same blob imported under multiple tags (first wins)", () => {
    // The real-world case: one model registered under two tag names.
    const out = dedupeByDigest([
      m("gemma-2b_q3_k_l:latest", "3d3d"),
      m("gemma-2b_q2_k:latest", "948a"),
      m("gemma-2b:q2_k", "948a"),
      m("gemma-2b:q3_k_l", "3d3d"),
    ]);
    expect(out.map((x) => x.name)).toEqual([
      "gemma-2b_q3_k_l:latest",
      "gemma-2b_q2_k:latest",
    ]);
  });

  it("keeps distinct digests (different quants are not duplicates)", () => {
    const out = dedupeByDigest([m("a", "x"), m("b", "y")]);
    expect(out).toHaveLength(2);
  });

  it("never merges entries without a digest (llama.cpp)", () => {
    const out = dedupeByDigest([m("phi3"), m("llama3"), m("remote-x")]);
    expect(out.map((x) => x.name)).toEqual(["phi3", "llama3", "remote-x"]);
  });

  it("treats an empty-string digest as absent", () => {
    const out = dedupeByDigest([m("a", ""), m("b", "")]);
    expect(out).toHaveLength(2);
  });
});
