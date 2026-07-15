import { describe, it, expect } from "vitest";
import { toTaskDef, type BuilderForm } from "../components/McpTaskBuilder";

const base: BuilderForm = {
  name: "t",
  instruction: "do it",
  k: 10,
  worldType: "fs",
  files: [],
  setupSql: "",
  present: "",
  absent: "",
  content: "",
  assertEq: "",
};

describe("toTaskDef", () => {
  it("builds a filesystem task with seed files and present/absent/content oracle", () => {
    const t = toTaskDef({
      ...base,
      files: [{ path: "keep.txt", content: "keep me" }, { path: "", content: "ignored blank" }],
      present: "result.txt\nkeep.txt",
      absent: "old.log",
      content: "result.txt :: DONE",
    });
    expect(t.world).toEqual({ type: "fs", files: [{ path: "keep.txt", content: "keep me" }] });
    expect(t.oracle.assert_present).toEqual(["result.txt", "keep.txt"]);
    expect(t.oracle.assert_absent).toEqual(["old.log"]);
    expect(t.oracle.assert_content).toEqual([["result.txt", "DONE"]]);
    expect(t.k).toBe(10);
  });

  it("builds a db task with setup SQL and query assertions", () => {
    const t = toTaskDef({
      ...base,
      worldType: "db",
      setupSql: "CREATE TABLE users(name TEXT);",
      assertEq: "SELECT COUNT(*) FROM users WHERE name='Alice' :: 1",
    });
    expect(t.world).toEqual({ type: "db", setupSql: "CREATE TABLE users(name TEXT);" });
    expect(t.oracle.assert_eq).toEqual([["SELECT COUNT(*) FROM users WHERE name='Alice'", "1"]]);
  });

  it("trims name/instruction and drops blank seed rows", () => {
    const t = toTaskDef({ ...base, name: "  x  ", instruction: "  go  ", files: [{ path: "  ", content: "" }] });
    expect(t.name).toBe("x");
    expect(t.instruction).toBe("go");
    expect(t.world).toEqual({ type: "fs", files: [] });
  });
});
