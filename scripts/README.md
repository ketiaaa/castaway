# scripts

## build-uxplay.ps1 / build-uxplay.sh

Builds the pinned UxPlay commit (`$UxPlayRef` in `build-uxplay.ps1`, currently master commit
`05b0275cb1b726592700e47a6e0307544b3d3a52`, pre-1.74) under MSYS2 UCRT64, then
vendors `uxplay.exe`, the non-system DLLs it needs (found by walking `objdump -p` imports),
and a curated set of GStreamer plugins into `src-tauri/resources/uxplay/`
(`gst-plugins/` subfolder). Only `VERSION.txt` there is committed.

    powershell -ExecutionPolicy Bypass -File scripts/build-uxplay.ps1

### Pinned version and Bonjour

Release v1.73.7 (newest tag) needs the Bonjour SDK on Windows; there is no `USE_DNS_SD`
option in that tag. Pre-1.74 master uses the built-in `lib/mdnsd` responder by default and
only opts into Bonjour with `-DUSE_DNS_SD=1`, so we pin master commit
`05b0275cb1b726592700e47a6e0307544b3d3a52` (recorded in `src-tauri/resources/uxplay/VERSION.txt`).
Built without USE_DNS_SD and with `-DNO_MARCH_NATIVE=ON` (portable binary). Verified:
`objdump -p uxplay.exe` shows no `dnssd.dll` import, so Bonjour is irrelevant at runtime.

### Flags (verified against `uxplay.exe -h`)

All of `-n -nh -s -fps -vs -as -p -pin -reg -fs -d` exist. Notes for later tasks:
- `-pin` takes an optional *separate* 4-digit value (`-pin 1234`; range 0001-9999); bare `-pin` = random pin.
- `-p n` uses three consecutive TCP and UDP ports n, n+1, n+2 (range 1024-65535); bare `-p` = legacy ports.
- `-reg [fn]` optional file; default `$HOME/.uxplay.register`.
- `-vs` / `-as` take one sink string (e.g. `-vs d3d11videosink`).

### GStreamer environment for running the vendored uxplay.exe

Plugins live in `<uxplay dir>\gst-plugins` (not a GStreamer-relative `lib\gstreamer-1.0`), so the
app must set these when spawning uxplay.exe (verified: uxplay starts, builds its audio/video
pipelines and advertises, from a path with a space and with MSYS2 off PATH):

    GST_PLUGIN_PATH=<uxplay dir>\gst-plugins
    GST_PLUGIN_SYSTEM_PATH=<uxplay dir>\gst-plugins
    GST_REGISTRY=<app data dir>\gst-registry.bin     (writable location)
    GST_REGISTRY_FORK=no                               (no gst-plugin-scanner helper is bundled)

Plugin set was found by running uxplay: `level` (audio pipeline) and others were initially
missing; the list in `build-uxplay.sh` now covers audio, h264 video (decodebin/libav, d3d11/d3d12/gl sinks),
overlays, and DirectSound/WASAPI. HLS (`-hls`) plugins are not bundled.
