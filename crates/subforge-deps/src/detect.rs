use crate::hide::HideWindow;
use crate::platform::Os;
use std::path::{Path, PathBuf};
use std::process::Command;

pub const MIN_PYTHON: (u32, u32) = (3, 9);

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
                v.extend(python_dirs_in(&pf));
            }
            if let Some(la) = env_path("LOCALAPPDATA") {
                v.push(la.join("Microsoft").join("WinGet").join("Links"));
                v.extend(python_dirs_in(&la.join("Programs").join("Python")));
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

/// `Python3*` install folders (python.org / winget layouts) under `root`.
fn python_dirs_in(root: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(root) else { return Vec::new() };
    entries
        .filter_map(Result::ok)
        .filter(|e| e.file_name().to_string_lossy().starts_with("Python3"))
        .map(|e| e.path())
        .collect()
}

pub fn find_executable(name: &str, env: &SearchEnv) -> Option<PathBuf> {
    let candidates: Vec<String> = if env.os == Os::Windows {
        vec![format!("{name}.exe")]
    } else {
        vec![name.to_string()]
    };
    env.dirs
        .iter()
        .flat_map(|dir| candidates.iter().map(move |c| dir.join(c)))
        .find(|p| is_executable(p))
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

#[derive(Debug, Clone, PartialEq)]
pub struct PythonInfo {
    pub path: PathBuf,
    pub major: u32,
    pub minor: u32,
}

impl PythonInfo {
    pub fn meets_minimum(&self) -> bool {
        (self.major, self.minor) >= MIN_PYTHON
    }

    pub fn version_string(&self) -> String {
        format!("{}.{}", self.major, self.minor)
    }
}

pub fn parse_major_minor(s: &str) -> Option<(u32, u32)> {
    let (major, minor) = s.trim().split_once('.')?;
    Some((major.parse().ok()?, minor.parse().ok()?))
}

/// Runs the interpreter to read its version. This also rejects the Windows
/// Microsoft-Store `python.exe` stub, which fails instead of printing a version.
pub fn probe_python(path: &Path) -> Option<PythonInfo> {
    let out = Command::new(path)
        .hide_window()
        .args(["-c", "import sys;print('%d.%d' % sys.version_info[:2])"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let (major, minor) = parse_major_minor(&String::from_utf8_lossy(&out.stdout))?;
    Some(PythonInfo { path: path.to_path_buf(), major, minor })
}

pub fn venv_python_path(venv_dir: &Path, os: Os) -> PathBuf {
    match os {
        Os::Windows => venv_dir.join("Scripts").join("python.exe"),
        _ => venv_dir.join("bin").join("python"),
    }
}

/// SubForge's private venv first, then the system interpreter.
pub fn find_python(env: &SearchEnv, venv_dir: &Path) -> Option<PythonInfo> {
    let venv_py = venv_python_path(venv_dir, env.os);
    if venv_py.is_file() {
        if let Some(info) = probe_python(&venv_py) {
            return Some(info);
        }
    }
    let names: &[&str] = if env.os == Os::Windows {
        &["python", "python3"]
    } else {
        &["python3", "python"]
    };
    names
        .iter()
        .filter_map(|n| find_executable(n, env))
        .find_map(|p| probe_python(&p))
}

/// Installed version of a Python distribution, or `None` if it is not installed.
/// The distribution name is passed as `argv`, never interpolated into code.
pub fn python_package_version(python: &Path, dist: &str) -> Option<String> {
    const CODE: &str = "import sys, importlib.metadata as m\n\
try:\n    print(m.version(sys.argv[1]))\n\
except m.PackageNotFoundError:\n    sys.exit(1)";
    let out = Command::new(python)
        .hide_window()
        .args(["-c", CODE, dist])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let v = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!v.is_empty()).then_some(v)
}

/// True if `python -m venv` can work (Debian splits this into `python3-venv`).
pub fn python_has_venv(python: &Path) -> bool {
    Command::new(python)
        .hide_window()
        .args(["-c", "import venv, ensurepip"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_major_minor() {
        assert_eq!(parse_major_minor("3.11"), Some((3, 11)));
        assert_eq!(parse_major_minor(" 3.9\n"), Some((3, 9)));
        assert_eq!(parse_major_minor("3"), None);
        assert_eq!(parse_major_minor("garbage"), None);
        assert_eq!(parse_major_minor(""), None);
    }

    #[test]
    fn minimum_python_is_3_9() {
        let mk = |major, minor| PythonInfo { path: "x".into(), major, minor };
        assert!(mk(3, 9).meets_minimum());
        assert!(mk(3, 12).meets_minimum());
        assert!(!mk(3, 8).meets_minimum());
        assert!(!mk(2, 7).meets_minimum());
    }

    #[test]
    fn venv_python_location_depends_on_os() {
        let d = Path::new("/v");
        assert_eq!(venv_python_path(d, Os::Linux), Path::new("/v/bin/python"));
        assert_eq!(
            venv_python_path(d, Os::Windows),
            Path::new("/v").join("Scripts").join("python.exe")
        );
    }

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
        fn probe_python_reads_version() {
            let tmp = tempdir().unwrap();
            let p = make_script(tmp.path(), "python3", "echo 3.11");
            let info = probe_python(&p).unwrap();
            assert_eq!((info.major, info.minor), (3, 11));
        }

        #[test]
        fn probe_python_rejects_broken_interpreters() {
            // e.g. the Windows Store stub: exits non-zero and prints nothing useful.
            let tmp = tempdir().unwrap();
            let p = make_script(tmp.path(), "python3", "exit 9");
            assert!(probe_python(&p).is_none());
        }

        #[test]
        fn find_python_prefers_private_venv() {
            let tmp = tempdir().unwrap();
            let sys = tmp.path().join("sys");
            make_script(&sys, "python3", "echo 3.9");
            let venv = tmp.path().join("venv");
            make_script(&venv.join("bin"), "python", "echo 3.12");
            let found = find_python(&env_with(&sys), &venv).unwrap();
            assert_eq!((found.major, found.minor), (3, 12));
            assert!(found.path.starts_with(&venv));
        }

        #[test]
        fn find_python_falls_back_to_system() {
            let tmp = tempdir().unwrap();
            let sys = tmp.path().join("sys");
            make_script(&sys, "python3", "echo 3.10");
            let found = find_python(&env_with(&sys), &tmp.path().join("no-venv")).unwrap();
            assert_eq!((found.major, found.minor), (3, 10));
        }

        #[test]
        fn find_python_none_when_absent() {
            let tmp = tempdir().unwrap();
            assert!(find_python(&env_with(tmp.path()), &tmp.path().join("v")).is_none());
        }

        #[test]
        fn package_version_present_and_missing() {
            let tmp = tempdir().unwrap();
            let p = make_script(
                tmp.path(),
                "python3",
                r#"case "$*" in *faster-whisper*) exit 1;; esac; echo 4.2.0"#,
            );
            assert_eq!(python_package_version(&p, "pysubs2").as_deref(), Some("4.2.0"));
            assert_eq!(python_package_version(&p, "faster-whisper"), None);
        }

        #[test]
        fn venv_support_probe() {
            let tmp = tempdir().unwrap();
            let ok = make_script(tmp.path(), "py-ok", "exit 0");
            let bad = make_script(tmp.path(), "py-bad", "exit 1");
            assert!(python_has_venv(&ok));
            assert!(!python_has_venv(&bad));
        }
    }
}
