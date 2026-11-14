//! Parser for UxPlay's stdout/stderr lines. Pure: no I/O.

#[derive(Debug, Clone, PartialEq)]
pub enum UxEvent {
    Listening,
    ClientConnected { device: Option<String> },
    ClientDisconnected,
    Error(String),
}

/// Remove ANSI CSI escape sequences (e.g. colour codes) from `s`.
fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' && chars.peek() == Some(&'[') {
            chars.next();
            for n in chars.by_ref() {
                if ('\u{40}'..='\u{7e}').contains(&n) {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Classify one line of UxPlay output (stdout or stderr).
///
/// Patterns come from the captured fixtures (startup, crash_missing_plugin,
/// port_in_use) and, for connect/disconnect, from UxPlay source at the pinned
/// commit (see tests/fixtures/README.md). All of them are LOGI/LOGE level, so
/// `-d` is not required.
pub fn parse_line(line: &str) -> Option<UxEvent> {
    let clean = strip_ansi(line);
    let t = clean.trim();

    // httpd prints this once bind+listen succeeded (absent in port_in_use.log).
    if t == "Initialized server socket(s)" {
        return Some(UxEvent::Listening);
    }

    // `connection request from <name> (<model>) with deviceID = <id>`
    if let Some(rest) = t.strip_prefix("connection request from ") {
        let device = rest
            .rfind(" with deviceID = ")
            .map(|i| &rest[..i])
            .and_then(|head| head.rfind(" (").map(|i| head[..i].trim()))
            .filter(|n| !n.is_empty())
            .map(str::to_string);
        return Some(UxEvent::ClientConnected { device });
    }

    // Per-socket close (httpd) or lost client (conn_reset / feedback timeout).
    // Checked before the generic error rule: "*** ERROR lost connection ..."
    // is a LOGI line, not a fatal error.
    if t.starts_with("Connection closed on socket ") || t.contains("lost connection with client") {
        return Some(UxEvent::ClientDisconnected);
    }

    // LOGE lines ("*** ERROR: ...", incl. bind failure and "stopping") and the
    // missing-GStreamer-plugin report.
    if t.starts_with("*** ERROR:") || t.starts_with("Required gstreamer plugin ") {
        return Some(UxEvent::Error(t.to_string()));
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const STARTUP: &str = include_str!("../tests/fixtures/startup.log");
    const MISSING_PLUGIN: &str = include_str!("../tests/fixtures/crash_missing_plugin.log");
    const PORT_IN_USE: &str = include_str!("../tests/fixtures/port_in_use.log");
    const CONNECT: &str = include_str!("../tests/fixtures/connect.log");
    const DISCONNECT: &str = include_str!("../tests/fixtures/disconnect.log");

    fn events(log: &str) -> Vec<UxEvent> {
        log.lines().filter_map(parse_line).collect()
    }

    #[test]
    fn startup_yields_exactly_one_listening() {
        assert_eq!(events(STARTUP), vec![UxEvent::Listening]);
    }

    #[test]
    fn missing_plugin_yields_errors_with_original_text() {
        let ev = events(MISSING_PLUGIN);
        assert!(!ev.contains(&UxEvent::Listening));
        assert!(ev.contains(&UxEvent::Error(
            "Required gstreamer plugin 'app' not found".to_string()
        )));
        assert!(ev.contains(&UxEvent::Error("*** ERROR: stopping".to_string())));
        assert_eq!(ev.len(), 6); // 5 missing plugins + stopping
    }

    #[test]
    fn port_in_use_yields_bind_error_and_no_listening() {
        let ev = events(PORT_IN_USE);
        assert_eq!(
            ev,
            vec![UxEvent::Error(
                "*** ERROR: Error initialising socket 10013".to_string()
            )]
        );
    }

    // UNVERIFIED against a real device; re-verify in Task 9
    #[test]
    fn connect_extracts_device_name() {
        let ev = events(CONNECT);
        assert_eq!(
            ev,
            vec![UxEvent::ClientConnected {
                device: Some("Test iPhone".to_string())
            }]
        );
    }

    // UNVERIFIED against a real device; re-verify in Task 9
    #[test]
    fn disconnect_line_yields_disconnected() {
        assert_eq!(events(DISCONNECT), vec![UxEvent::ClientDisconnected]);
        assert_eq!(
            parse_line("*** ERROR lost connection with client (network problem?)"),
            Some(UxEvent::ClientDisconnected)
        );
    }

    // UNVERIFIED against a real device; re-verify in Task 9
    #[test]
    fn connect_with_empty_name_has_no_device() {
        assert_eq!(
            parse_line("connection request from  (iPad1,1) with deviceID = aa:bb"),
            Some(UxEvent::ClientConnected { device: None })
        );
    }

    #[test]
    fn unrelated_lines_return_none() {
        for l in [
            "",
            "   ",
            "UxPlay 1.74: An Open-Source AirPlay mirroring and audio-streaming server.",
            "using system MAC address 0a:00:27:00:00:0b",
            "GStreamer h264 bus message pipeline2 state-changed NULL READY",
            "register_dnssd: advertised AirPlay service with \"Features\" code = 0x527FFEE6",
            "Accepted IPv4 client on socket 1234, port 7001",
            "Client identified as User-Agent: AirPlay/690.7.1",
        ] {
            assert_eq!(parse_line(l), None, "line: {l:?}");
        }
    }

    #[test]
    fn robust_to_line_endings_and_unicode() {
        assert_eq!(
            parse_line("Initialized server socket(s)\r\n"),
            Some(UxEvent::Listening)
        );
        assert_eq!(
            parse_line("  *** ERROR: Error initialising socket 10013\r"),
            Some(UxEvent::Error(
                "*** ERROR: Error initialising socket 10013".to_string()
            ))
        );
        assert_eq!(
            parse_line("connection request from Ketia\u{2019}s iPhone \u{1F4F1} (iPhone15,3) with deviceID = aa:bb\r\n"),
            Some(UxEvent::ClientConnected {
                device: Some("Ketia\u{2019}s iPhone \u{1F4F1}".to_string())
            })
        );
        assert_eq!(parse_line("\u{1b}[32mhello\u{1b}[0m \u{4f60}\u{597d}"), None);
        assert_eq!(
            parse_line("\u{1b}[31mInitialized server socket(s)\u{1b}[0m"),
            Some(UxEvent::Listening)
        );
    }
}
