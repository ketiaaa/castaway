import { describe, expect, it } from "vitest";
import { formToPin, formToPort, pinToForm, portToForm, statusView } from "./model";

describe("statusView", () => {
  it("maps every status", () => {
    expect(statusView("Stopped")).toEqual({ label: "Stopped", tone: "idle" });
    expect(statusView("Starting")).toEqual({ label: "Starting…", tone: "busy" });
    expect(statusView("Waiting")).toEqual({ label: "Ready for a device", tone: "ok" });
    expect(statusView({ Connected: { device: "Ann's iPhone" } })).toEqual({
      label: "Connected: Ann's iPhone",
      tone: "ok",
    });
    expect(statusView({ Error: "boom" })).toEqual({ label: "Problem", tone: "bad", detail: "boom" });
  });
});

describe("pin mapping", () => {
  it("pinToForm", () => {
    expect(pinToForm("Off")).toEqual({ mode: "off", fixed: "" });
    expect(pinToForm("Random")).toEqual({ mode: "random", fixed: "" });
    expect(pinToForm({ Fixed: "1234" })).toEqual({ mode: "fixed", fixed: "1234" });
  });
  it("formToPin", () => {
    expect(formToPin("off", "1234")).toBe("Off");
    expect(formToPin("random", "1234")).toBe("Random");
    expect(formToPin("fixed", "1234")).toEqual({ Fixed: "1234" });
    expect(formToPin("fixed", "")).toEqual({ Fixed: "" });
  });
});

describe("port mapping", () => {
  it("portToForm", () => {
    expect(portToForm(null)).toBe("");
    expect(portToForm(7000)).toBe("7000");
  });
  it("formToPort", () => {
    expect(formToPort("")).toBeNull();
    expect(formToPort("  ")).toBeNull();
    expect(formToPort("7000")).toBe(7000);
    expect(formToPort(" 7000 ")).toBe(7000);
  });
});
