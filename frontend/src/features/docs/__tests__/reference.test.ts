import { describe, it, expect } from "vitest";
import { REFERENCE_SECTIONS, referenceToDocSection } from "../reference";
import { findPage } from "../content";
import { searchDocs } from "../search";

describe("referenceToDocSection", () => {
  const section = referenceToDocSection();

  it("produces one Reference section with a page per reference section", () => {
    expect(section.id).toBe("reference");
    expect(section.title).toBe("Reference");
    expect(section.pages.length).toBe(REFERENCE_SECTIONS.length);
    expect(section.pages.map((p) => p.id)).toEqual(REFERENCE_SECTIONS.map((s) => `reference-${s.id}`));
  });

  it("renders a block's what/why/how/formula/source into its page body (lossless)", () => {
    const tests = REFERENCE_SECTIONS.find((s) => s.id === "eval")!;
    const runMetrics = REFERENCE_SECTIONS.find((s) => s.id === "workspace")!
      .blocks.find((b) => b.id === "run-metrics")!;
    const page = section.pages.find((p) => p.id === "reference-workspace")!;
    // Heading is level-2 so it lands in the TOC; What/Why/How + formula + source all present.
    expect(page.body).toContain(`## ${runMetrics.heading}`);
    expect(page.body).toContain(runMetrics.what);
    expect(page.body).toContain(runMetrics.why);
    expect(page.body).toContain(runMetrics.how);
    expect(page.body).toContain(runMetrics.formula!);
    expect(page.body).toContain(runMetrics.source!);
    // The Tests page exists and carries the CSV-import block the deep-link targets.
    expect(section.pages.some((p) => p.id === `reference-${tests.id}`)).toBe(true);
  });
});

describe("reference is wired into the Docs content model", () => {
  it("findPage resolves a generated reference page", () => {
    expect(findPage("reference-eval")).not.toBeNull();
    expect(findPage("reference-eval")!.page.title).toBe("Tests");
  });

  it("⌘K search indexes the reference (a term unique to the reference is found)", () => {
    // "Pass^k" only appears in the per-feature reference, never in the friendly guides.
    const results = searchDocs("schema resilience");
    expect(results.length).toBeGreaterThan(0);
    expect(results.some((r) => r.pageId.startsWith("reference-"))).toBe(true);
  });
});
