import { describe, expect, it } from "vitest";
import {
  GENERIC_PROBLEM,
  formToPin,
  formToPort,
  pinToForm,
  portToForm,
  registryAllowed,
  statusView,
  toggleAction,
  toggleLabel,
} from "./model";

describe("statusView", () => {
  it("maps every status", () => {
    expect(statusView("Stopped")).toEqual({ label: "Stopped", tone: "idle" });
    expect(statusView("Starting")).toEqual({ label: "Starting…", tone: "busy" });
    expect(statusView("Waiting")).toEqual({ label: "Ready for a device", tone: "ok" });
    expect(statusView({ Connected: { device: "Ann's iPhone" } })).toEqual({
      label: "Connected: Ann's iPhone",
      tone: "ok",
    });
  });
  it("shows a plain sentence for errors, raw text only as secondary detail", () => {
    expect(statusView({ Error: "boom" })).toEqual({
      label: "Problem",
      tone: "bad",
      detail: GENERIC_PROBLEM,
      raw: "boom",
    });
    expect(statusView({ Error: "boom" }, "A video component (app) is missing. Reinstall Castaway.")).toEqual({
      label: "Problem",
      tone: "bad",
      detail: "A video component (app) is missing. Reinstall Castaway.",
      raw: "boom",
    });
    expect(GENERIC_PROBLEM).toBe(
      "Castaway's streaming engine stopped unexpectedly. Try Restart, or reinstall Castaway.",
    );
  });
});

describe("toggleAction", () => {
  it("maps every status", () => {
    expect(toggleAction("Stopped")).toBe("start");
    expect(toggleAction("Starting")).toBe("stop");
    expect(toggleAction("Waiting")).toBe("stop");
    expect(toggleAction({ Connected: { device: "d" } })).toBe("stop");
    expect(toggleAction({ Error: "x" })).toBe("restart");
  });
  it("labels", () => {
    expect(toggleLabel("start")).toBe("Start");
    expect(toggleLabel("stop")).toBe("Stop");
    expect(toggleLabel("restart")).toBe("Restart");
  });
});

describe("pin mapping", () => {
  it("pinToForm", () => {
    expect(pinToForm("Off")).toEqual({ mode: "off", fixed: "" });
    // Random is no longer offered: the user must choose a fixed PIN (or Off).
    expect(pinToForm("Random")).toEqual({ mode: "fixed", fixed: "" });
    expect(pinToForm({ Fixed: "1234" })).toEqual({ mode: "fixed", fixed: "1234" });
  });
  it("formToPin", () => {
    expect(formToPin("off", "1234")).toBe("Off");
    expect(formToPin("fixed", "1234")).toEqual({ Fixed: "1234" });
    expect(formToPin("fixed", "")).toEqual({ Fixed: "" });
  });
  it("registryAllowed only with a fixed PIN", () => {
    expect(registryAllowed("off")).toBe(false);
    expect(registryAllowed("fixed")).toBe(true);
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
