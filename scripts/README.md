# scripts

## build-uxplay.ps1 / build-uxplay.sh

Builds the pinned UxPlay tag (`$UxPlayTag` in `build-uxplay.ps1`, currently `v1.73.7`, the
newest tag on https://github.com/FDH2/UxPlay as of 2026-11-15) under MSYS2 UCRT64, then
vendors `uxplay.exe`, the non-system DLLs it needs (found by walking `objdump -p` imports),
and a curated set of GStreamer plugins into `src-tauri/resources/uxplay/`
(`gst-plugins/` subfolder). Only `VERSION.txt` there is committed.

    powershell -ExecutionPolicy Bypass -File scripts/build-uxplay.ps1

### OPEN ISSUE: UxPlay on Windows requires Bonjour (conflicts with Global Constraints)

The constraint "build WITHOUT `-DUSE_DNS_SD=1` (no Bonjour)" cannot be satisfied with
v1.73.x. There is no `USE_DNS_SD` CMake option in this version. `lib/CMakeLists.txt`
(lines ~144-185) on `WIN32` always requires `dns_sd.h` and `Lib/x64/dnssd.lib` from the
Bonjour SDK (`BONJOUR_SDK_HOME`, default `C:\Program Files\Bonjour SDK`); configure
fails with `dns_sd.h not found` otherwise (verified). At runtime uxplay.exe imports
`dnssd.dll`, which on Windows is Apple's Bonjour client library; the Bonjour Service
(`mDNSResponder.exe`) must be installed and running for advertisement to work. The README
offers a Bluetooth LE beacon as an alternative *discovery* method, but the build still
needs the Bonjour SDK. The script therefore stops early with a clear error until a
decision is made (see the task report for options).

### GStreamer environment for running the vendored uxplay.exe (to be applied by the app)

The plugins live in `<uxplay dir>\gst-plugins`, not in a GStreamer-relative `lib\gstreamer-1.0`,
so the app must set, when spawning uxplay.exe (planned, not yet verified because the build is blocked):

    GST_PLUGIN_PATH=<uxplay dir>\gst-plugins
    GST_PLUGIN_SYSTEM_PATH=<uxplay dir>\gst-plugins
    GST_REGISTRY=<app data>\gst-registry.bin
    GST_REGISTRY_FORK=no
