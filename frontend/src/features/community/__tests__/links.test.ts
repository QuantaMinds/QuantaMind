import { describe, it, expect } from "vitest";
import * as links from "../links";
import capability from "../../../../../backend/capabilities/default.json";

/// Every community URL is opened through the scoped `shell:allow-open` allowlist —
/// a URL missing from backend/capabilities/default.json fails at RUNTIME with no
/// compile-time signal. This guard makes the desync a test failure instead: update
/// links.ts and the capability together, or this goes red.
describe("community links stay inside the shell allowlist", () => {
  it("every exported URL appears in backend/capabilities/default.json", () => {
    const allowlist = JSON.stringify(capability);
    for (const [name, url] of Object.entries(links)) {
      expect(allowlist, `${name} (${url}) missing from shell:allow-open`).toContain(`"${url}"`);
    }
  });
});
