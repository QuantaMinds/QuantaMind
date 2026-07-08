import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import { DocMarkdown, tocFromMarkdown, slugify } from "../render";

vi.mock("@tauri-apps/plugin-shell", () => ({ open: vi.fn() }));

describe("docs render", () => {
  it("slugifies headings for anchors", () => {
    expect(slugify("How to find files")).toBe("how-to-find-files");
    expect(slugify("Native tool-calling!")).toBe("native-tool-calling");
  });

  it("extracts only h2/h3 into the TOC with the right ids", () => {
    const toc = tocFromMarkdown("# Title\n\n## First\ntext\n\n### Nested\n\n## Second");
    expect(toc).toEqual([
      { id: "first", title: "First", level: 2 },
      { id: "nested", title: "Nested", level: 3 },
      { id: "second", title: "Second", level: 2 },
    ]);
  });

  it("renders a fenced code block, a callout, and an anchored heading", () => {
    render(<DocMarkdown markdown={"## Setup\n\n```bash\nollama pull qwen3.5\n```\n\n> [!TIP]\n> Use prompt-based mode."} />);
    // Code block content
    expect(screen.getByTestId("doc-code").textContent).toContain("ollama pull qwen3.5");
    // Callout with its label
    const callout = screen.getByTestId("doc-callout");
    expect(callout.textContent).toContain("Tip");
    expect(callout.textContent).toContain("prompt-based");
    // Heading carries a slug id (for the TOC + deep links)
    expect(document.getElementById("setup")).not.toBeNull();
  });

  it("renders a pipe table with header cells", () => {
    render(<DocMarkdown markdown={"| Backend | Note |\n| --- | --- |\n| Ollama | easy |"} />);
    expect(screen.getByText("Backend")).toBeTruthy();
    expect(screen.getByText("Ollama")).toBeTruthy();
  });
});
