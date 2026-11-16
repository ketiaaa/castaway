//! Pre-flight and post-mortem health checks: ports, firewall rule, bundled
//! engine presence, and mapping of UxPlay error text to user-facing issues.

use std::io::Read;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde::Serialize;

/// Name of the Windows Firewall rule created by the installer (Task 8 must use this literal).
pub const FIREWALL_RULE_NAME: &str = "Castaway (UxPlay)";

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Shown for an error status that no specific issue explains.
pub const GENERIC_ERROR: &str =
    "Castaway's streaming engine stopped unexpectedly. Try Restart, or reinstall Castaway.";

#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum HealthIssue {
    PortInUse(u16),
    FirewallRuleMissing,
    GStreamerPluginMissing(String),
    UxplayMissing,
    PublicNetwork,
}

/// `<system_root>\<rel>`; `C:\Windows` when SystemRoot is unset or blank.
pub fn system_exe_in(system_root: Option<String>, rel: &str) -> PathBuf {
    let root = system_root
        .filter(|r| !r.trim().is_empty())
        .unwrap_or_else(|| r"C:\Windows".to_string());
    Path::new(&root).join(rel)
}

/// Full path of a Windows system executable (never resolved through PATH).
fn system_exe(rel: &str) -> PathBuf {
    system_exe_in(std::env::var("SystemRoot").ok(), rel)
}

/// Run `cmd` hidden with stdin null, capture stdout, and kill it after
/// `timeout`. Any failure (spawn, timeout, non-zero exit) yields None.
fn run_with_timeout(mut cmd: Command, timeout: Duration) -> Option<String> {
    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = cmd.spawn().ok()?;
    let mut out = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = out.read_to_end(&mut buf);
        buf
    });
    let end = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break Some(s),
            Ok(None) if Instant::now() < end => std::thread::sleep(Duration::from_millis(25)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
        }
    };
    let buf = reader.join().ok()?;
    status
        .filter(|s| s.success())
        .map(|_| String::from_utf8_lossy(&buf).into_owned())
}

/// One trimmed, non-empty line per active connection profile.
pub fn parse_network_categories(stdout: &str) -> Vec<String> {
    stdout
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect()
}

/// True when any active profile is "Public" (the enum name, not localized).
pub fn has_public_network(cats: &[String]) -> bool {
    cats.iter().any(|c| c.eq_ignore_ascii_case("Public"))
}

/// Network categories of the active connection profiles, via PowerShell's
/// Get-NetConnectionProfile. None when they could not be read.
pub fn network_categories() -> Option<Vec<String>> {
    let mut cmd = Command::new(system_exe(r"System32\WindowsPowerShell\v1.0\powershell.exe"));
    cmd.args([
        "-NoProfile",
        "-NonInteractive",
        "-Command",
        "Get-NetConnectionProfile | ForEach-Object { $_.NetworkCategory }",
    ]);
    run_with_timeout(cmd, Duration::from_secs(5)).map(|s| parse_network_categories(&s))
}

/// Check the port UxPlay really listens on for `-p base`: TCP `base+1` (verified
/// with this build: `base` and `base+2` stay unused, and there is no UDP
/// listener while idle). `PortInUse(base+1)` when it cannot be bound.
pub fn check_ports(base: u16) -> Vec<HealthIssue> {
    match base.checked_add(1) {
        Some(port) if TcpListener::bind(("0.0.0.0", port)).is_err() => {
            vec![HealthIssue::PortInUse(port)]
        }
        _ => Vec::new(),
    }
}

/// True when `netsh advfirewall firewall show rule name=<rule_name>` succeeds.
pub fn firewall_rule_exists(rule_name: &str) -> bool {
    let mut cmd = Command::new(system_exe(r"System32\netsh.exe"));
    cmd.args(["advfirewall", "firewall", "show", "rule"])
        .arg(format!("name={rule_name}"))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd.status().map(|s| s.success()).unwrap_or(false)
}

pub fn check_uxplay(path: &Path) -> Option<HealthIssue> {
    if path.is_file() {
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
    let mut from = 0;
    while let Some(i) = l[from..].find("port ") {
        let at = from + i;
        let whole_word = l[..at].chars().next_back().is_none_or(|c| !c.is_alphanumeric());
        if whole_word {
            let digits: String = l[at + 5..].chars().take_while(char::is_ascii_digit).collect();
            if let Some(p) = digits.parse().ok().filter(|p| *p != 0) {
                return Some(p);
            }
        }
        from = at + 5;
    }
    None
}

/// Map a UxPlay error line / `Status::Error` text to a health issue.
///
/// Note: the real Windows bind failure line (`*** ERROR: Error initialising
/// socket 10013`) carries no port number, so this returns `None` for it; use
/// [`classify_error_with_base`] when the configured base port is known.
///
/// Patterns are found anywhere in the text, so the crash-loop status
/// ("UxPlay keeps stopping: <diagnostic>") is classified too.
pub fn classify_error(text: &str) -> Option<HealthIssue> {
    const PLUGIN: &str = "Required gstreamer plugin ";
    if let Some(i) = text.find(PLUGIN) {
        return quoted(&text[i + PLUGIN.len()..])
            .map(|n| HealthIssue::GStreamerPluginMissing(n.to_string()));
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

/// The plain sentence shown to people for a `Status::Error` text: the mapped
/// issue's message, or a generic sentence (never raw log text).
pub fn error_sentence(text: &str, base: Option<u16>) -> String {
    classify_error_with_base(text, base)
        .map(|i| issue_message(&i))
        .unwrap_or_else(|| GENERIC_ERROR.to_string())
}

pub fn issue_message(i: &HealthIssue) -> String {
    match i {
        HealthIssue::PortInUse(p) => format!(
            "Another program is using network port {p}. Close it, or choose a different port in Castaway's settings."
        ),
        HealthIssue::FirewallRuleMissing => "Windows Firewall may be blocking Castaway, so your iPhone can't find this PC. Reinstall Castaway; if it still doesn't appear on your iPhone, open Windows Security > Firewall & network protection > Allow an app through firewall and allow Castaway.".to_string(),
        HealthIssue::PublicNetwork => "Your network is set to Public, so Windows blocks other devices from finding this PC. In Windows Settings, open Network & internet, choose your Wi-Fi network, and set Network profile type to Private.".to_string(),
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

    /// A base port whose base+1 is currently free.
    fn free_trio() -> u16 {
        for _ in 0..50 {
            let l = TcpListener::bind(("0.0.0.0", 0)).unwrap();
            let p = l.local_addr().unwrap().port();
            drop(l);
            if p <= 65534 && check_ports(p).is_empty() {
                return p;
            }
        }
        panic!("no free port found");
    }

    #[test]
    fn port_from_text_needs_a_whole_word() {
        assert_eq!(port_from_text("error on port 7001"), Some(7001));
        assert_eq!(port_from_text("Port 7001 busy"), Some(7001));
        assert_eq!(port_from_text("no support 7001 here"), None);
        assert_eq!(port_from_text("report 80 then port 7002"), Some(7002));
    }

    #[test]
    fn check_uxplay_needs_a_file_not_a_folder() {
        assert_eq!(check_uxplay(&std::env::temp_dir()), Some(HealthIssue::UxplayMissing));
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
    fn check_ports_reports_base_plus_one_held_by_a_listener() {
        for _ in 0..20 {
            let l = TcpListener::bind(("0.0.0.0", 0)).unwrap();
            let held = l.local_addr().unwrap().port();
            if held < 2 {
                continue;
            }
            assert_eq!(check_ports(held - 1), vec![HealthIssue::PortInUse(held)]);
            return;
        }
        panic!("could not pick a port");
    }

    #[test]
    fn check_ports_ignores_a_program_on_base_alone() {
        // UxPlay never listens on `base`, so a holder there is no problem.
        // Retry: another test may briefly hold base+1.
        for _ in 0..50 {
            let l = TcpListener::bind(("0.0.0.0", 0)).unwrap();
            let base = l.local_addr().unwrap().port();
            if base < u16::MAX && check_ports(base).is_empty() {
                return; // `base` was held by `l` for the whole check
            }
        }
        panic!("check_ports kept flagging a held base port");
    }

    #[test]
    fn issue_message_is_plain_language() {
        let all = [
            HealthIssue::PortInUse(7401),
            HealthIssue::FirewallRuleMissing,
            HealthIssue::GStreamerPluginMissing("libav".to_string()),
            HealthIssue::UxplayMissing,
            HealthIssue::PublicNetwork,
        ];
        let generic = [GENERIC_ERROR.to_string()];
        for m in all.iter().map(issue_message).chain(generic) {
            assert!(!m.is_empty());
            for bad in ["MSYS2", "GStreamer", "WSA", "uxplay", "10013", "\\"] {
                assert!(!m.contains(bad), "{m:?} contains {bad:?}");
            }
        }
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

    /// The supervisor's final crash-loop status for the missing-plugin log:
    /// prefix + the first Error line of the launch.
    fn crash_status_from_fixture() -> String {
        let first = MISSING_PLUGIN
            .lines()
            .filter_map(parse_line)
            .find_map(|e| match e {
                UxEvent::Error(t) => Some(t),
                _ => None,
            })
            .unwrap();
        format!("UxPlay keeps stopping: {first}")
    }

    #[test]
    fn classify_error_finds_plugin_after_crash_prefix() {
        let text = crash_status_from_fixture();
        assert_eq!(text, "UxPlay keeps stopping: Required gstreamer plugin 'app' not found");
        assert_eq!(
            classify_error(&text),
            Some(HealthIssue::GStreamerPluginMissing("app".to_string()))
        );
        assert_eq!(
            classify_error_with_base(&text, Some(7000)),
            Some(HealthIssue::GStreamerPluginMissing("app".to_string()))
        );
        assert_eq!(classify_error("UxPlay keeps stopping: *** ERROR: stopping"), None);
    }

    #[test]
    fn error_sentence_is_plain() {
        assert_eq!(
            error_sentence(&crash_status_from_fixture(), None),
            issue_message(&HealthIssue::GStreamerPluginMissing("app".to_string()))
        );
        assert_eq!(
            error_sentence("*** ERROR: Error initialising socket 10013", Some(7000)),
            issue_message(&HealthIssue::PortInUse(7001))
        );
        assert_eq!(error_sentence("UxPlay keeps stopping: *** ERROR: fake failure", None), GENERIC_ERROR);
        assert_eq!(
            GENERIC_ERROR,
            "Castaway's streaming engine stopped unexpectedly. Try Restart, or reinstall Castaway."
        );
    }

    #[test]
    fn parse_network_categories_exact() {
        assert_eq!(parse_network_categories("Public\r\nPrivate\r\n"), vec!["Public", "Private"]);
        assert_eq!(parse_network_categories(""), Vec::<String>::new());
        assert_eq!(parse_network_categories("\r\n  \r\n"), Vec::<String>::new());
        assert_eq!(parse_network_categories("  DomainAuthenticated  \n"), vec!["DomainAuthenticated"]);
    }

    #[test]
    fn has_public_network_cases() {
        let v = |s: &[&str]| s.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert!(has_public_network(&v(&["Public"])));
        assert!(has_public_network(&v(&["Private", "Public"])));
        assert!(has_public_network(&parse_network_categories("Private\r\nPublic\r\n")));
        assert!(!has_public_network(&v(&["Private"])));
        assert!(!has_public_network(&v(&["DomainAuthenticated", "Private"])));
        assert!(!has_public_network(&v(&[])));
        assert!(!has_public_network(&v(&["Öffentlich", "Publique", "Public network"])));
    }

    #[test]
    fn public_network_and_firewall_messages() {
        assert_eq!(
            issue_message(&HealthIssue::PublicNetwork),
            "Your network is set to Public, so Windows blocks other devices from finding this PC. In Windows Settings, open Network & internet, choose your Wi-Fi network, and set Network profile type to Private."
        );
        let fw = issue_message(&HealthIssue::FirewallRuleMissing);
        assert!(fw.contains("Reinstall Castaway"), "{fw}");
        assert!(fw.contains("Allow an app through firewall"), "{fw}");
    }

    #[test]
    fn system_exe_path_uses_system_root() {
        assert_eq!(
            system_exe_in(Some(r"D:\Win".to_string()), r"System32\netsh.exe"),
            Path::new(r"D:\Win").join(r"System32\netsh.exe")
        );
        assert_eq!(
            system_exe_in(None, r"System32\netsh.exe"),
            Path::new(r"C:\Windows").join(r"System32\netsh.exe")
        );
        assert_eq!(
            system_exe_in(Some("  ".to_string()), "x.exe"),
            Path::new(r"C:\Windows").join("x.exe")
        );
    }

    #[test]
    fn network_categories_runs_or_returns_none() {
        // Real probe: must finish within its timeout and never panic.
        let start = std::time::Instant::now();
        let r = network_categories();
        assert!(start.elapsed() < std::time::Duration::from_secs(8));
        if let Some(c) = r {
            assert!(c.iter().all(|s| !s.trim().is_empty()));
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
