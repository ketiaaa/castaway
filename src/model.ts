// Pure mapping logic between backend shapes and the form. No validation here:
// all rules live in Rust (save_settings returns the error text).

// "Random" may still arrive from an old settings file; it is never offered.
export type PinMode = "Off" | "Random" | { Fixed: string };

export interface Settings {
  name: string;
  width: number;
  height: number;
  fps: number;
  fullscreen: boolean;
  low_latency: boolean;
  video_sink: string;
  audio_sink: string;
  pin: PinMode;
  use_registry: boolean;
  port_base: number | null;
  autostart: boolean;
}

export type Status =
  | "Stopped"
  | "Starting"
  | "Waiting"
  | { Connected: { device: string } }
  | { Error: string };

export type Tone = "ok" | "busy" | "idle" | "bad";

export interface StatusView {
  label: string;
  tone: Tone;
  /** Plain sentence shown to people. */
  detail?: string;
  /** Raw engine message, secondary detail only (errors). */
  raw?: string;
}

/** Same text as the Rust health::GENERIC_ERROR. */
export const GENERIC_PROBLEM =
  "Castaway's streaming engine stopped unexpectedly. Try Restart, or reinstall Castaway.";

/** `plain` is the backend's plain sentence for an error (describe_error). */
export function statusView(status: Status, plain?: string): StatusView {
  if (status === "Stopped") return { label: "Stopped", tone: "idle" };
  if (status === "Starting") return { label: "Starting…", tone: "busy" };
  if (status === "Waiting") return { label: "Ready for a device", tone: "ok" };
  if ("Connected" in status) {
    return { label: `Connected: ${status.Connected.device}`, tone: "ok" };
  }
  return { label: "Problem", tone: "bad", detail: plain ?? GENERIC_PROBLEM, raw: status.Error };
}

export type ToggleAction = "stop" | "start" | "restart";

/** In a live error UxPlay may still be running, so the button restarts it. */
export function toggleAction(status: Status): ToggleAction {
  if (status === "Stopped") return "start";
  if (typeof status === "object" && "Error" in status) return "restart";
  return "stop";
}

export function toggleLabel(a: ToggleAction): string {
  return a === "stop" ? "Stop" : a === "start" ? "Start" : "Restart";
}

export type PinFormMode = "off" | "fixed";

export function pinToForm(p: PinMode): { mode: PinFormMode; fixed: string } {
  if (p === "Off") return { mode: "off", fixed: "" };
  if (p === "Random") return { mode: "fixed", fixed: "" };
  return { mode: "fixed", fixed: p.Fixed };
}

export function formToPin(mode: PinFormMode, fixed: string): PinMode {
  if (mode === "off") return "Off";
  return { Fixed: fixed };
}

/** UxPlay only remembers trusted devices when a PIN is used. */
export function registryAllowed(mode: PinFormMode): boolean {
  return mode === "fixed";
}

export function portToForm(n: number | null): string {
  return n === null ? "" : String(n);
}

export function formToPort(s: string): number | null {
  const t = s.trim();
  return t === "" ? null : Number(t);
}
