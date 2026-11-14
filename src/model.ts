// Pure mapping logic between backend shapes and the form. No validation here:
// all rules live in Rust (save_settings returns the error text).

export type PinMode = "Off" | "Random" | { Fixed: string };

export interface Settings {
  name: string;
  hide_hostname: boolean;
  width: number;
  height: number;
  fps: number;
  fullscreen: boolean;
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
  detail?: string;
}

export function statusView(status: Status): StatusView {
  if (status === "Stopped") return { label: "Stopped", tone: "idle" };
  if (status === "Starting") return { label: "Starting…", tone: "busy" };
  if (status === "Waiting") return { label: "Ready for a device", tone: "ok" };
  if ("Connected" in status) {
    return { label: `Connected: ${status.Connected.device}`, tone: "ok" };
  }
  return { label: "Problem", tone: "bad", detail: status.Error };
}

export type PinFormMode = "off" | "random" | "fixed";

export function pinToForm(p: PinMode): { mode: PinFormMode; fixed: string } {
  if (p === "Off") return { mode: "off", fixed: "" };
  if (p === "Random") return { mode: "random", fixed: "" };
  return { mode: "fixed", fixed: p.Fixed };
}

export function formToPin(mode: PinFormMode, fixed: string): PinMode {
  if (mode === "off") return "Off";
  if (mode === "random") return "Random";
  return { Fixed: fixed };
}

export function portToForm(n: number | null): string {
  return n === null ? "" : String(n);
}

export function formToPort(s: string): number | null {
  const t = s.trim();
  return t === "" ? null : Number(t);
}
