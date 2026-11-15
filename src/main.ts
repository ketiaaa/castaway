import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
  type PinFormMode,
  type Settings,
  type Status,
  type ToggleAction,
  formToPin,
  formToPort,
  pinToForm,
  portToForm,
  registryAllowed,
  statusView,
  toggleAction,
  toggleLabel,
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

let action: ToggleAction = "start";
let renderSeq = 0;

function paintStatus(status: Status, plain?: string) {
  const v = statusView(status, plain);
  banner.className = `banner ${v.tone}`;
  $("banner-label").textContent = v.label;
  const d = $("banner-detail");
  d.textContent = v.detail ?? "";
  d.hidden = !v.detail;
  const raw = $("banner-raw");
  raw.textContent = v.raw ? `Details: ${v.raw}` : "";
  raw.hidden = !v.raw;
  action = toggleAction(status);
  toggle.textContent = toggleLabel(action);
  toggle.disabled = false;
}

async function renderStatus(status: Status) {
  const seq = ++renderSeq;
  paintStatus(status);
  if (typeof status === "object" && "Error" in status) {
    try {
      const plain = await invoke<string>("describe_error", { msg: status.Error });
      if (seq === renderSeq) paintStatus(status, plain);
    } catch {
      // Keep the generic sentence.
    }
  }
}

function pinMode(): PinFormMode {
  return (pinRadios().find((r) => r.checked)?.value ?? "off") as PinFormMode;
}

function updatePinInput() {
  const mode = pinMode();
  pinFixed.hidden = mode !== "fixed";
  const allowed = registryAllowed(mode);
  $<HTMLInputElement>("use_registry").disabled = !allowed;
  $("registry-help").hidden = allowed;
}

function fillForm(s: Settings) {
  setVal("name", s.name);
  setVal("width", String(s.width));
  setVal("height", String(s.height));
  setVal("fps", String(s.fps));
  setChecked("fullscreen", s.fullscreen);
  setChecked("low_latency", s.low_latency);
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
  const mode = pinMode();
  return {
    name: val("name"),
    width: Number(val("width")),
    height: Number(val("height")),
    fps: Number(val("fps")),
    fullscreen: checked("fullscreen"),
    low_latency: checked("low_latency"),
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
    await renderStatus(await invoke<Status>("get_status"));
  } catch (e) {
    showError(e);
  }
}

async function loadHealth(force = false) {
  try {
    const msgs = await invoke<string[]>("get_health", { force });
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

interface UpdateInfo {
  version: string;
  notes: string | null;
}

const updateMsg = $("update-msg");
const checkBtn = $<HTMLButtonElement>("check-update");
const installBtn = $<HTMLButtonElement>("install-update");

function say(msg: string, cls = "") {
  updateMsg.className = cls;
  updateMsg.textContent = msg;
}

async function checkUpdate(manual: boolean) {
  checkBtn.disabled = true;
  if (manual) say("Checking…");
  try {
    const u = await invoke<UpdateInfo | null>("check_update");
    if (u) {
      installBtn.hidden = false;
      say(`Version ${u.version} is available.`, "okmsg");
    } else {
      installBtn.hidden = true;
      if (manual) say("You're up to date.", "okmsg");
    }
  } catch (e) {
    if (manual) say(text(e), "err");
  } finally {
    checkBtn.disabled = false;
  }
}

async function installUpdate() {
  installBtn.disabled = true;
  checkBtn.disabled = true;
  say("Downloading and installing… Castaway will restart.");
  try {
    await invoke("install_update");
  } catch (e) {
    say(text(e), "err");
    installBtn.disabled = false;
    checkBtn.disabled = false;
  }
}

async function init() {
  checkBtn.addEventListener("click", () => void checkUpdate(true));
  installBtn.addEventListener("click", () => void installUpdate());
  try {
    $("version").textContent = `Version ${await invoke<string>("app_version")}`;
  } catch {
    // Version label is cosmetic.
  }
  void checkUpdate(false);
  $("form").addEventListener("submit", save);
  pinRadios().forEach((r) => r.addEventListener("change", updatePinInput));
  $("recheck").addEventListener("click", () => void loadHealth(true));
  toggle.addEventListener("click", async () => {
    toggle.disabled = true;
    try {
      await invoke(`${action}_server`);
    } catch (e) {
      showError(e);
    } finally {
      toggle.disabled = false;
    }
  });
  try {
    await listen<Status>("status-changed", (e) => {
      void renderStatus(e.payload);
      void loadHealth();
    });
  } catch (e) {
    showError(e);
  }
  await Promise.all([loadSettings(), loadStatus(), loadHealth()]);
}

void init();
