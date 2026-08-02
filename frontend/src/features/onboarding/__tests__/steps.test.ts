import { describe, it, expect } from "vitest";
import { currentStep } from "../steps";

describe("currentStep", () => {
  it("starts at the server step when it isn't healthy", () => {
    expect(currentStep(null, 0)).toBe("server");
    expect(currentStep(false, 5)).toBe("server");
  });

  it("asks for a model once healthy with none installed", () => {
    expect(currentStep(true, 0)).toBe("model");
  });

  it("is ready once healthy with a model", () => {
    expect(currentStep(true, 1)).toBe("ready");
  });
});
