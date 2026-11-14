import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
  type Settings,
  type Status,
  formToPin,
  formToPort,
  pinToForm,
  portToForm,
  statusView,
} from "./model";

const $ = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;
const text = (e: unknown) => (typeof e === "string" ? e : e instanceof Error ? e.message : String(e));

const banner = $("banner");
const toggle = $<HTMLButtonElement>("toggle");
const loadError = $("load-error");
const saveBtn = $<HTMLButtonElement>("save");
const saveMsg = $("save-msg");
const pinFixed = $<HTMLInputElement>("pin_fixed");

const val = (id: string) => $<HTMLInputElement | HTMLSelectElement>(id).value;
const checked = (id: string) => $<HTMLInputElement>(id).checked;
const setVal = (id: string, v: string) => ($<HTMLInputElement | HTMLSelectElement>(id).value = v);
const setChecked = (id: string, v: boolean) => ($<HTMLInputElement>(id).checked = v);
const pinRadios = () => Array.from(document.querySelectorAll<HTMLInputElement>('input[name="pin"]'));

function showError(e: unknown) {
  loadError.textContent = `Something went wrong: ${text(e)}`;
  loadError.hidden = false;
}
function clearError() {
  loadError.hidden = true;
}

let running = false;

function renderStatus(status: Status) {
  const v = statusView(status);
  banner.className = `banner ${v.tone}`;
  $("banner-label").textContent = v.label;
  const d = $("banner-detail");
  d.textContent = v.detail ?? "";
  d.hidden = !v.detail;
  running = status !== "Stopped" && !(typeof status === "object" && "Error" in status);
  toggle.textContent = running ? "Stop" : "Start";
  toggle.disabled = false;
}

function updatePinInput() {
  const mode = pinRadios().find((r) => r.checked)?.value;
  pinFixed.hidden = mode !== "fixed";
}

function fillForm(s: Settings) {
  setVal("name", s.name);
  setChecked("hide_hostname", s.hide_hostname);
  setVal("width", String(s.width));
  setVal("height", String(s.height));
  setVal("fps", String(s.fps));
  setChecked("fullscreen", s.fullscreen);
  setVal("video_sink", s.video_sink);
  setVal("audio_sink", s.audio_sink);
  const p = pinToForm(s.pin);
  pinRadios().forEach((r) => (r.checked = r.value === p.mode));
  pinFixed.value = p.fixed;
  setChecked("use_registry", s.use_registry);
  setVal("port_base", portToForm(s.port_base));
  setChecked("autostart", s.autostart);
  updatePinInput();
}

function readForm(): Settings {
  const mode = (pinRadios().find((r) => r.checked)?.value ?? "off") as "off" | "random" | "fixed";
  return {
    name: val("name"),
    hide_hostname: checked("hide_hostname"),
    width: Number(val("width")),
    height: Number(val("height")),
    fps: Number(val("fps")),
    fullscreen: checked("fullscreen"),
    video_sink: val("video_sink"),
    audio_sink: val("audio_sink"),
    pin: formToPin(mode, pinFixed.value),
    use_registry: checked("use_registry"),
    port_base: formToPort(val("port_base")),
    autostart: checked("autostart"),
  };
}

async function loadSettings() {
  try {
    fillForm(await invoke<Settings>("get_settings"));
    clearError();
  } catch (e) {
    showError(e);
  }
}

async function loadStatus() {
  try {
    renderStatus(await invoke<Status>("get_status"));
  } catch (e) {
    showError(e);
  }
}

async function loadHealth() {
  try {
    const msgs = await invoke<string[]>("get_health");
    $("health-list").replaceChildren(
      ...msgs.map((m) => {
        const li = document.createElement("li");
        li.textContent = m;
        return li;
      }),
    );
    $("health").hidden = msgs.length === 0;
  } catch (e) {
    showError(e);
  }
}

let saveTimer: number | undefined;

async function save(ev: Event) {
  ev.preventDefault();
  window.clearTimeout(saveTimer);
  saveBtn.disabled = true;
  saveMsg.className = "";
  saveMsg.textContent = "Saving…";
  try {
    await invoke("save_settings", { s: readForm() });
    saveMsg.className = "okmsg";
    saveMsg.textContent = "Saved";
    saveTimer = window.setTimeout(() => (saveMsg.textContent = ""), 2500);
    await loadSettings();
  } catch (e) {
    saveMsg.className = "err";
    saveMsg.textContent = text(e);
  } finally {
    saveBtn.disabled = false;
  }
}

async function init() {
  $("form").addEventListener("submit", save);
  pinRadios().forEach((r) => r.addEventListener("change", updatePinInput));
  $("recheck").addEventListener("click", loadHealth);
  toggle.addEventListener("click", async () => {
    toggle.disabled = true;
    try {
      await invoke(running ? "stop_server" : "start_server");
    } catch (e) {
      showError(e);
    } finally {
      toggle.disabled = false;
    }
  });
  try {
    await listen<Status>("status-changed", (e) => {
      renderStatus(e.payload);
      void loadHealth();
    });
  } catch (e) {
    showError(e);
  }
  await Promise.all([loadSettings(), loadStatus(), loadHealth()]);
}

void init();
