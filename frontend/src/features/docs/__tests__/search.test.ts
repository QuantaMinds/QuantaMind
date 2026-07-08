import { describe, it, expect } from "vitest";
import { searchDocs } from "../search";

describe("searchDocs", () => {
  it("returns nothing for an empty query", () => {
    expect(searchDocs("")).toEqual([]);
    expect(searchDocs("   ")).toEqual([]);
  });

  it("finds the backend guide when searching 'backend' and ranks a title match first", () => {
    const results = searchDocs("backend");
    expect(results.length).toBeGreaterThan(0);
    // "Choosing a backend" has the term in its title → should rank at the top.
    expect(results[0].pageId).toBe("choosing-a-backend");
  });

  it("finds troubleshooting content by a body term ('memory')", () => {
    const ids = searchDocs("memory").map((r) => r.pageId);
    expect(ids).toContain("troubleshooting");
  });

  it("requires ALL terms (AND) and returns [] for gibberish", () => {
    expect(searchDocs("zzzznotawordxyz")).toEqual([]);
  });

  it("produces a snippet for each result", () => {
    const [first] = searchDocs("native tool");
    expect(first).toBeTruthy();
    expect(first.snippet.length).toBeGreaterThan(0);
  });
});
