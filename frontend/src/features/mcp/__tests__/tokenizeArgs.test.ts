import { describe, it, expect } from "vitest";
import { tokenizeArgs } from "../components/McpConnectPanel";

describe("tokenizeArgs", () => {
  it("keeps a quoted path with spaces as ONE argument", () => {
    // The macOS app-config dir contains "Application Support" — plain whitespace split tore it
    // in two, so the sqlite server got a broken db path.
    expect(
      tokenizeArgs('-y mcp-server-sqlite-npx "/Users/d/Library/Application Support/app/scratch.db"'),
    ).toEqual(["-y", "mcp-server-sqlite-npx", "/Users/d/Library/Application Support/app/scratch.db"]);
  });

  it("splits unquoted args on whitespace", () => {
    expect(tokenizeArgs("-y @modelcontextprotocol/server-filesystem")).toEqual([
      "-y",
      "@modelcontextprotocol/server-filesystem",
    ]);
  });

  it("strips placeholder angle-brackets a user copies literally", () => {
    expect(tokenizeArgs("-y mcp-server-sqlite-npx </Users/d/scratch.db>")).toEqual([
      "-y",
      "mcp-server-sqlite-npx",
      "/Users/d/scratch.db",
    ]);
  });
});
