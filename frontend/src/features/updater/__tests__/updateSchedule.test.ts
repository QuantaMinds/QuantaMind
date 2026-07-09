import { describe, it, expect } from "vitest";
import { shouldCheck, DAY_MS } from "../updateSchedule";

const now = 1_700_000_000_000;

describe("shouldCheck", () => {
  it("checks when never checked before", () => {
    expect(shouldCheck(null, now)).toBe(true);
    expect(shouldCheck(undefined, now)).toBe(true);
  });

  it("checks when the timestamp is unparseable", () => {
    expect(shouldCheck("not-a-date", now)).toBe(true);
  });

  it("skips when checked under 24h ago", () => {
    const recent = new Date(now - DAY_MS / 2).toISOString();
    expect(shouldCheck(recent, now)).toBe(false);
  });

  it("checks when more than 24h has passed", () => {
    const old = new Date(now - DAY_MS - 1000).toISOString();
    expect(shouldCheck(old, now)).toBe(true);
  });

  it("checks exactly at the 24h boundary", () => {
    const boundary = new Date(now - DAY_MS).toISOString();
    expect(shouldCheck(boundary, now)).toBe(true);
  });
});
