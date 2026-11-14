//! Castaway settings model, validation, persistence and UxPlay argv builder.
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tauri::Manager;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PinMode {
    Off,
    Random,
    Fixed(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub name: String,
    pub hide_hostname: bool,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub fullscreen: bool,
    pub video_sink: String,
    pub audio_sink: String,
    pub pin: PinMode,
    pub use_registry: bool,
    pub port_base: Option<u16>,
    pub autostart: bool,
}

/// UxPlay (a MinGW build without a UTF-8 manifest) receives argv in the ANSI
/// code page, so only printable ASCII survives. Removes everything else from a
/// host name and trims it; "PC" when nothing is left.
pub fn ascii_hostname(raw: Option<&str>) -> String {
    let kept: String = raw
        .unwrap_or_default()
        .chars()
        .filter(|c| c.is_ascii_graphic() || *c == ' ')
        .collect();
    let t = kept.trim();
    if t.is_empty() { "PC".to_string() } else { t.to_string() }
}

/// This PC's name as UxPlay will see it (ASCII-filtered COMPUTERNAME).
pub fn hostname() -> String {
    ascii_hostname(std::env::var("COMPUTERNAME").ok().as_deref())
}

fn default_name_for(host: Option<&str>) -> String {
    format!("Castaway on {}", ascii_hostname(host))
}

fn default_name() -> String {
    default_name_for(std::env::var("COMPUTERNAME").ok().as_deref())
}

/// mDNS labels are at most 63 bytes. UxPlay appends "@<hostname>" unless
/// `-nh` is set, and the AirPlay audio service name adds a 12-hex MAC and "@".
const MDNS_LABEL_MAX: usize = 63;
const RAOP_PREFIX: usize = 13;

/// How many characters of device name fit, given the hostname suffix.
pub fn name_budget(hide_hostname: bool, host_len: usize) -> usize {
    let suffix = if hide_hostname { 0 } else { 1 + host_len };
    MDNS_LABEL_MAX.saturating_sub(RAOP_PREFIX + suffix)
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            name: default_name(),
            hide_hostname: true,
            width: 1920,
            height: 1080,
            fps: 30,
            fullscreen: false,
            video_sink: "d3d11videosink".into(),
            audio_sink: "directsoundsink".into(),
            pin: PinMode::Off,
            use_registry: false,
            port_base: None,
            autostart: false,
        }
    }
}

impl Settings {
    /// Returns a short plain-language message for the first invalid field.
    pub fn validate(&self) -> Result<(), String> {
        self.validate_for_host(&hostname())
    }

    /// [`Settings::validate`] with an explicit (ASCII) hostname.
    pub fn validate_for_host(&self, host: &str) -> Result<(), String> {
        let n = self.name.trim();
        if n.is_empty() {
            return Err("The name must not be empty.".into());
        }
        if !self.name.chars().all(|c| c.is_ascii_graphic() || c == ' ') {
            return Err("Use plain letters, numbers, spaces and basic punctuation in the device name \u{2014} accented letters and emoji aren't supported yet.".into());
        }
        if n.starts_with('-') {
            return Err("The device name can't start with a dash (-).".into());
        }
        // ASCII only from here on, so bytes == characters.
        let budget = name_budget(self.hide_hostname, host.len());
        if self.name.len() > budget {
            return Err(if self.hide_hostname {
                format!("The device name is too long. Use at most {budget} characters.")
            } else {
                format!("The device name is too long. Use at most {budget} characters (or turn on \"Don't add the computer name\" for more).")
            });
        }
        if !(320..=7680).contains(&self.width) {
            return Err("The width must be between 320 and 7680.".into());
        }
        if !(320..=7680).contains(&self.height) {
            return Err("The height must be between 320 and 7680.".into());
        }
        if !(1..=60).contains(&self.fps) {
            return Err("The fps must be between 1 and 60.".into());
        }
        if let Some(p) = self.port_base {
            if !(1024..=65532).contains(&p) {
                return Err("The port must be between 1024 and 65532.".into());
            }
        }
        match &self.pin {
            PinMode::Off => {}
            // UxPlay shows a random PIN only as console art, which Castaway hides.
            PinMode::Random => {
                return Err("A random PIN isn't supported yet. Choose Off or a fixed 4-digit PIN.".into());
            }
            PinMode::Fixed(s) => {
                if s.len() != 4 || !s.bytes().all(|b| b.is_ascii_digit()) {
                    return Err("The pin must be exactly 4 digits.".into());
                }
                if s == "0000" {
                    return Err("0000 can't be used as a PIN. Choose a different 4-digit PIN.".into());
                }
            }
        }
        if !["d3d11videosink", "d3d12videosink", "glimagesink"].contains(&self.video_sink.as_str()) {
            return Err("The video_sink is not a supported video sink.".into());
        }
        if !["directsoundsink", "wasapisink"].contains(&self.audio_sink.as_str()) {
            return Err("The audio_sink is not a supported audio sink.".into());
        }
        Ok(())
    }
}

/// Builds the UxPlay argv. Always a Vec of separate args, never a shell string.
/// `-p n` makes UxPlay use three consecutive ports: n, n+1, n+2.
/// `-d` (debug) is intentionally not part of Settings.
pub fn to_args(s: &Settings) -> Vec<String> {
    let mut a: Vec<String> = Vec::new();
    let mut push = |v: &str| a.push(v.to_string());
    push("-n");
    push(&s.name);
    if s.hide_hostname {
        push("-nh");
    }
    push("-s");
    push(&format!("{}x{}", s.width, s.height));
    push("-fps");
    push(&s.fps.to_string());
    push("-vs");
    push(&s.video_sink);
    push("-as");
    push(&s.audio_sink);
    match &s.pin {
        PinMode::Off => {}
        PinMode::Random => push("-pin"),
        PinMode::Fixed(p) => {
            push("-pin");
            push(p);
        }
    }
    if let Some(p) = s.port_base {
        push("-p");
        push(&p.to_string());
    }
    // UxPlay only uses its register of trusted devices together with -pin.
    if s.use_registry && s.pin != PinMode::Off {
        push("-reg");
    }
    if s.fullscreen {
        push("-fs");
    }
    a
}

/// Never fails: a missing, corrupt or invalid file yields defaults (values
/// from the file go straight into UxPlay's argv and GStreamer pipelines, so
/// they must pass the same validation as a save).
pub fn load(path: &Path) -> Settings {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str::<Settings>(&t).ok())
        .filter(|s| s.validate().is_ok())
        .unwrap_or_default()
}

pub fn save(path: &Path, s: &Settings) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(s).map_err(std::io::Error::other)?;
    std::fs::write(path, json)
}

pub fn settings_path_in(dir: &Path) -> PathBuf {
    dir.join("settings.json")
}

/// `%APPDATA%\<app identifier>\settings.json`.
pub fn settings_path(app: &tauri::AppHandle) -> PathBuf {
    let dir = app
        .path()
        .app_config_dir()
        .unwrap_or_else(|_| PathBuf::from("."));
    settings_path_in(&dir)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    fn s() -> Settings {
        Settings::default()
    }
    fn has_pair(a: &[String], k: &str, v: &str) -> bool {
        a.windows(2).any(|w| w[0] == k && w[1] == v)
    }
    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("castaway é test {tag} {}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn default_args_include_sinks_and_size() {
        let a = to_args(&s());
        assert!(has_pair(&a, "-vs", "d3d11videosink"));
        assert!(has_pair(&a, "-as", "directsoundsink"));
        assert!(has_pair(&a, "-s", "1920x1080"));
        assert!(has_pair(&a, "-fps", "30"));
        assert!(!a.contains(&"-d".to_string()));
        assert!(!a.contains(&"-pin".to_string()));
        assert!(!a.contains(&"-reg".to_string()));
    }
    #[test]
    fn default_name_starts_with_castaway_on() {
        assert!(s().name.starts_with("Castaway on "));
        assert!(s().name.is_ascii());
    }
    #[test]
    fn default_hides_hostname() {
        assert!(s().hide_hostname);
        assert!(to_args(&s()).contains(&"-nh".to_string()));
    }
    #[test]
    fn ascii_hostname_sanitizer() {
        assert_eq!(ascii_hostname(Some("DESKTOP-AB12")), "DESKTOP-AB12");
        assert_eq!(ascii_hostname(Some("JOSÉ-PC")), "JOS-PC");
        assert_eq!(ascii_hostname(Some(" Ünï ")), "n");
        assert_eq!(ascii_hostname(Some("ÉÈ")), "PC");
        assert_eq!(ascii_hostname(Some("   ")), "PC");
        assert_eq!(ascii_hostname(Some("")), "PC");
        assert_eq!(ascii_hostname(None), "PC");
        assert_eq!(default_name_for(Some("JOSÉ")), "Castaway on JOS");
        assert_eq!(default_name_for(Some("日本")), "Castaway on PC");
    }
    #[test]
    fn name_with_quotes_and_spaces_is_one_arg() {
        let mut x = s();
        x.name = "Ken's \"PC\"".into();
        let a = to_args(&x);
        assert!(has_pair(&a, "-n", "Ken's \"PC\""));
        assert_eq!(a.iter().filter(|v| v.contains("Ken")).count(), 1);
    }
    #[test]
    fn non_ascii_name_is_rejected() {
        let msg = "Use plain letters, numbers, spaces and basic punctuation in the device name \u{2014} accented letters and emoji aren't supported yet.";
        assert_eq!(err(|x| x.name = "José".into()), msg);
        assert_eq!(err(|x| x.name = "Ken 🎬".into()), msg);
        assert_eq!(err(|x| x.name = "tab\there".into()), msg);
    }
    #[test]
    fn leading_dash_name_is_rejected() {
        assert_eq!(
            err(|x| x.name = "-Den-".into()),
            "The device name can't start with a dash (-)."
        );
        assert_eq!(
            err(|x| x.name = " -x".into()),
            "The device name can't start with a dash (-)."
        );
        let mut ok = s();
        ok.name = "Den-".into();
        assert!(ok.validate().is_ok());
    }
    #[test]
    fn name_budget_depends_on_hostname_suffix() {
        assert_eq!(name_budget(true, 11), 50);
        assert_eq!(name_budget(false, 11), 38);
        assert_eq!(name_budget(false, 15), 34);
        assert_eq!(name_budget(false, 200), 0);
        let mut x = s();
        x.hide_hostname = true;
        x.name = "a".repeat(50);
        assert!(x.validate_for_host("DESKTOP-ABC").is_ok());
        x.name = "a".repeat(51);
        assert_eq!(
            x.validate_for_host("DESKTOP-ABC").unwrap_err(),
            "The device name is too long. Use at most 50 characters."
        );
        x.hide_hostname = false;
        x.name = "a".repeat(38);
        assert!(x.validate_for_host("DESKTOP-ABC").is_ok());
        x.name = "a".repeat(39);
        assert_eq!(
            x.validate_for_host("DESKTOP-ABC").unwrap_err(),
            "The device name is too long. Use at most 38 characters (or turn on \"Don't add the computer name\" for more)."
        );
    }
    #[test]
    fn random_pin_and_0000_are_rejected() {
        assert_eq!(
            err(|x| x.pin = PinMode::Random),
            "A random PIN isn't supported yet. Choose Off or a fixed 4-digit PIN."
        );
        assert_eq!(
            err(|x| x.pin = PinMode::Fixed("0000".into())),
            "0000 can't be used as a PIN. Choose a different 4-digit PIN."
        );
    }
    #[test]
    fn registry_flag_only_with_a_pin() {
        let mut x = s();
        x.use_registry = true;
        assert!(!to_args(&x).contains(&"-reg".to_string()));
        x.pin = PinMode::Fixed("1234".into());
        assert!(to_args(&x).contains(&"-reg".to_string()));
        x.use_registry = false;
        assert!(!to_args(&x).contains(&"-reg".to_string()));
    }
    #[test]
    fn load_invalid_but_parseable_returns_default() {
        let d = tmp("invalid");
        let p = d.join("settings.json");
        for json in [
            r#"{"fps": 0}"#,
            r#"{"video_sink": "filesink location=C:/x.bin"}"#,
            r#"{"name": "-x"}"#,
            r#"{"pin": {"Fixed": "12"}}"#,
            r#"{"pin": "Random"}"#,
            r#"{"name": "José"}"#,
        ] {
            std::fs::write(&p, json).unwrap();
            assert_eq!(load(&p), s(), "{json}");
        }
    }
    #[test]
    fn fixed_pin_and_port_flags() {
        let mut x = s();
        x.pin = PinMode::Fixed("1234".into());
        x.port_base = Some(7000);
        let a = to_args(&x);
        assert!(has_pair(&a, "-pin", "1234"));
        assert!(has_pair(&a, "-p", "7000"));
    }
    #[test]
    fn random_pin_is_bare_flag() {
        let mut x = s();
        x.pin = PinMode::Random;
        let a = to_args(&x);
        let i = a.iter().position(|v| v == "-pin").unwrap();
        assert!(a.get(i + 1).map_or(true, |n| n.starts_with('-')));
    }
    #[test]
    fn fullscreen_and_hide_hostname_flags() {
        let mut x = s();
        x.fullscreen = true;
        x.hide_hostname = true;
        x.use_registry = true;
        x.pin = PinMode::Fixed("1234".into());
        let a = to_args(&x);
        for f in ["-fs", "-nh", "-reg"] {
            assert!(a.contains(&f.to_string()), "{f}");
        }
        x.hide_hostname = false;
        assert!(!to_args(&x).contains(&"-nh".to_string()));
    }
    #[test]
    fn load_missing_file_returns_default() {
        let d = tmp("missing");
        assert_eq!(load(&d.join("nope.json")), s());
    }
    #[test]
    fn load_corrupt_json_returns_default() {
        let d = tmp("corrupt");
        let p = d.join("settings.json");
        std::fs::write(&p, "{ not json").unwrap();
        assert_eq!(load(&p), s());
    }
    #[test]
    fn load_partial_json_fills_defaults() {
        let d = tmp("partial");
        let p = d.join("settings.json");
        std::fs::write(&p, r#"{"fps": 60}"#).unwrap();
        let l = load(&p);
        assert_eq!(l.fps, 60);
        assert_eq!(l.width, 1920);
    }
    #[test]
    fn save_then_load_roundtrips() {
        let d = tmp("roundtrip");
        let p = settings_path_in(&d);
        let mut x = s();
        x.name = "Ken's \"PC\"".into();
        x.pin = PinMode::Fixed("0042".into());
        x.port_base = Some(7000);
        save(&p, &x).unwrap();
        assert_eq!(load(&p), x);
    }
    #[test]
    fn settings_path_in_joins_file_name() {
        let d = Path::new(r"C:\Users\Ken é\App Data");
        assert_eq!(settings_path_in(d), d.join("settings.json"));
    }

    #[test]
    fn default_validates() {
        assert!(s().validate().is_ok());
    }
    fn err(f: impl FnOnce(&mut Settings)) -> String {
        let mut x = s();
        f(&mut x);
        x.validate().unwrap_err()
    }
    #[test]
    fn validate_rules() {
        assert!(err(|x| x.name = "  ".into()).contains("name"));
        assert!(err(|x| x.name = "a".repeat(51)).contains("name"));
        assert!(err(|x| x.width = 319).contains("width"));
        assert!(err(|x| x.width = 7681).contains("width"));
        assert!(err(|x| x.height = 100).contains("height"));
        assert!(err(|x| x.fps = 0).contains("fps"));
        assert!(err(|x| x.fps = 61).contains("fps"));
        assert!(err(|x| x.port_base = Some(1023)).contains("port"));
        assert!(err(|x| x.port_base = Some(65533)).contains("port"));
        assert!(err(|x| x.pin = PinMode::Fixed("12a4".into())).contains("pin"));
        assert!(err(|x| x.pin = PinMode::Fixed("123".into())).contains("pin"));
        assert!(err(|x| x.video_sink = "x".into()).contains("video_sink"));
        assert!(err(|x| x.audio_sink = "x".into()).contains("audio_sink"));
        let mut ok = s();
        ok.port_base = Some(65532);
        ok.pin = PinMode::Fixed("0042".into());
        ok.name = "a".repeat(50);
        assert!(ok.validate().is_ok());
    }
}
