import { describe, it, expect, beforeEach } from "vitest";
import { useNavStore } from "../navStore";

beforeEach(() => useNavStore.setState({ topView: "workspace", history: [] }));

describe("navStore", () => {
  it("defaults to workspace", () => {
    expect(useNavStore.getState().topView).toBe("workspace");
  });

  it("pushes the prior view onto history when navigating", () => {
    useNavStore.getState().setTopView("models");
    expect(useNavStore.getState().topView).toBe("models");
    expect(useNavStore.getState().history).toEqual(["workspace"]);
  });

  it("goBack returns to the previous view and pops history", () => {
    const { setTopView } = useNavStore.getState();
    setTopView("models");
    setTopView("downloads");
    useNavStore.getState().goBack();
    expect(useNavStore.getState().topView).toBe("models");
    useNavStore.getState().goBack();
    expect(useNavStore.getState().topView).toBe("workspace");
    expect(useNavStore.getState().history).toEqual([]);
  });

  it("goBack is a no-op with empty history", () => {
    useNavStore.getState().goBack();
    expect(useNavStore.getState().topView).toBe("workspace");
  });

  it("ignores navigating to the current view (no dup history)", () => {
    useNavStore.getState().setTopView("workspace");
    expect(useNavStore.getState().history).toEqual([]);
  });

  it("caps history at 20 entries", () => {
    const views = ["models", "compare", "downloads", "docs"] as const;
    for (let i = 0; i < 30; i++) useNavStore.getState().setTopView(views[i % 4]);
    expect(useNavStore.getState().history.length).toBeLessThanOrEqual(20);
  });
});
