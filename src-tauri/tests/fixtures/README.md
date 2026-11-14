# Fixtures for log_parser tests

## SYNTHESIZED FROM SOURCE, NOT CAPTURED FROM A DEVICE

`connect.log` and `disconnect.log` were NOT captured from a real device (none was available).
They are built from the log strings in UxPlay source at commit
05b0275cb1b726592700e47a6e0307544b3d3a52 (shallow clone of https://github.com/FDH2/UxPlay):

- `uxplay.cpp` `report_client_request()`: `LOGI("connection request from %s (%s) with deviceID = %s\n", name, model, deviceid)`
  (name = the client's device name, model e.g. iPhone14,2). LOGI is the default log level, so `-d` is NOT required.
  Printed on every connection request, including ones that are then denied (restrict/block lists).
- `lib/httpd.c`: `LOGGER_INFO "Accepted %s client on socket %d, port %u"` (TCP accept, one per socket)
  and `LOGGER_INFO "Connection closed on socket %d"` (one per socket; a client opens several).
  Also `uxplay.cpp`: `LOGI("*** ERROR lost connection with client (network problem?)")` (conn_reset / feedback timeout).
- The per-client-session `conn_init`/`conn_destroy` messages ("Open connections: %i") are LOGD only
  (need `-d`) and are therefore not used.
- Sample values (device name "Test iPhone", deviceID, socket numbers) are invented.

Re-verify against a real device in Task 9.

## Captured fixtures (real runs of the bundled uxplay.exe, Windows 11)

Common: `uxplay.exe -d -n TestPC` (plus extras below), stdout+stderr merged (`> file 2>&1`), run via cmd.exe,
killed after ~8-10 s. Env for all runs (UXDIR = F:\Airplay\src-tauri\resources\uxplay):

    GST_PLUGIN_PATH=%UXDIR%\gst-plugins
    GST_PLUGIN_SYSTEM_PATH=%UXDIR%\gst-plugins
    GST_REGISTRY=<temp file>.bin
    GST_REGISTRY_FORK=no

- `startup.log`: command exactly as above. Ready line = `Initialized server socket(s)` (printed by httpd after bind+listen;
  `register_dnssd: advertised ...` follows).
- `crash_missing_plugin.log`: same command, but `GST_PLUGIN_PATH` and `GST_PLUGIN_SYSTEM_PATH` pointed at an EMPTY directory,
  so every required plugin is missing. uxplay exits after `*** ERROR: stopping`.
- `port_in_use.log`: `uxplay.exe -d -n TestPC -p 7400` while another process held TCP 7401 with
  `TcpListener` (IPv6Any, DualMode, `ExclusiveAddressUse = true`). uxplay's httpd listens on base+1.
  Note: without the exclusive hold (plain listener, or a second uxplay instance on the same -p) Windows let both bind
  (SO_REUSEADDR semantics), so no error occurred. uxplay does NOT exit here: it logs the error, still advertises,
  and never prints `Initialized server socket(s)`.
