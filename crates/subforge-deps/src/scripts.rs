use std::path::{Path, PathBuf};

/// Finds a bundled helper script. Order: `SUBFORGE_SCRIPTS_DIR`, then the layouts
/// used by Windows/Linux installers, Linux packages (`/usr/lib/subforge`), and
/// macOS bundles. Returns `None` when nothing matches so callers can keep their
/// dev-mode fallbacks.
pub fn locate_script(name: &str) -> Option<PathBuf> {
    let override_dir = override_dir_from(std::env::var_os("SUBFORGE_SCRIPTS_DIR"));
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(Path::to_path_buf));
    locate_script_in(name, override_dir.as_deref(), exe_dir.as_deref())
}

/// An empty `SUBFORGE_SCRIPTS_DIR` must not resolve scripts against the cwd.
fn override_dir_from(value: Option<std::ffi::OsString>) -> Option<PathBuf> {
    value.map(PathBuf::from).filter(|d| !d.as_os_str().is_empty())
}

pub fn locate_script_in(
    name: &str,
    override_dir: Option<&Path>,
    exe_dir: Option<&Path>,
) -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(dir) = override_dir {
        candidates.push(dir.join(name));
    }
    if let Some(exe) = exe_dir {
        candidates.push(exe.join("scripts").join(name));
        candidates.push(exe.join("..").join("lib").join("subforge").join("scripts").join(name));
        candidates.push(exe.join("..").join("share").join("subforge").join("scripts").join(name));
        candidates.push(exe.join("..").join("Resources").join("scripts").join(name));
    }
    candidates.into_iter().find(|p| p.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn touch(path: &std::path::Path) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, "#!/usr/bin/env python3\n").unwrap();
    }

    #[test]
    fn finds_scripts_next_to_binary() {
        let tmp = tempdir().unwrap();
        let exe_dir = tmp.path().join("app");
        touch(&exe_dir.join("scripts").join("transcribe.py"));
        let found = locate_script_in("transcribe.py", None, Some(&exe_dir)).unwrap();
        assert!(found.ends_with("scripts/transcribe.py"));
    }

    #[test]
    fn finds_linux_package_layout() {
        // /usr/bin/subforge  +  /usr/lib/subforge/scripts/x.py  (deb, rpm, AUR, AppImage)
        let tmp = tempdir().unwrap();
        let exe_dir = tmp.path().join("usr").join("bin");
        fs::create_dir_all(&exe_dir).unwrap();
        touch(&tmp.path().join("usr/lib/subforge/scripts/subtitle_ops.py"));
        assert!(locate_script_in("subtitle_ops.py", None, Some(&exe_dir)).is_some());
    }

    #[test]
    fn env_override_wins() {
        let tmp = tempdir().unwrap();
        let custom = tmp.path().join("custom");
        touch(&custom.join("subtitle_lines.py"));
        let exe_dir = tmp.path().join("bin");
        touch(&exe_dir.join("scripts").join("subtitle_lines.py"));
        let found = locate_script_in("subtitle_lines.py", Some(&custom), Some(&exe_dir)).unwrap();
        assert!(found.starts_with(&custom));
    }

    #[test]
    fn empty_override_is_ignored() {
        assert_eq!(override_dir_from(None), None);
        assert_eq!(override_dir_from(Some("".into())), None);
        assert_eq!(override_dir_from(Some("/x".into())), Some(PathBuf::from("/x")));
    }

    #[test]
    fn returns_none_when_missing() {
        let tmp = tempdir().unwrap();
        assert!(locate_script_in("nope.py", None, Some(tmp.path())).is_none());
    }
}
