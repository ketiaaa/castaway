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

fn default_name() -> String {
    let host = std::env::var("COMPUTERNAME")
        .ok()
        .filter(|h| !h.trim().is_empty())
        .unwrap_or_else(|| "PC".to_string());
    format!("Castaway on {host}")
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            name: default_name(),
            hide_hostname: false,
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
        let n = self.name.trim();
        if n.is_empty() {
            return Err("The name must not be empty.".into());
        }
        if self.name.chars().count() > 63 {
            return Err("The name must be 63 characters or fewer.".into());
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
        if let PinMode::Fixed(s) = &self.pin {
            if s.len() != 4 || !s.bytes().all(|b| b.is_ascii_digit()) {
                return Err("The pin must be exactly 4 digits.".into());
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
    if s.use_registry {
        push("-reg");
    }
    if s.fullscreen {
        push("-fs");
    }
    a
}

/// Never fails: a missing or corrupt file yields defaults.
pub fn load(path: &Path) -> Settings {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
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
    }
    #[test]
    fn name_with_quotes_and_emoji_is_one_arg() {
        let mut x = s();
        x.name = "Ken's \"PC\" 🎬".into();
        assert!(has_pair(&to_args(&x), "-n", "Ken's \"PC\" 🎬"));
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
        let a = to_args(&x);
        for f in ["-fs", "-nh", "-reg"] {
            assert!(a.contains(&f.to_string()), "{f}");
        }
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
        x.name = "Ken's 🎬".into();
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
        assert!(err(|x| x.name = "a".repeat(64)).contains("name"));
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
        ok.name = "a".repeat(63);
        assert!(ok.validate().is_ok());
    }
}
