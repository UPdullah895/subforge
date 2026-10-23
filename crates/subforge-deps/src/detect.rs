use crate::hide::HideWindow;
use crate::platform::Os;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Directories searched for executables: `PATH` plus well-known install
/// locations that a freshly started GUI process may not have on `PATH` yet
/// (e.g. right after `winget install`).
#[derive(Debug, Clone)]
pub struct SearchEnv {
    pub dirs: Vec<PathBuf>,
    pub os: Os,
}

impl SearchEnv {
    pub fn from_process() -> Self {
        let os = Os::current();
        let mut dirs: Vec<PathBuf> = std::env::var_os("PATH")
            .map(|p| std::env::split_paths(&p).collect())
            .unwrap_or_default();
        dirs = absolute_only(dirs);
        dirs.extend(well_known_dirs(os));
        Self { dirs, os }
    }
}

fn env_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name).map(PathBuf::from)
}

/// Filter to keep only absolute paths, removing relative and empty entries.
fn absolute_only(dirs: Vec<PathBuf>) -> Vec<PathBuf> {
    dirs.into_iter().filter(|d| d.is_absolute()).collect()
}

fn well_known_dirs(os: Os) -> Vec<PathBuf> {
    let mut v = Vec::new();
    match os {
        Os::Windows => {
            if let Some(pf) = env_path("ProgramFiles") {
                v.push(pf.join("MKVToolNix"));
                v.push(pf.join("ffmpeg").join("bin"));
            }
            if let Some(la) = env_path("LOCALAPPDATA") {
                v.push(la.join("Microsoft").join("WinGet").join("Links"));
            }
        }
        Os::MacOs => {
            v.push(PathBuf::from("/opt/homebrew/bin"));
            v.push(PathBuf::from("/usr/local/bin"));
        }
        Os::Linux => {
            v.push(PathBuf::from("/usr/local/bin"));
            v.push(PathBuf::from("/usr/bin"));
            if let Some(home) = dirs::home_dir() {
                v.push(home.join(".local").join("bin"));
            }
        }
    }
    v
}

pub fn find_executable(name: &str, env: &SearchEnv) -> Option<PathBuf> {
    find_executables(name, env).into_iter().next()
}

/// Every executable called `name` across the search dirs, in search order,
/// without duplicates.
pub fn find_executables(name: &str, env: &SearchEnv) -> Vec<PathBuf> {
    let file = if env.os == Os::Windows { format!("{name}.exe") } else { name.to_string() };
    let mut found: Vec<PathBuf> = Vec::new();
    for dir in &env.dirs {
        let p = dir.join(&file);
        if is_executable(&p) && !found.contains(&p) {
            found.push(p);
        }
    }
    found
}

#[cfg(unix)]
fn is_executable(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    p.is_file()
        && std::fs::metadata(p)
            .map(|m| m.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable(p: &Path) -> bool {
    p.is_file()
}

/// First line of `<tool> <version_arg>` output, or `None` if it fails.
pub fn tool_version(path: &Path, version_arg: &str) -> Option<String> {
    let out = Command::new(path).hide_window().arg(version_arg).output().ok()?;
    if !out.status.success() {
        return None;
    }
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .next()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absolute_only_filters_relative_and_empty() {
        #[cfg(windows)]
        let abs = PathBuf::from("C:\\Windows");
        #[cfg(not(windows))]
        let abs = PathBuf::from("/usr/bin");
        let dirs = vec![PathBuf::from(""), PathBuf::from("relative/bin"), abs.clone()];
        let result = absolute_only(dirs);
        assert_eq!(result, vec![abs]);
    }

    #[cfg(unix)]
    mod unix {
        use super::*;
        use crate::testutil::make_script;
        use tempfile::tempdir;

        fn env_with(dir: &Path) -> SearchEnv {
            SearchEnv { dirs: vec![dir.to_path_buf()], os: Os::Linux }
        }

        #[test]
        fn finds_executable_on_search_dirs() {
            let tmp = tempdir().unwrap();
            let script = make_script(tmp.path(), "mkvmerge", "echo hi");
            assert_eq!(find_executable("mkvmerge", &env_with(tmp.path())), Some(script));
            assert_eq!(find_executable("ffmpeg", &env_with(tmp.path())), None);
        }

        #[test]
        fn ignores_files_without_execute_bit() {
            let tmp = tempdir().unwrap();
            std::fs::write(tmp.path().join("ffmpeg"), "not executable").unwrap();
            assert_eq!(find_executable("ffmpeg", &env_with(tmp.path())), None);
        }

        #[test]
        fn windows_lookup_appends_exe() {
            let tmp = tempdir().unwrap();
            let script = make_script(tmp.path(), "ffmpeg.exe", "echo hi");
            let env = SearchEnv { dirs: vec![tmp.path().to_path_buf()], os: Os::Windows };
            assert_eq!(find_executable("ffmpeg", &env), Some(script));
        }

        #[test]
        fn windows_lookup_rejects_bare_name() {
            let tmp = tempdir().unwrap();
            make_script(tmp.path(), "ffmpeg", "echo hi");
            let env = SearchEnv { dirs: vec![tmp.path().to_path_buf()], os: Os::Windows };
            assert_eq!(find_executable("ffmpeg", &env), None);
        }

        #[test]
        fn tool_version_reads_first_line_only() {
            let tmp = tempdir().unwrap();
            let p = make_script(tmp.path(), "mkvmerge", "echo 'mkvmerge v81.0'; echo second-line");
            assert_eq!(tool_version(&p, "--version").as_deref(), Some("mkvmerge v81.0"));
        }

        #[test]
        fn tool_version_none_when_command_fails() {
            let tmp = tempdir().unwrap();
            let p = make_script(tmp.path(), "ffmpeg", "exit 1");
            assert_eq!(tool_version(&p, "-version"), None);
        }

        #[test]
        fn find_executables_returns_every_match_in_order() {
            let tmp = tempdir().unwrap();
            let a = make_script(&tmp.path().join("a"), "ffmpeg.exe", "exit 9");
            let b = make_script(&tmp.path().join("b"), "ffmpeg.exe", "echo 3.12");
            let env = SearchEnv {
                dirs: vec![tmp.path().join("a"), tmp.path().join("b"), tmp.path().join("a")],
                os: Os::Windows,
            };
            assert_eq!(find_executables("ffmpeg", &env), vec![a.clone(), b]);
            assert_eq!(find_executable("ffmpeg", &env), Some(a));
        }
    }
}
