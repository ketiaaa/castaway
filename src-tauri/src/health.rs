//! Pre-flight and post-mortem health checks: ports, firewall rule, bundled
//! engine presence, and mapping of UxPlay error text to user-facing issues.

use std::net::{TcpListener, UdpSocket};
use std::path::Path;
use std::process::{Command, Stdio};

use serde::Serialize;

/// Name of the Windows Firewall rule created by the installer (Task 8 must use this literal).
pub const FIREWALL_RULE_NAME: &str = "Castaway (UxPlay)";

#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum HealthIssue {
    PortInUse(u16),
    FirewallRuleMissing,
    GStreamerPluginMissing(String),
    UxplayMissing,
}

/// Check `base`, `base+1`, `base+2` for TCP and UDP by binding on 0.0.0.0.
/// One `PortInUse` per port that cannot be bound. Skip when ports are random.
pub fn check_ports(base: u16) -> Vec<HealthIssue> {
    let mut issues = Vec::new();
    for offset in 0..3u16 {
        let Some(port) = base.checked_add(offset) else {
            break;
        };
        let tcp_ok = TcpListener::bind(("0.0.0.0", port)).is_ok();
        let udp_ok = UdpSocket::bind(("0.0.0.0", port)).is_ok();
        if !(tcp_ok && udp_ok) {
            issues.push(HealthIssue::PortInUse(port));
        }
    }
    issues
}

/// True when `netsh advfirewall firewall show rule name=<rule_name>` succeeds.
pub fn firewall_rule_exists(rule_name: &str) -> bool {
    let mut cmd = Command::new("netsh");
    cmd.args(["advfirewall", "firewall", "show", "rule"])
        .arg(format!("name={rule_name}"))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    cmd.status().map(|s| s.success()).unwrap_or(false)
}

pub fn check_uxplay(path: &Path) -> Option<HealthIssue> {
    if path.exists() {
        None
    } else {
        Some(HealthIssue::UxplayMissing)
    }
}

/// Return the text between the first pair of `'` quotes, if any.
fn quoted(text: &str) -> Option<&str> {
    let start = text.find('\'')? + 1;
    let len = text[start..].find('\'')?;
    let name = &text[start..start + len];
    (!name.is_empty()).then_some(name)
}

fn is_bind_failure(text: &str) -> bool {
    let l = text.to_ascii_lowercase();
    l.contains("initialising socket")
        || l.contains("initializing socket")
        || l.contains("address already in use")
}

/// Extract a port written as "port <N>" in the text.
fn port_from_text(text: &str) -> Option<u16> {
    let l = text.to_ascii_lowercase();
    let idx = l.find("port ")? + "port ".len();
    let digits: String = l[idx..].chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok().filter(|p| *p != 0)
}

/// Map a UxPlay error line / `Status::Error` text to a health issue.
///
/// Note: the real Windows bind failure line (`*** ERROR: Error initialising
/// socket 10013`) carries no port number, so this returns `None` for it; use
/// [`classify_error_with_base`] when the configured base port is known.
pub fn classify_error(text: &str) -> Option<HealthIssue> {
    if let Some(rest) = text.trim().strip_prefix("Required gstreamer plugin ") {
        return quoted(rest).map(|n| HealthIssue::GStreamerPluginMissing(n.to_string()));
    }
    if is_bind_failure(text) {
        return port_from_text(text).map(HealthIssue::PortInUse);
    }
    None
}

/// Like [`classify_error`], but a bind failure without a port in its text is
/// attributed to `base + 1` (UxPlay's HTTP listener, where it fails in practice).
pub fn classify_error_with_base(text: &str, base: Option<u16>) -> Option<HealthIssue> {
    classify_error(text).or_else(|| {
        if is_bind_failure(text) {
            base.and_then(|b| b.checked_add(1)).map(HealthIssue::PortInUse)
        } else {
            None
        }
    })
}

pub fn issue_message(i: &HealthIssue) -> String {
    match i {
        HealthIssue::PortInUse(p) => format!(
            "Another program is using network port {p}. Close it, or choose a different port in Castaway's settings."
        ),
        HealthIssue::FirewallRuleMissing => "Windows Firewall may be blocking Castaway, so your iPhone can't find this PC. Reinstall Castaway or allow it through the firewall.".to_string(),
        HealthIssue::GStreamerPluginMissing(name) => {
            format!("A video component ({name}) is missing. Reinstall Castaway.")
        }
        HealthIssue::UxplayMissing => {
            "Castaway's streaming engine is missing. Reinstall Castaway.".to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::log_parser::{UxEvent, parse_line};

    const MISSING_PLUGIN: &str = include_str!("../tests/fixtures/crash_missing_plugin.log");
    const PORT_IN_USE: &str = include_str!("../tests/fixtures/port_in_use.log");

    /// A base port whose base..base+2 are all currently free.
    fn free_trio() -> u16 {
        for _ in 0..50 {
            let l = TcpListener::bind(("0.0.0.0", 0)).unwrap();
            let p = l.local_addr().unwrap().port();
            drop(l);
            if p <= 65532 && check_ports(p).is_empty() {
                return p;
            }
        }
        panic!("no free port trio found");
    }

    #[test]
    fn check_ports_ok_when_free() {
        for _ in 0..5 {
            let base = free_trio();
            if check_ports(base).is_empty() {
                return;
            }
        }
        panic!("ports never reported free");
    }

    #[test]
    fn check_ports_reports_a_port_held_by_a_listener() {
        for _ in 0..20 {
            let l = TcpListener::bind(("0.0.0.0", 0)).unwrap();
            let p = l.local_addr().unwrap().port();
            if p > 65532 {
                continue;
            }
            let issues = check_ports(p);
            assert!(issues.contains(&HealthIssue::PortInUse(p)), "{issues:?}");
            assert_eq!(
                issues.iter().filter(|i| **i == HealthIssue::PortInUse(p)).count(),
                1
            );
            return;
        }
        panic!("could not pick a port");
    }

    #[test]
    fn check_ports_reports_udp_holder() {
        let s = UdpSocket::bind(("0.0.0.0", 0)).unwrap();
        let p = s.local_addr().unwrap().port();
        if p <= 65532 {
            assert!(check_ports(p).contains(&HealthIssue::PortInUse(p)));
        }
    }

    #[test]
    fn issue_message_is_plain_language() {
        let all = [
            HealthIssue::PortInUse(7401),
            HealthIssue::FirewallRuleMissing,
            HealthIssue::GStreamerPluginMissing("libav".to_string()),
            HealthIssue::UxplayMissing,
        ];
        for i in &all {
            let m = issue_message(i);
            assert!(!m.is_empty());
            for bad in ["MSYS2", "GStreamer", "WSA", "uxplay", "10013", "\\"] {
                assert!(!m.contains(bad), "{m:?} contains {bad:?}");
            }
        }
        assert!(issue_message(&all[0]).contains("7401"));
        assert!(issue_message(&all[2]).contains("libav"));
    }

    #[test]
    fn classify_error_maps_fixture_lines() {
        let texts = |log: &'static str| {
            log.lines()
                .filter_map(parse_line)
                .filter_map(|e| match e {
                    UxEvent::Error(t) => Some(t),
                    _ => None,
                })
                .collect::<Vec<_>>()
        };
        let plugin_issues: Vec<_> = texts(MISSING_PLUGIN)
            .iter()
            .filter_map(|t| classify_error(t))
            .collect();
        let names = ["app", "libav", "playback", "autodetect", "videoparsersbad"];
        let expected: Vec<_> = names
            .iter()
            .map(|n| HealthIssue::GStreamerPluginMissing(n.to_string()))
            .collect();
        assert_eq!(plugin_issues, expected);
        // "*** ERROR: stopping" is not itself an issue.
        assert_eq!(classify_error("*** ERROR: stopping"), None);

        let port_texts = texts(PORT_IN_USE);
        assert_eq!(port_texts.len(), 1);
        // The real line carries no port: only the base-aware variant can attribute it.
        assert_eq!(classify_error(&port_texts[0]), None);
        assert_eq!(
            classify_error_with_base(&port_texts[0], Some(7400)),
            Some(HealthIssue::PortInUse(7401))
        );
        assert_eq!(classify_error_with_base(&port_texts[0], None), None);
        // A text that does contain a port is mapped directly.
        assert_eq!(
            classify_error("*** ERROR: Error initialising socket on port 7401"),
            Some(HealthIssue::PortInUse(7401))
        );
    }

    #[test]
    fn classify_error_ignores_unrelated_text() {
        for t in [
            "",
            "Connection closed on socket 5",
            "*** ERROR: lost connection with client",
            "using network ports UDP 7400 7401 7402 TCP 7400 7401 7402",
        ] {
            assert_eq!(classify_error(t), None, "{t:?}");
            assert_eq!(classify_error_with_base(t, Some(7400)), None, "{t:?}");
        }
    }

    #[test]
    fn check_uxplay_reports_missing_file() {
        let p = std::env::temp_dir().join("castaway-definitely-missing-uxplay.exe");
        assert_eq!(check_uxplay(&p), Some(HealthIssue::UxplayMissing));
    }

    #[test]
    fn check_uxplay_ok_for_existing_file() {
        let p = std::env::temp_dir().join(format!("castaway-health-{}.exe", std::process::id()));
        std::fs::write(&p, b"x").unwrap();
        let r = check_uxplay(&p);
        let _ = std::fs::remove_file(&p);
        assert_eq!(r, None);
    }

    #[test]
    fn firewall_rule_exists_false_for_nonexistent_rule() {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let name = format!("Castaway nonexistent rule {nanos}");
        assert!(!firewall_rule_exists(&name));
    }
}
