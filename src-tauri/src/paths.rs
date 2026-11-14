use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager};

const UXPLAY_EXE: &str = "resources/uxplay/uxplay.exe";

/// Location of the bundled `uxplay.exe`.
///
/// Debug builds use the vendored copy in the source tree
/// (`src-tauri/resources/uxplay/uxplay.exe`); release builds use the
/// Tauri resource directory next to the installed app.
pub fn uxplay_path(app: &AppHandle) -> PathBuf {
    if cfg!(debug_assertions) {
        dev_path(Path::new(env!("CARGO_MANIFEST_DIR")))
    } else {
        let dir = app
            .path()
            .resource_dir()
            .expect("tauri resource directory is unavailable");
        release_path(&dir)
    }
}

fn dev_path(manifest_dir: &Path) -> PathBuf {
    manifest_dir.join(UXPLAY_EXE)
}

fn release_path(resource_dir: &Path) -> PathBuf {
    resource_dir.join(UXPLAY_EXE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dev_path_points_into_source_tree() {
        let p = dev_path(Path::new("C:/src/app/src-tauri"));
        assert!(p.ends_with("resources/uxplay/uxplay.exe"));
        assert!(p.starts_with("C:/src/app/src-tauri"));
    }

    #[test]
    fn release_path_points_into_resource_dir() {
        let p = release_path(Path::new("C:/Program Files/Castaway"));
        assert_eq!(
            p,
            Path::new("C:/Program Files/Castaway/resources/uxplay/uxplay.exe")
        );
    }
}
