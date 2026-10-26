# Release Readiness Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make SubForge publishable on GitHub and the AUR, working on Linux and Windows, with a startup dependency checker that tells users exactly what is missing and can install it for them.

**Architecture:** A new pure-Rust crate `crates/subforge-deps` holds all detection, install-planning and install-running logic (no Tauri/Axum types). Both backends (`src-tauri`, `server`) call it and expose `check_dependencies` / `install_dependency` (Tauri commands and `GET /api/deps` / `POST /api/deps/install`), streaming install output as `dep-install-progress` events over the existing event channel. The Vue frontend gets a `depsStore`, a shared `DependencyList`, a startup dialog, a "fix now" banner, and a Settings panel. Packaging (AUR `subforge` + `subforge-bin`, deb/rpm metadata) and GitHub docs/CI follow.

**Tech Stack:** Rust (std::process, serde, thiserror, dirs, tempfile for tests), Tauri 2, Axum 0.8, Vue 3 + Pinia + vue-i18n + Vitest, GitHub Actions, PKGBUILD.

**Spec:** `docs/superpowers/specs/2026-10-05-release-readiness-design.md`

## Deviations from the spec (decided while reading the code)

- The existing `ToolRegistry` / `get_tool_registry` stays as-is (six views read it). It keeps working because `which()` now delegates to the new crate. The new `check_dependencies` command is additive.
- "Inline notice in each feature" is delivered as one global banner (`DependencyBanner`) that reopens the dialog. Optional-only gaps (faster-whisper, mkvpropedit) do not open the dialog; they show in the Settings panel (TranscribeView already explains a missing engine).
- The venv lives in `<data_local_dir>/subforge/venv` (lowercase, matching the existing `~/.cache/subforge` naming).
- Server-mode `POST /api/deps/install` is disabled unless `SUBFORGE_ALLOW_INSTALL=1` is set, because the endpoint installs software on the host.
- No `launch-subforge.ps1`: Windows users use the installer; the README says so.
- The root `SubForge.desktop` is removed from the repo (it hard-codes one machine's path); `scripts/install-desktop-launcher.sh` is the supported launcher.

## Global Constraints

- Required deps: `mkvmerge`, `mkvextract`, `ffmpeg`, `ffprobe`, Python >= 3.9, `pysubs2`. Optional: `mkvpropedit`, `faster-whisper`.
- Install runs through an argv array, never a shell; only allowlisted package names/ids (fixed tables in `platform.rs` and `DepId::pip_name`) are ever passed to a package manager.
- Linux elevation uses `pkexec`; if it is missing, fall back to showing the `sudo` command (`auto_runnable = false`).
- Python packages install into a venv created with `--system-site-packages`; never into system Python.
- Do not push, tag, or submit to the AUR. Local commits only.
- Commit trailer on every commit: `Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>` (as a second `-m`).
- Shell is zsh/bash-compatible; Rust builds run from each crate dir (there is no root Cargo workspace).

## File Structure

New crate `crates/subforge-deps/`:
- `Cargo.toml`, `src/lib.rs` (re-exports)
- `src/model.rs` — `DepId`, `Severity`, `DepState`, `DepSpec`, `DepStatus`, `DepReport`
- `src/platform.rs` — `Os`, `PackageManager`, `SystemPackage`, package tables, manager detection
- `src/hide.rs` — `HideWindow` trait (CREATE_NO_WINDOW on Windows)
- `src/scripts.rs` — `locate_script`
- `src/detect.rs` — `SearchEnv`, `find_executable`, `tool_version`, Python probing, package versions
- `src/context.rs` — `Context` (everything environment-dependent in one struct)
- `src/install.rs` — `InstallPlan`, `plan_for`, `run_plan`, `InstallError`
- `src/check.rs` — `check_all`
- `src/testutil.rs` — test-only fake executables
- `examples/check.rs` — prints the report as JSON (manual verification)

Modified: `src-tauri/{Cargo.toml,tauri.conf.json,src/lib.rs,src/tools/*.rs,src/commands/{mod,deps,subtitle,transcribe}.rs,src/jobs/mod.rs}`, `server/{Cargo.toml,src/main.rs,src/error.rs,src/routes/{mod,deps,subtitle,transcribe}.rs,src/tools/*.rs}`, `src/api/{client,schemas}.ts`, `src/App.vue`, `src/views/SettingsView.vue`, `src/i18n/{en,ar}.json`, `.github/workflows/{ci,release}.yml`, `.gitignore`, `README.md`.

New elsewhere: `src/stores/depsStore.ts` (+test), `src/utils/openExternal.ts`, `src/components/{DependencyList,DependencyDialog,DependencyBanner,DependenciesPanel}.vue` (+tests), `scripts/check-versions.mjs`, `packaging/linux/subforge.desktop`, `packaging/aur/subforge/PKGBUILD`, `packaging/aur/subforge-bin/PKGBUILD`, `packaging/aur/README.md`, `CONTRIBUTING.md`, `SECURITY.md`, `CHANGELOG.md`, `.github/ISSUE_TEMPLATE/*`, `.github/pull_request_template.md`.

---

### Task 1: Crate scaffold and data model

**Files:**
- Create: `crates/subforge-deps/Cargo.toml`, `crates/subforge-deps/src/lib.rs`, `crates/subforge-deps/src/model.rs`

**Interfaces:**
- Produces: `DepId` (`ALL`, `as_str`, `parse`, `spec`, `version_arg`, `pip_name`), `DepSpec { label, severity, purpose, download_url }`, `Severity`, `DepState`, `DepStatus`, `DepReport::new(os: String, items: Vec<DepStatus>)`.

- [ ] **Step 1: Create the manifest**

`crates/subforge-deps/Cargo.toml`:
```toml
[package]
name = "subforge-deps"
version = "0.1.0"
edition = "2021"
license = "GPL-3.0-only"
description = "Dependency detection and installation helpers for SubForge"

[dependencies]
serde = { version = "1", features = ["derive"] }
thiserror = "2"
dirs = "6"
tokio = { version = "1", default-features = false, features = ["process"], optional = true }

[features]
default = []
# Implements HideWindow for tokio::process::Command as well.
tokio = ["dep:tokio"]

[dev-dependencies]
tempfile = "3"
serde_json = "1"
```

`crates/subforge-deps/src/lib.rs`:
```rust
pub mod model;

pub use model::{DepId, DepReport, DepSpec, DepState, DepStatus, Severity};
```

- [ ] **Step 2: Write the failing tests (bottom of `model.rs`)**

`crates/subforge-deps/src/model.rs` (tests first; implementation follows in Step 4):
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_round_trip_through_strings() {
        for id in DepId::ALL {
            assert_eq!(DepId::parse(id.as_str()), Some(id));
        }
    }

    #[test]
    fn parse_rejects_unknown_and_injection_attempts() {
        assert_eq!(DepId::parse("nope"), None);
        assert_eq!(DepId::parse("pysubs2; rm -rf /"), None);
        assert_eq!(DepId::parse(""), None);
    }

    #[test]
    fn serde_names_match_as_str() {
        let json = serde_json::to_string(&DepId::FasterWhisper).unwrap();
        assert_eq!(json, "\"faster_whisper\"");
        assert_eq!(DepId::FasterWhisper.as_str(), "faster_whisper");
    }

    #[test]
    fn only_python_packages_have_pip_names() {
        assert_eq!(DepId::Pysubs2.pip_name(), Some("pysubs2"));
        assert_eq!(DepId::FasterWhisper.pip_name(), Some("faster-whisper"));
        assert_eq!(DepId::Ffmpeg.pip_name(), None);
        assert_eq!(DepId::Python.pip_name(), None);
    }

    fn status(id: DepId, state: DepState) -> DepStatus {
        let spec = id.spec();
        DepStatus {
            id,
            label: spec.label.to_string(),
            severity: spec.severity,
            state,
            path: None,
            version: None,
            purpose: spec.purpose.to_string(),
            download_url: spec.download_url.to_string(),
            install_command: None,
            can_auto_install: false,
        }
    }

    #[test]
    fn report_flags_missing_required_only() {
        let optional_missing = DepReport::new(
            "linux".into(),
            vec![status(DepId::Ffmpeg, DepState::Ok), status(DepId::FasterWhisper, DepState::Missing)],
        );
        assert!(!optional_missing.has_missing_required);

        let required_missing = DepReport::new(
            "linux".into(),
            vec![status(DepId::Ffmpeg, DepState::Missing)],
        );
        assert!(required_missing.has_missing_required);

        let outdated = DepReport::new("linux".into(), vec![status(DepId::Python, DepState::Outdated)]);
        assert!(outdated.has_missing_required);
    }
}
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cd crates/subforge-deps && cargo test`
Expected: FAIL to compile (`DepId`, `DepReport` etc. not defined).

- [ ] **Step 4: Write the implementation (top of `model.rs`, above the tests)**

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Required,
    Optional,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DepState {
    Ok,
    Missing,
    Outdated,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DepId {
    Mkvmerge,
    Mkvextract,
    Mkvpropedit,
    Ffmpeg,
    Ffprobe,
    Python,
    Pysubs2,
    FasterWhisper,
}

/// Static, human-readable facts about a dependency.
#[derive(Debug, Clone, Copy)]
pub struct DepSpec {
    pub label: &'static str,
    pub severity: Severity,
    pub purpose: &'static str,
    pub download_url: &'static str,
}

impl DepId {
    /// Display / check order: Python comes before the Python packages.
    pub const ALL: [DepId; 8] = [
        DepId::Mkvmerge,
        DepId::Mkvextract,
        DepId::Mkvpropedit,
        DepId::Ffmpeg,
        DepId::Ffprobe,
        DepId::Python,
        DepId::Pysubs2,
        DepId::FasterWhisper,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            DepId::Mkvmerge => "mkvmerge",
            DepId::Mkvextract => "mkvextract",
            DepId::Mkvpropedit => "mkvpropedit",
            DepId::Ffmpeg => "ffmpeg",
            DepId::Ffprobe => "ffprobe",
            DepId::Python => "python",
            DepId::Pysubs2 => "pysubs2",
            DepId::FasterWhisper => "faster_whisper",
        }
    }

    pub fn parse(s: &str) -> Option<DepId> {
        DepId::ALL.into_iter().find(|id| id.as_str() == s)
    }

    /// Argument that makes an executable print its version.
    pub fn version_arg(self) -> &'static str {
        match self {
            DepId::Ffmpeg | DepId::Ffprobe => "-version",
            _ => "--version",
        }
    }

    /// PyPI distribution name. This is the allowlist for pip installs: only ids
    /// that return `Some` can ever be passed to `pip install`.
    pub fn pip_name(self) -> Option<&'static str> {
        match self {
            DepId::Pysubs2 => Some("pysubs2"),
            DepId::FasterWhisper => Some("faster-whisper"),
            _ => None,
        }
    }

    pub fn spec(self) -> DepSpec {
        const MKV_URL: &str = "https://mkvtoolnix.download/downloads.html";
        const FFMPEG_URL: &str = "https://ffmpeg.org/download.html";
        match self {
            DepId::Mkvmerge => DepSpec {
                label: "mkvmerge (MKVToolNix)",
                severity: Severity::Required,
                purpose: "Merging subtitle and audio tracks into MKV files",
                download_url: MKV_URL,
            },
            DepId::Mkvextract => DepSpec {
                label: "mkvextract (MKVToolNix)",
                severity: Severity::Required,
                purpose: "Extracting subtitle and audio tracks from MKV files",
                download_url: MKV_URL,
            },
            DepId::Mkvpropedit => DepSpec {
                label: "mkvpropedit (MKVToolNix)",
                severity: Severity::Optional,
                purpose: "Editing track names, languages and flags without remuxing",
                download_url: MKV_URL,
            },
            DepId::Ffmpeg => DepSpec {
                label: "FFmpeg",
                severity: Severity::Required,
                purpose: "Extracting, converting and merging audio",
                download_url: FFMPEG_URL,
            },
            DepId::Ffprobe => DepSpec {
                label: "ffprobe (FFmpeg)",
                severity: Severity::Required,
                purpose: "Reading audio track information",
                download_url: FFMPEG_URL,
            },
            DepId::Python => DepSpec {
                label: "Python 3.9+",
                severity: Severity::Required,
                purpose: "Running the subtitle conversion, translation and transcription helpers",
                download_url: "https://www.python.org/downloads/",
            },
            DepId::Pysubs2 => DepSpec {
                label: "pysubs2 (Python package)",
                severity: Severity::Required,
                purpose: "Converting and retiming subtitle formats (SRT, ASS, VTT)",
                download_url: "https://pypi.org/project/pysubs2/",
            },
            DepId::FasterWhisper => DepSpec {
                label: "faster-whisper (Python package)",
                severity: Severity::Optional,
                purpose: "Speech-to-text transcription",
                download_url: "https://pypi.org/project/faster-whisper/",
            },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DepStatus {
    pub id: DepId,
    pub label: String,
    pub severity: Severity,
    pub state: DepState,
    pub path: Option<String>,
    pub version: Option<String>,
    pub purpose: String,
    pub download_url: String,
    /// Exact command a user can paste into a terminal (shown/copyable in the UI).
    pub install_command: Option<String>,
    /// True when SubForge can run the install itself right now.
    pub can_auto_install: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DepReport {
    pub os: String,
    pub items: Vec<DepStatus>,
    pub has_missing_required: bool,
}

impl DepReport {
    pub fn new(os: String, items: Vec<DepStatus>) -> Self {
        let has_missing_required = items
            .iter()
            .any(|i| i.severity == Severity::Required && i.state != DepState::Ok);
        Self { os, items, has_missing_required }
    }
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cd crates/subforge-deps && cargo test`
Expected: 5 tests PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/subforge-deps
git commit -m "feat(deps): add subforge-deps crate with dependency data model" -m "Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>"
```

---

### Task 2: Platform and package-manager tables

**Files:**
- Create: `crates/subforge-deps/src/platform.rs`
- Modify: `crates/subforge-deps/src/lib.rs`

**Interfaces:**
- Consumes: nothing.
- Produces: `Os` (`current()`, `as_str()`), `PackageManager` (`program()`, `needs_elevation()`, `install_commands(SystemPackage) -> Vec<Vec<String>>`), `SystemPackage { Mkvtoolnix, Ffmpeg, Python }`, `detect_package_manager(os: Os, has: &dyn Fn(&str) -> bool) -> Option<PackageManager>`.

- [ ] **Step 1: Write the failing tests**

`crates/subforge-deps/src/platform.rs` (tests block first):
```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn has_only(names: &'static [&'static str]) -> impl Fn(&str) -> bool {
        move |n| names.contains(&n)
    }

    #[test]
    fn linux_prefers_pacman_then_apt_dnf_zypper() {
        assert_eq!(
            detect_package_manager(Os::Linux, &has_only(&["apt-get", "pacman"])),
            Some(PackageManager::Pacman)
        );
        assert_eq!(
            detect_package_manager(Os::Linux, &has_only(&["apt-get"])),
            Some(PackageManager::Apt)
        );
        assert_eq!(
            detect_package_manager(Os::Linux, &has_only(&["dnf"])),
            Some(PackageManager::Dnf)
        );
        assert_eq!(
            detect_package_manager(Os::Linux, &has_only(&["zypper"])),
            Some(PackageManager::Zypper)
        );
        assert_eq!(detect_package_manager(Os::Linux, &has_only(&[])), None);
    }

    #[test]
    fn windows_uses_winget_and_macos_uses_brew() {
        assert_eq!(
            detect_package_manager(Os::Windows, &has_only(&["winget"])),
            Some(PackageManager::Winget)
        );
        assert_eq!(detect_package_manager(Os::Windows, &has_only(&[])), None);
        assert_eq!(
            detect_package_manager(Os::MacOs, &has_only(&["brew"])),
            Some(PackageManager::Brew)
        );
    }

    #[test]
    fn pacman_command_is_exact() {
        assert_eq!(
            PackageManager::Pacman.install_commands(SystemPackage::Mkvtoolnix),
            vec![vec!["pacman", "-S", "--needed", "--noconfirm", "mkvtoolnix-cli"]]
        );
    }

    #[test]
    fn apt_python_installs_venv_and_pip_too() {
        let cmds = PackageManager::Apt.install_commands(SystemPackage::Python);
        assert_eq!(
            cmds,
            vec![vec!["apt-get", "install", "-y", "python3", "python3-venv", "python3-pip"]]
        );
    }

    #[test]
    fn winget_uses_one_command_per_package_id() {
        assert_eq!(
            PackageManager::Winget.install_commands(SystemPackage::Python),
            vec![vec![
                "winget", "install", "--exact", "--id", "Python.Python.3.12",
                "--accept-package-agreements", "--accept-source-agreements"
            ]]
        );
        assert_eq!(
            PackageManager::Winget.install_commands(SystemPackage::Mkvtoolnix)[0][4],
            "MoritzBunkus.MKVToolNix"
        );
        assert_eq!(
            PackageManager::Winget.install_commands(SystemPackage::Ffmpeg)[0][4],
            "Gyan.FFmpeg"
        );
    }

    #[test]
    fn elevation_rules() {
        assert!(PackageManager::Pacman.needs_elevation());
        assert!(PackageManager::Apt.needs_elevation());
        assert!(PackageManager::Dnf.needs_elevation());
        assert!(PackageManager::Zypper.needs_elevation());
        assert!(!PackageManager::Brew.needs_elevation());
        assert!(!PackageManager::Winget.needs_elevation());
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cd crates/subforge-deps && cargo test platform`
Expected: FAIL (module not declared / types missing).

- [ ] **Step 3: Implement (above the tests in `platform.rs`) and register the module**

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Os {
    Linux,
    Windows,
    MacOs,
}

impl Os {
    pub fn current() -> Os {
        if cfg!(target_os = "windows") {
            Os::Windows
        } else if cfg!(target_os = "macos") {
            Os::MacOs
        } else {
            Os::Linux
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Os::Linux => "linux",
            Os::Windows => "windows",
            Os::MacOs => "macos",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackageManager {
    Pacman,
    Apt,
    Dnf,
    Zypper,
    Brew,
    Winget,
}

/// Groups of system packages SubForge knows how to install.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemPackage {
    Mkvtoolnix,
    Ffmpeg,
    Python,
}

impl PackageManager {
    pub fn program(self) -> &'static str {
        match self {
            PackageManager::Pacman => "pacman",
            PackageManager::Apt => "apt-get",
            PackageManager::Dnf => "dnf",
            PackageManager::Zypper => "zypper",
            PackageManager::Brew => "brew",
            PackageManager::Winget => "winget",
        }
    }

    /// Linux system managers need root; brew and winget handle that themselves.
    pub fn needs_elevation(self) -> bool {
        matches!(
            self,
            PackageManager::Pacman | PackageManager::Apt | PackageManager::Dnf | PackageManager::Zypper
        )
    }

    /// Fixed allowlist of package names (or winget ids) — nothing user-supplied.
    fn packages(self, pkg: SystemPackage) -> &'static [&'static str] {
        use PackageManager::*;
        use SystemPackage::*;
        match (self, pkg) {
            (Pacman, Mkvtoolnix) => &["mkvtoolnix-cli"],
            (Pacman, Ffmpeg) => &["ffmpeg"],
            (Pacman, Python) => &["python"],
            (Apt, Mkvtoolnix) => &["mkvtoolnix"],
            (Apt, Ffmpeg) => &["ffmpeg"],
            (Apt, Python) => &["python3", "python3-venv", "python3-pip"],
            (Dnf, Mkvtoolnix) => &["mkvtoolnix"],
            (Dnf, Ffmpeg) => &["ffmpeg-free"],
            (Dnf, Python) => &["python3", "python3-pip"],
            (Zypper, Mkvtoolnix) => &["mkvtoolnix"],
            (Zypper, Ffmpeg) => &["ffmpeg"],
            (Zypper, Python) => &["python3", "python3-pip"],
            (Brew, Mkvtoolnix) => &["mkvtoolnix"],
            (Brew, Ffmpeg) => &["ffmpeg"],
            (Brew, Python) => &["python"],
            (Winget, Mkvtoolnix) => &["MoritzBunkus.MKVToolNix"],
            (Winget, Ffmpeg) => &["Gyan.FFmpeg"],
            (Winget, Python) => &["Python.Python.3.12"],
        }
    }

    /// argv lists (program first, no elevation prefix) that install `pkg`.
    pub fn install_commands(self, pkg: SystemPackage) -> Vec<Vec<String>> {
        let names = self.packages(pkg);
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        match self {
            PackageManager::Winget => names
                .iter()
                .map(|id| {
                    s(&[
                        "winget", "install", "--exact", "--id", id,
                        "--accept-package-agreements", "--accept-source-agreements",
                    ])
                })
                .collect(),
            _ => {
                let mut argv = match self {
                    PackageManager::Pacman => s(&["pacman", "-S", "--needed", "--noconfirm"]),
                    PackageManager::Apt => s(&["apt-get", "install", "-y"]),
                    PackageManager::Dnf => s(&["dnf", "install", "-y"]),
                    PackageManager::Zypper => s(&["zypper", "--non-interactive", "install"]),
                    PackageManager::Brew => s(&["brew", "install"]),
                    PackageManager::Winget => unreachable!(),
                };
                argv.extend(names.iter().map(|n| n.to_string()));
                vec![argv]
            }
        }
    }
}

/// `has` answers "is this program on the search path?".
pub fn detect_package_manager(os: Os, has: &dyn Fn(&str) -> bool) -> Option<PackageManager> {
    let candidates: &[PackageManager] = match os {
        Os::Windows => &[PackageManager::Winget],
        Os::MacOs => &[PackageManager::Brew],
        Os::Linux => &[
            PackageManager::Pacman,
            PackageManager::Apt,
            PackageManager::Dnf,
            PackageManager::Zypper,
        ],
    };
    candidates.iter().copied().find(|pm| has(pm.program()))
}
```

Add to `lib.rs`:
```rust
pub mod platform;

pub use platform::{detect_package_manager, Os, PackageManager, SystemPackage};
```

- [ ] **Step 4: Run to verify pass**

Run: `cd crates/subforge-deps && cargo test`
Expected: all tests PASS (11 total).

- [ ] **Step 5: Commit**

```bash
git add crates/subforge-deps
git commit -m "feat(deps): OS and package-manager tables with fixed package allowlist" -m "Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>"
```

---

### Task 3: Window hiding and script location

**Files:**
- Create: `crates/subforge-deps/src/hide.rs`, `crates/subforge-deps/src/scripts.rs`
- Modify: `crates/subforge-deps/src/lib.rs`

**Interfaces:**
- Produces: trait `HideWindow { fn hide_window(&mut self) -> &mut Self }` for `std::process::Command` (and `tokio::process::Command` with feature `tokio`); `locate_script(name: &str) -> Option<PathBuf>`; `locate_script_in(name, override_dir: Option<&Path>, exe_dir: Option<&Path>) -> Option<PathBuf>`.

- [ ] **Step 1: Write the failing test for `scripts.rs`**

`crates/subforge-deps/src/scripts.rs`:
```rust
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
    fn returns_none_when_missing() {
        let tmp = tempdir().unwrap();
        assert!(locate_script_in("nope.py", None, Some(tmp.path())).is_none());
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cd crates/subforge-deps && cargo test scripts`
Expected: FAIL (function missing / module not declared).

- [ ] **Step 3: Implement `scripts.rs` (above tests) and `hide.rs`**

`scripts.rs`:
```rust
use std::path::{Path, PathBuf};

/// Finds a bundled helper script. Order: `SUBFORGE_SCRIPTS_DIR`, then the layouts
/// used by Windows/Linux installers, Linux packages (`/usr/lib/subforge`), and
/// macOS bundles. Returns `None` when nothing matches so callers can keep their
/// dev-mode fallbacks.
pub fn locate_script(name: &str) -> Option<PathBuf> {
    let override_dir = std::env::var_os("SUBFORGE_SCRIPTS_DIR").map(PathBuf::from);
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(Path::to_path_buf));
    locate_script_in(name, override_dir.as_deref(), exe_dir.as_deref())
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
```

`hide.rs`:
```rust
/// Stops a console window from flashing when a GUI app spawns a child process
/// on Windows. No-op on other platforms.
pub trait HideWindow {
    fn hide_window(&mut self) -> &mut Self;
}

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

impl HideWindow for std::process::Command {
    #[cfg(windows)]
    fn hide_window(&mut self) -> &mut Self {
        use std::os::windows::process::CommandExt;
        self.creation_flags(CREATE_NO_WINDOW)
    }

    #[cfg(not(windows))]
    fn hide_window(&mut self) -> &mut Self {
        self
    }
}

#[cfg(feature = "tokio")]
impl HideWindow for tokio::process::Command {
    #[cfg(windows)]
    fn hide_window(&mut self) -> &mut Self {
        self.creation_flags(CREATE_NO_WINDOW)
    }

    #[cfg(not(windows))]
    fn hide_window(&mut self) -> &mut Self {
        self
    }
}
```

`lib.rs` additions:
```rust
pub mod hide;
pub mod scripts;

pub use hide::HideWindow;
pub use scripts::{locate_script, locate_script_in};
```

- [ ] **Step 4: Run to verify pass**

Run: `cd crates/subforge-deps && cargo test && cargo check --features tokio`
Expected: tests PASS; check OK.

- [ ] **Step 5: Commit**

```bash
git add crates/subforge-deps
git commit -m "feat(deps): script locator for packaged layouts and HideWindow trait" -m "Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>"
```

---

### Task 4: Executable and Python detection

**Files:**
- Create: `crates/subforge-deps/src/detect.rs`, `crates/subforge-deps/src/testutil.rs`
- Modify: `crates/subforge-deps/src/lib.rs`

**Interfaces:**
- Consumes: `Os`, `HideWindow`.
- Produces: `SearchEnv { dirs: Vec<PathBuf>, os: Os }` with `SearchEnv::from_process()`; `find_executable(name, &SearchEnv) -> Option<PathBuf>`; `tool_version(&Path, &str) -> Option<String>`; `PythonInfo { path: PathBuf, major: u32, minor: u32 }` with `meets_minimum()` and `version_string()`; `MIN_PYTHON: (u32, u32)`; `parse_major_minor(&str) -> Option<(u32,u32)>`; `probe_python(&Path) -> Option<PythonInfo>`; `venv_python_path(&Path, Os) -> PathBuf`; `find_python(&SearchEnv, venv_dir: &Path) -> Option<PythonInfo>`; `python_package_version(&Path, &str) -> Option<String>`; `python_has_venv(&Path) -> bool`.

- [ ] **Step 1: Test helper**

`crates/subforge-deps/src/testutil.rs`:
```rust
use std::fs;
use std::path::{Path, PathBuf};

/// Writes an executable `#!/bin/sh` script named `name` into `dir`.
#[cfg(unix)]
pub fn make_script(dir: &Path, name: &str, body: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    fs::create_dir_all(dir).unwrap();
    let path = dir.join(name);
    fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    path
}
```
Add to `lib.rs`: `#[cfg(test)] pub(crate) mod testutil;`

- [ ] **Step 2: Write the failing tests**

Bottom of `crates/subforge-deps/src/detect.rs`:
```rust
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
```

- [ ] **Step 3: Run to verify failure**

Run: `cd crates/subforge-deps && cargo test detect`
Expected: FAIL (items undefined).

- [ ] **Step 4: Implement (above the tests in `detect.rs`)**

```rust
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
        dirs.extend(well_known_dirs(os));
        Self { dirs, os }
    }
}

fn env_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name).map(PathBuf::from)
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
        vec![format!("{name}.exe"), name.to_string()]
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
```

`lib.rs` additions:
```rust
pub mod detect;

pub use detect::{
    find_executable, find_python, parse_major_minor, probe_python, python_has_venv,
    python_package_version, tool_version, venv_python_path, PythonInfo, SearchEnv, MIN_PYTHON,
};
```

- [ ] **Step 5: Run to verify pass**

Run: `cd crates/subforge-deps && cargo test`
Expected: all PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/subforge-deps
git commit -m "feat(deps): executable, Python and package detection" -m "Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>"
```

---

### Task 5: Context

**Files:**
- Create: `crates/subforge-deps/src/context.rs`
- Modify: `crates/subforge-deps/src/lib.rs`

**Interfaces:**
- Consumes: `SearchEnv`, `find_executable`, `find_python`, `detect_package_manager`, `Os`.
- Produces: `Context { os: Os, env: SearchEnv, venv_dir: PathBuf, package_manager: Option<PackageManager>, has_pkexec: bool }` with `Context::from_process()`, `find_executable(&self, &str) -> Option<PathBuf>`, `find_python(&self) -> Option<PythonInfo>`; `default_venv_dir() -> PathBuf`.

- [ ] **Step 1: Write the failing test**

`crates/subforge-deps/src/context.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_venv_dir_is_under_subforge() {
        let d = default_venv_dir();
        assert!(d.ends_with("subforge/venv") || d.ends_with("subforge\\venv"));
    }

    #[test]
    fn from_process_matches_current_os() {
        let ctx = Context::from_process();
        assert_eq!(ctx.os, Os::current());
        assert_eq!(ctx.env.os, ctx.os);
    }

    #[test]
    fn context_lookups_use_its_search_env() {
        let ctx = Context {
            os: Os::Linux,
            env: SearchEnv { dirs: vec![], os: Os::Linux },
            venv_dir: "/nonexistent/venv".into(),
            package_manager: None,
            has_pkexec: false,
        };
        assert!(ctx.find_executable("ffmpeg").is_none());
        assert!(ctx.find_python().is_none());
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cd crates/subforge-deps && cargo test context`
Expected: FAIL (module missing).

- [ ] **Step 3: Implement (above the tests)**

```rust
use crate::detect::{find_executable, find_python, PythonInfo, SearchEnv};
use crate::platform::{detect_package_manager, Os, PackageManager};
use std::path::PathBuf;

/// Everything about the host that detection and install planning depend on,
/// gathered once so the logic stays pure and testable.
#[derive(Debug, Clone)]
pub struct Context {
    pub os: Os,
    pub env: SearchEnv,
    pub venv_dir: PathBuf,
    pub package_manager: Option<PackageManager>,
    pub has_pkexec: bool,
}

pub fn default_venv_dir() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("subforge")
        .join("venv")
}

impl Context {
    pub fn from_process() -> Self {
        let os = Os::current();
        let env = SearchEnv::from_process();
        let package_manager = detect_package_manager(os, &|n| find_executable(n, &env).is_some());
        let has_pkexec = find_executable("pkexec", &env).is_some();
        Self { os, env, venv_dir: default_venv_dir(), package_manager, has_pkexec }
    }

    pub fn find_executable(&self, name: &str) -> Option<PathBuf> {
        find_executable(name, &self.env)
    }

    pub fn find_python(&self) -> Option<PythonInfo> {
        find_python(&self.env, &self.venv_dir)
    }
}
```

`lib.rs`: `pub mod context;` and `pub use context::{default_venv_dir, Context};`

- [ ] **Step 4: Run to verify pass**

Run: `cd crates/subforge-deps && cargo test`
Expected: all PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/subforge-deps
git commit -m "feat(deps): Context struct capturing host environment" -m "Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>"
```

---

### Task 6: Install planning and running

**Files:**
- Create: `crates/subforge-deps/src/install.rs`
- Modify: `crates/subforge-deps/src/lib.rs`

**Interfaces:**
- Consumes: `Context`, `DepId`, `PackageManager`, `SystemPackage`, `Os`, `find_python`/`probe_python`/`python_has_venv`/`venv_python_path`, `HideWindow`.
- Produces: `InstallPlan { dep: DepId, steps: Vec<Vec<String>>, display_command: String, auto_runnable: bool }`; `InstallError` (`NoPackageManager`, `PythonRequired`, `PythonTooOld`, `Spawn(String)`, `StepFailed { command: String, code: Option<i32> }`); `plan_for(DepId, &Context) -> Result<InstallPlan, InstallError>`; `run_plan(&InstallPlan, impl FnMut(&str)) -> Result<(), InstallError>`.

- [ ] **Step 1: Write the failing tests**

Bottom of `crates/subforge-deps/src/install.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::detect::SearchEnv;
    use crate::platform::Os;
    use std::path::PathBuf;

    fn ctx(os: Os, pm: Option<PackageManager>, pkexec: bool) -> Context {
        Context {
            os,
            env: SearchEnv { dirs: vec![], os },
            venv_dir: PathBuf::from("/nonexistent/subforge/venv"),
            package_manager: pm,
            has_pkexec: pkexec,
        }
    }

    #[test]
    fn pacman_plan_runs_through_pkexec_and_displays_sudo() {
        let plan = plan_for(DepId::Mkvmerge, &ctx(Os::Linux, Some(PackageManager::Pacman), true)).unwrap();
        assert_eq!(
            plan.steps,
            vec![vec!["pkexec", "pacman", "-S", "--needed", "--noconfirm", "mkvtoolnix-cli"]]
        );
        assert_eq!(plan.display_command, "sudo pacman -S --needed --noconfirm mkvtoolnix-cli");
        assert!(plan.auto_runnable);
    }

    #[test]
    fn without_pkexec_the_plan_is_display_only() {
        let plan = plan_for(DepId::Ffmpeg, &ctx(Os::Linux, Some(PackageManager::Apt), false)).unwrap();
        assert!(!plan.auto_runnable);
        assert_eq!(plan.display_command, "sudo apt-get install -y ffmpeg");
    }

    #[test]
    fn winget_needs_no_elevation_prefix() {
        let plan = plan_for(DepId::Ffprobe, &ctx(Os::Windows, Some(PackageManager::Winget), false)).unwrap();
        assert!(plan.auto_runnable);
        assert_eq!(plan.steps[0][0], "winget");
        assert!(!plan.display_command.starts_with("sudo"));
        assert!(plan.display_command.contains("Gyan.FFmpeg"));
    }

    #[test]
    fn shared_packages_give_the_same_command() {
        let c = ctx(Os::Linux, Some(PackageManager::Pacman), true);
        let a = plan_for(DepId::Mkvmerge, &c).unwrap();
        let b = plan_for(DepId::Mkvextract, &c).unwrap();
        assert_eq!(a.steps, b.steps);
    }

    #[test]
    fn no_package_manager_is_an_error() {
        let err = plan_for(DepId::Ffmpeg, &ctx(Os::Linux, None, true)).unwrap_err();
        assert!(matches!(err, InstallError::NoPackageManager));
    }

    #[test]
    fn pip_plan_requires_python() {
        let err = plan_for(DepId::Pysubs2, &ctx(Os::Linux, Some(PackageManager::Pacman), true)).unwrap_err();
        assert!(matches!(err, InstallError::PythonRequired));
    }

    #[cfg(unix)]
    mod unix {
        use super::*;
        use crate::testutil::make_script;
        use tempfile::tempdir;

        fn ctx_with_python(tmp: &std::path::Path, version: &str, pm: Option<PackageManager>) -> Context {
            let sys = tmp.join("sys");
            make_script(&sys, "python3", &format!("echo {version}"));
            Context {
                os: Os::Linux,
                env: SearchEnv { dirs: vec![sys], os: Os::Linux },
                venv_dir: tmp.join("venv"),
                package_manager: pm,
                has_pkexec: true,
            }
        }

        #[test]
        fn pip_plan_creates_venv_then_installs_allowlisted_package() {
            let tmp = tempdir().unwrap();
            let c = ctx_with_python(tmp.path(), "3.11", Some(PackageManager::Pacman));
            let plan = plan_for(DepId::Pysubs2, &c).unwrap();
            let venv = c.venv_dir.to_string_lossy().to_string();
            let venv_py = c.venv_dir.join("bin").join("python").to_string_lossy().to_string();
            assert_eq!(plan.steps.len(), 2);
            assert_eq!(&plan.steps[0][1..], ["-m", "venv", "--system-site-packages", venv.as_str()]);
            assert_eq!(
                plan.steps[1],
                vec![venv_py, "-m".into(), "pip".into(), "install".into(), "--upgrade".into(), "pysubs2".into()]
            );
            assert!(plan.auto_runnable);
        }

        #[test]
        fn pip_plan_rejects_old_python() {
            let tmp = tempdir().unwrap();
            let c = ctx_with_python(tmp.path(), "3.8", Some(PackageManager::Pacman));
            assert!(matches!(plan_for(DepId::FasterWhisper, &c).unwrap_err(), InstallError::PythonTooOld));
        }

        #[test]
        fn run_plan_streams_stdout_and_stderr() {
            let plan = InstallPlan {
                dep: DepId::Ffmpeg,
                steps: vec![vec!["sh".into(), "-c".into(), "echo out-line; echo err-line 1>&2".into()]],
                display_command: String::new(),
                auto_runnable: true,
            };
            let mut lines = Vec::new();
            run_plan(&plan, |l| lines.push(l.to_string())).unwrap();
            assert!(lines.iter().any(|l| l == "out-line"));
            assert!(lines.iter().any(|l| l == "err-line"));
        }

        #[test]
        fn run_plan_reports_failing_step_and_stops() {
            let plan = InstallPlan {
                dep: DepId::Ffmpeg,
                steps: vec![
                    vec!["sh".into(), "-c".into(), "exit 3".into()],
                    vec!["sh".into(), "-c".into(), "echo should-not-run".into()],
                ],
                display_command: String::new(),
                auto_runnable: true,
            };
            let mut lines = Vec::new();
            let err = run_plan(&plan, |l| lines.push(l.to_string())).unwrap_err();
            assert!(matches!(err, InstallError::StepFailed { code: Some(3), .. }));
            assert!(!lines.iter().any(|l| l == "should-not-run"));
        }

        #[test]
        fn run_plan_reports_unstartable_program() {
            let plan = InstallPlan {
                dep: DepId::Ffmpeg,
                steps: vec![vec!["/definitely/not/a/program".into()]],
                display_command: String::new(),
                auto_runnable: true,
            };
            assert!(matches!(run_plan(&plan, |_| {}).unwrap_err(), InstallError::Spawn(_)));
        }
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cd crates/subforge-deps && cargo test install`
Expected: FAIL (items undefined).

- [ ] **Step 3: Implement (above the tests in `install.rs`)**

```rust
use crate::context::Context;
use crate::detect::{probe_python, python_has_venv, venv_python_path};
use crate::hide::HideWindow;
use crate::model::DepId;
use crate::platform::{Os, PackageManager, SystemPackage};
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum InstallError {
    #[error("No supported package manager was found on this system. Use the download page to install it manually.")]
    NoPackageManager,
    #[error("Python 3.9 or newer is needed first. Install Python, then try again.")]
    PythonRequired,
    #[error("The Python on this system is older than 3.9. Install a newer Python first.")]
    PythonTooOld,
    #[error("Could not start the installer: {0}")]
    Spawn(String),
    #[error("The installer step failed (exit code {code:?}): {command}")]
    StepFailed { command: String, code: Option<i32> },
}

#[derive(Debug, Clone, PartialEq)]
pub struct InstallPlan {
    pub dep: DepId,
    /// argv lists run in order (already prefixed with `pkexec` where needed).
    pub steps: Vec<Vec<String>>,
    /// What a user would paste into a terminal.
    pub display_command: String,
    /// False when elevation is required but `pkexec` is unavailable.
    pub auto_runnable: bool,
}

struct Cmd {
    argv: Vec<String>,
    elevated: bool,
}

pub fn plan_for(id: DepId, ctx: &Context) -> Result<InstallPlan, InstallError> {
    match id {
        DepId::Mkvmerge | DepId::Mkvextract | DepId::Mkvpropedit => system_plan(id, SystemPackage::Mkvtoolnix, ctx),
        DepId::Ffmpeg | DepId::Ffprobe => system_plan(id, SystemPackage::Ffmpeg, ctx),
        DepId::Python => system_plan(id, SystemPackage::Python, ctx),
        DepId::Pysubs2 | DepId::FasterWhisper => pip_plan(id, ctx),
    }
}

fn system_plan(id: DepId, pkg: SystemPackage, ctx: &Context) -> Result<InstallPlan, InstallError> {
    let pm = ctx.package_manager.ok_or(InstallError::NoPackageManager)?;
    let cmds = pm
        .install_commands(pkg)
        .into_iter()
        .map(|argv| Cmd { argv, elevated: pm.needs_elevation() })
        .collect();
    Ok(finalize(id, cmds, ctx))
}

fn pip_plan(id: DepId, ctx: &Context) -> Result<InstallPlan, InstallError> {
    // Allowlist: only ids with a pip name reach this function by construction.
    let pip_name = id.pip_name().expect("pip_plan called for a non-pip dependency");
    let python = ctx.find_python().ok_or(InstallError::PythonRequired)?;
    if !python.meets_minimum() {
        return Err(InstallError::PythonTooOld);
    }

    let venv_py = venv_python_path(&ctx.venv_dir, ctx.os);
    let venv_ok = probe_python(&venv_py).is_some();
    let mut cmds = Vec::new();

    if !venv_ok {
        // Debian/Ubuntu ship `venv` in a separate package.
        if ctx.package_manager == Some(PackageManager::Apt) && !python_has_venv(&python.path) {
            cmds.push(Cmd {
                argv: vec!["apt-get".into(), "install".into(), "-y".into(), "python3-venv".into()],
                elevated: true,
            });
        }
        let mut argv = vec![
            python.path.to_string_lossy().into_owned(),
            "-m".into(),
            "venv".into(),
            "--system-site-packages".into(),
        ];
        if ctx.venv_dir.exists() {
            argv.push("--clear".into()); // repair a venv whose interpreter vanished
        }
        argv.push(ctx.venv_dir.to_string_lossy().into_owned());
        cmds.push(Cmd { argv, elevated: false });
    }

    cmds.push(Cmd {
        argv: vec![
            venv_py.to_string_lossy().into_owned(),
            "-m".into(),
            "pip".into(),
            "install".into(),
            "--upgrade".into(),
            pip_name.to_string(),
        ],
        elevated: false,
    });
    Ok(finalize(id, cmds, ctx))
}

fn quote(arg: &str) -> String {
    if arg.contains(' ') {
        format!("\"{arg}\"")
    } else {
        arg.to_string()
    }
}

fn finalize(id: DepId, cmds: Vec<Cmd>, ctx: &Context) -> InstallPlan {
    let mut auto_runnable = true;
    let mut steps = Vec::new();
    let mut shown = Vec::new();

    for cmd in cmds {
        let line = cmd.argv.iter().map(|a| quote(a)).collect::<Vec<_>>().join(" ");
        if cmd.elevated && ctx.os == Os::Linux {
            shown.push(format!("sudo {line}"));
            if ctx.has_pkexec {
                let mut argv = vec!["pkexec".to_string()];
                argv.extend(cmd.argv);
                steps.push(argv);
            } else {
                auto_runnable = false;
                steps.push(cmd.argv);
            }
        } else {
            shown.push(line);
            steps.push(cmd.argv);
        }
    }

    InstallPlan { dep: id, steps, display_command: shown.join(" && "), auto_runnable }
}

/// Runs every step, passing each output line (stdout and stderr) to `on_line`.
/// Stops at the first failing step.
pub fn run_plan(plan: &InstallPlan, mut on_line: impl FnMut(&str)) -> Result<(), InstallError> {
    for step in &plan.steps {
        on_line(&format!("$ {}", step.join(" ")));
        run_step(step, &mut on_line)?;
    }
    Ok(())
}

fn run_step(step: &[String], on_line: &mut impl FnMut(&str)) -> Result<(), InstallError> {
    let (program, args) = step
        .split_first()
        .ok_or_else(|| InstallError::Spawn("empty command".into()))?;

    let mut child = Command::new(program)
        .hide_window()
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| InstallError::Spawn(format!("{program}: {e}")))?;

    let (tx, rx) = mpsc::channel::<String>();
    let mut readers = Vec::new();
    if let Some(out) = child.stdout.take() {
        let tx = tx.clone();
        readers.push(thread::spawn(move || {
            for line in BufReader::new(out).lines().map_while(Result::ok) {
                if tx.send(line).is_err() {
                    break;
                }
            }
        }));
    }
    if let Some(err) = child.stderr.take() {
        let tx = tx.clone();
        readers.push(thread::spawn(move || {
            for line in BufReader::new(err).lines().map_while(Result::ok) {
                if tx.send(line).is_err() {
                    break;
                }
            }
        }));
    }
    drop(tx);

    for line in rx {
        on_line(&line);
    }
    for r in readers {
        let _ = r.join();
    }

    let status = child.wait().map_err(|e| InstallError::Spawn(e.to_string()))?;
    if status.success() {
        Ok(())
    } else {
        Err(InstallError::StepFailed { command: step.join(" "), code: status.code() })
    }
}
```

`lib.rs`: `pub mod install;` and `pub use install::{plan_for, run_plan, InstallError, InstallPlan};`

- [ ] **Step 4: Run to verify pass**

Run: `cd crates/subforge-deps && cargo test`
Expected: all PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/subforge-deps
git commit -m "feat(deps): install planning (pkexec/winget/brew/venv) and streaming runner" -m "Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>"
```

---

### Task 7: `check_all` and the manual-check example

**Files:**
- Create: `crates/subforge-deps/src/check.rs`, `crates/subforge-deps/examples/check.rs`
- Modify: `crates/subforge-deps/src/lib.rs`

**Interfaces:**
- Consumes: everything above.
- Produces: `check_all(&Context) -> DepReport`.

- [ ] **Step 1: Write the failing tests**

Bottom of `crates/subforge-deps/src/check.rs`:
```rust
#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::detect::SearchEnv;
    use crate::model::{DepState, Severity};
    use crate::platform::{Os, PackageManager};
    use crate::testutil::make_script;
    use tempfile::tempdir;

    fn ctx(dir: &std::path::Path) -> Context {
        Context {
            os: Os::Linux,
            env: SearchEnv { dirs: vec![dir.to_path_buf()], os: Os::Linux },
            venv_dir: dir.join("no-venv"),
            package_manager: Some(PackageManager::Pacman),
            has_pkexec: true,
        }
    }

    fn item(report: &DepReport, id: DepId) -> &crate::model::DepStatus {
        report.items.iter().find(|i| i.id == id).unwrap()
    }

    fn install_all_tools(dir: &std::path::Path) {
        for t in ["mkvmerge", "mkvextract", "ffmpeg", "ffprobe"] {
            make_script(dir, t, &format!("echo '{t} version 1.0'"));
        }
        make_script(dir, "python3", r#"case "$*" in *faster-whisper*) exit 1;; esac; echo 3.11"#);
    }

    #[test]
    fn healthy_system_has_no_missing_required() {
        let tmp = tempdir().unwrap();
        install_all_tools(tmp.path());
        let report = check_all(&ctx(tmp.path()));

        assert!(!report.has_missing_required);
        assert_eq!(report.os, "linux");
        assert_eq!(report.items.len(), 8);

        let mkv = item(&report, DepId::Mkvmerge);
        assert_eq!(mkv.state, DepState::Ok);
        assert_eq!(mkv.version.as_deref(), Some("mkvmerge version 1.0"));
        assert!(mkv.path.is_some());
        assert!(mkv.install_command.is_none());

        // Optional gaps are reported but do not count as missing-required.
        let whisper = item(&report, DepId::FasterWhisper);
        assert_eq!(whisper.state, DepState::Missing);
        assert_eq!(whisper.severity, Severity::Optional);
        assert_eq!(item(&report, DepId::Mkvpropedit).state, DepState::Missing);
    }

    #[test]
    fn missing_tool_gets_exact_install_command() {
        let tmp = tempdir().unwrap();
        install_all_tools(tmp.path());
        std::fs::remove_file(tmp.path().join("ffmpeg")).unwrap();
        let report = check_all(&ctx(tmp.path()));

        assert!(report.has_missing_required);
        let ff = item(&report, DepId::Ffmpeg);
        assert_eq!(ff.state, DepState::Missing);
        assert_eq!(ff.install_command.as_deref(), Some("sudo pacman -S --needed --noconfirm ffmpeg"));
        assert!(ff.can_auto_install);
        assert_eq!(ff.download_url, "https://ffmpeg.org/download.html");
    }

    #[test]
    fn missing_python_blocks_package_auto_install() {
        let tmp = tempdir().unwrap();
        let report = check_all(&ctx(tmp.path()));
        assert_eq!(item(&report, DepId::Python).state, DepState::Missing);
        let p = item(&report, DepId::Pysubs2);
        assert_eq!(p.state, DepState::Missing);
        assert!(!p.can_auto_install);
        assert!(p.install_command.is_none());
        // ...but Python itself can be installed.
        assert!(item(&report, DepId::Python).can_auto_install);
    }

    #[test]
    fn old_python_is_reported_outdated() {
        let tmp = tempdir().unwrap();
        make_script(tmp.path(), "python3", "echo 3.8");
        let report = check_all(&ctx(tmp.path()));
        let py = item(&report, DepId::Python);
        assert_eq!(py.state, DepState::Outdated);
        assert_eq!(py.version.as_deref(), Some("3.8"));
        assert!(report.has_missing_required);
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cd crates/subforge-deps && cargo test check`
Expected: FAIL (`check_all` undefined).

- [ ] **Step 3: Implement (above the tests in `check.rs`)**

```rust
use crate::context::Context;
use crate::detect::{python_package_version, tool_version, PythonInfo};
use crate::install::plan_for;
use crate::model::{DepId, DepReport, DepState, DepStatus};

pub fn check_all(ctx: &Context) -> DepReport {
    let python = ctx.find_python();
    let items = DepId::ALL
        .into_iter()
        .map(|id| check_one(ctx, id, python.as_ref()))
        .collect();
    DepReport::new(ctx.os.as_str().to_string(), items)
}

fn check_one(ctx: &Context, id: DepId, python: Option<&PythonInfo>) -> DepStatus {
    let spec = id.spec();

    let (state, path, version) = match id {
        DepId::Python => match python {
            Some(info) => (
                if info.meets_minimum() { DepState::Ok } else { DepState::Outdated },
                Some(info.path.to_string_lossy().into_owned()),
                Some(info.version_string()),
            ),
            None => (DepState::Missing, None, None),
        },
        DepId::Pysubs2 | DepId::FasterWhisper => {
            let dist = id.pip_name().expect("python package without a pip name");
            match python.and_then(|p| python_package_version(&p.path, dist)) {
                Some(v) => (DepState::Ok, None, Some(v)),
                None => (DepState::Missing, None, None),
            }
        }
        _ => match ctx.find_executable(id.as_str()) {
            Some(p) => {
                let version = tool_version(&p, id.version_arg());
                (DepState::Ok, Some(p.to_string_lossy().into_owned()), version)
            }
            None => (DepState::Missing, None, None),
        },
    };

    let plan = if state == DepState::Ok { None } else { plan_for(id, ctx).ok() };

    DepStatus {
        id,
        label: spec.label.to_string(),
        severity: spec.severity,
        state,
        path,
        version,
        purpose: spec.purpose.to_string(),
        download_url: spec.download_url.to_string(),
        install_command: plan.as_ref().map(|p| p.display_command.clone()),
        can_auto_install: plan.as_ref().map(|p| p.auto_runnable).unwrap_or(false),
    }
}
```

`lib.rs`: `pub mod check;` and `pub use check::check_all;`

`crates/subforge-deps/examples/check.rs`:
```rust
//! Prints the dependency report for this machine as JSON.
//! Run with: `cargo run --example check` (from crates/subforge-deps).

use subforge_deps::{check_all, Context};

fn main() {
    let report = check_all(&Context::from_process());
    println!("{}", serde_json::to_string_pretty(&report).expect("report serializes"));
}
```
(`serde_json` is already a dev-dependency, which examples may use.)

- [ ] **Step 4: Run to verify pass, then try it for real**

Run: `cd crates/subforge-deps && cargo test && cargo run --example check`
Expected: tests PASS; JSON report printed. On this machine `mkvmerge`/`ffmpeg`/`python`/`pysubs2` should be `"ok"`.

- [ ] **Step 5: Commit**

```bash
git add crates/subforge-deps
git commit -m "feat(deps): check_all report with per-item install commands" -m "Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>"
```

---

### Task 8: Wire the crate into `src-tauri`

**Files:**
- Modify: `src-tauri/Cargo.toml`, `src-tauri/src/tools/mod.rs`, `src-tauri/src/commands/mod.rs`, `src-tauri/src/lib.rs`, plus the `Command::new` sites listed in Step 3.
- Create: `src-tauri/src/commands/deps.rs`

**Interfaces:**
- Consumes: `subforge_deps::{check_all, plan_for, run_plan, Context, DepId, DepReport, HideWindow, SearchEnv, find_executable, locate_script}`.
- Produces: Tauri commands `check_dependencies() -> DepReport` and `install_dependency(id: String) -> DepReport`; events `dep-install-progress` `{ job_id, message }` and `dep-install-done` `{ job_id, status, message }`.

- [ ] **Step 1: Add the dependency**

In `src-tauri/Cargo.toml` under `[dependencies]` (after `subforge-metadata`):
```toml
subforge-deps = { path = "../crates/subforge-deps", features = ["tokio"] }
```

- [ ] **Step 2: Delegate `which`, `find_python`, and `script_path`**

In `src-tauri/src/tools/mod.rs` replace the bodies of `which` and `find_python`:
```rust
pub fn which(cmd: &str) -> Option<PathBuf> {
    subforge_deps::find_executable(cmd, &subforge_deps::SearchEnv::from_process())
}

pub fn find_python() -> Option<PathBuf> {
    subforge_deps::Context::from_process().find_python().map(|info| info.path)
}
```
and add at the very top of `script_path` (before the "1. Next to the binary" comment):
```rust
    if let Some(p) = subforge_deps::locate_script(name) {
        return p.to_string_lossy().to_string();
    }
```

- [ ] **Step 3: Hide console windows on Windows**

Run from the repo root:
```bash
perl -0pi -e 's/(?<![\w:])((?:std::process::|tokio::process::)?Command::new\([^()]*\))/$1.hide_window()/g' \
  src-tauri/src/jobs/mod.rs src-tauri/src/tools/ffmpeg.rs src-tauri/src/tools/mkvtoolnix.rs \
  src-tauri/src/tools/pysubs2.rs src-tauri/src/tools/translate.rs src-tauri/src/tools/whisper.rs \
  src-tauri/src/commands/subtitle.rs src-tauri/src/commands/transcribe.rs
for f in $(grep -l "hide_window" src-tauri/src -r); do sed -i '1i use subforge_deps::HideWindow;' "$f"; done
git diff --stat
```
Expected: the eight files modified, each `Command::new(..)` now followed by `.hide_window()`.

- [ ] **Step 4: Create the Tauri commands**

`src-tauri/src/commands/deps.rs`:
```rust
use subforge_deps::{check_all, plan_for, run_plan, Context, DepId, DepReport};
use tauri::{AppHandle, Emitter};

#[tauri::command]
pub async fn check_dependencies() -> Result<DepReport, String> {
    tokio::task::spawn_blocking(|| check_all(&Context::from_process()))
        .await
        .map_err(|e| e.to_string())
}

/// Installs one dependency, streaming installer output as `dep-install-progress`
/// events, and returns a fresh report so the UI updates immediately.
#[tauri::command]
pub async fn install_dependency(app: AppHandle, id: String) -> Result<DepReport, String> {
    let dep = DepId::parse(&id).ok_or_else(|| format!("Unknown dependency '{id}'"))?;

    tokio::task::spawn_blocking(move || {
        let ctx = Context::from_process();
        let plan = plan_for(dep, &ctx).map_err(|e| e.to_string())?;
        if !plan.auto_runnable {
            return Err(format!(
                "This needs administrator rights and no graphical password prompt (pkexec) was found. Run this in a terminal: {}",
                plan.display_command
            ));
        }

        let result = run_plan(&plan, |line| {
            let _ = app.emit(
                "dep-install-progress",
                serde_json::json!({ "job_id": id, "message": line }),
            );
        });

        let ok = result.is_ok();
        let message = match &result {
            Ok(()) => "Installed".to_string(),
            Err(e) => e.to_string(),
        };
        let _ = app.emit(
            "dep-install-done",
            serde_json::json!({
                "job_id": id,
                "status": if ok { "completed" } else { "failed" },
                "message": message,
            }),
        );

        result.map_err(|e| e.to_string())?;
        // Fresh context: PATH-independent lookups now see the new install.
        Ok(check_all(&Context::from_process()))
    })
    .await
    .map_err(|e| e.to_string())?
}
```

In `src-tauri/src/commands/mod.rs` add `pub mod deps;`. In `src-tauri/src/lib.rs`:
- extend the `use commands::{...}` list with `deps as deps_cmds,`
- add to `generate_handler![...]`:
```rust
            deps_cmds::check_dependencies,
            deps_cmds::install_dependency,
```

- [ ] **Step 5: Build and test**

Run: `cd src-tauri && SUBFORGE_SUBSOURCE_API_KEY=ci-placeholder-key cargo check && SUBFORGE_SUBSOURCE_API_KEY=ci-placeholder-key cargo test`
Expected: compiles; existing tests PASS. If a `Command::new(...)` site no longer type-checks (e.g. it was bound to a variable and mutated later), change that site to `let mut cmd = Command::new(..); cmd.hide_window();` and continue using `cmd`.

- [ ] **Step 6: Commit**

```bash
git add src-tauri crates
git commit -m "feat(tauri): dependency check/install commands; delegate which/python/scripts to subforge-deps" -m "Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>"
```

---

### Task 9: Wire the crate into `server`

**Files:**
- Modify: `server/Cargo.toml`, `server/src/tools/mod.rs`, `server/src/tools/pysubs2.rs`, `server/src/error.rs`, `server/src/main.rs`, `server/src/routes/mod.rs`, plus `Command::new` sites.
- Create: `server/src/routes/deps.rs`

**Interfaces:**
- Consumes: `subforge_deps::*`, `crate::jobs::{emit_event, JobEvent}`, `crate::AppState`, `crate::error::{blocking, AppError, err400, err500}`.
- Produces: `GET /api/deps -> DepReport`; `POST /api/deps/install` body `{ "id": "<dep id>" }` -> `DepReport` (403 unless `SUBFORGE_ALLOW_INSTALL=1`); WebSocket events `dep-install-progress` / `dep-install-done` with `payload.job_id` = dep id and `payload.message`.

- [ ] **Step 1: Add the dependency**

In `server/Cargo.toml` `[dependencies]`:
```toml
subforge-deps = { path = "../crates/subforge-deps", features = ["tokio"] }
```

- [ ] **Step 2: Delegate shared helpers**

In `server/src/tools/mod.rs`, replace `which` and `find_python` exactly as in Task 8 Step 2, and add the `locate_script` early-return at the top of `script_path`.

In `server/src/tools/pysubs2.rs`: delete the local `fn find_python() -> Option<PathBuf> { ... }` (the `PATH` splitting on `':'`, which is wrong on Windows) and change `get_python` to:
```rust
fn get_python() -> Result<PathBuf, Pysubs2Error> {
    super::find_python().ok_or(Pysubs2Error::PythonNotFound)
}
```

- [ ] **Step 3: Hide console windows**

```bash
perl -0pi -e 's/(?<![\w:])((?:std::process::|tokio::process::)?Command::new\([^()]*\))/$1.hide_window()/g' \
  server/src/tools/ffmpeg.rs server/src/tools/mkvtoolnix.rs server/src/tools/pysubs2.rs \
  server/src/tools/translate.rs server/src/tools/whisper.rs \
  server/src/routes/subtitle.rs server/src/routes/transcribe.rs
for f in $(grep -l "hide_window" server/src -r); do sed -i '1i use subforge_deps::HideWindow;' "$f"; done
```

- [ ] **Step 4: Add a 403 helper**

In `server/src/error.rs`, after `err400`:
```rust
pub fn err403(msg: impl ToString) -> AppError {
    AppError(StatusCode::FORBIDDEN, msg.to_string())
}
```

- [ ] **Step 5: Create the routes**

`server/src/routes/deps.rs`:
```rust
use crate::error::{blocking, err400, err403, err500, AppError};
use crate::{jobs, AppState};
use axum::{extract::State, Json};
use serde::Deserialize;
use subforge_deps::{check_all, plan_for, run_plan, Context, DepId, DepReport};

pub async fn get_deps() -> Result<Json<DepReport>, AppError> {
    let report = blocking(|| check_all(&Context::from_process()))
        .await
        .map_err(|_| err500("dependency check failed"))?;
    Ok(Json(report))
}

#[derive(Deserialize)]
pub struct InstallRequest {
    pub id: String,
}

fn emit(sender: &jobs::EventSender, event_type: &str, id: &str, status: Option<&str>, message: String) {
    jobs::emit_event(
        sender,
        jobs::JobEvent {
            event_type: event_type.to_string(),
            job_id: id.to_string(),
            status: status.map(str::to_string),
            progress: None,
            message: Some(message),
            stage: None,
            output_path: None,
        },
    );
}

/// Installs software on the machine running the server, so it must be switched
/// on explicitly with `SUBFORGE_ALLOW_INSTALL=1`.
pub async fn install_dep(
    State(state): State<AppState>,
    Json(req): Json<InstallRequest>,
) -> Result<Json<DepReport>, AppError> {
    let dep = DepId::parse(&req.id).ok_or_else(|| err400(format!("Unknown dependency '{}'", req.id)))?;

    let ctx = Context::from_process();
    let plan = plan_for(dep, &ctx).map_err(|e| err400(e.to_string()))?;

    if std::env::var("SUBFORGE_ALLOW_INSTALL").as_deref() != Ok("1") {
        return Err(err403(format!(
            "Installing software from the web UI is disabled on this server. Run this on the server, or start it with SUBFORGE_ALLOW_INSTALL=1: {}",
            plan.display_command
        )));
    }
    if !plan.auto_runnable {
        return Err(err400(format!(
            "This needs administrator rights and no graphical password prompt (pkexec) was found. Run this in a terminal: {}",
            plan.display_command
        )));
    }

    let sender = state.event_sender.clone();
    let id = req.id.clone();
    let result = blocking(move || {
        let result = run_plan(&plan, |line| emit(&sender, "dep-install-progress", &id, None, line.to_string()));
        let (status, message) = match &result {
            Ok(()) => ("completed", "Installed".to_string()),
            Err(e) => ("failed", e.to_string()),
        };
        emit(&sender, "dep-install-done", &id, Some(status), message);
        result.map(|_| check_all(&Context::from_process()))
    })
    .await
    .map_err(|_| err500("install task crashed"))?;

    result.map(Json).map_err(|e| err500(e.to_string()))
}
```

In `server/src/routes/mod.rs` add `pub mod deps;` (keep alphabetical: after `config`). In `server/src/main.rs` `api_routes()`, after the tools registry route:
```rust
        // ── Dependencies (startup check + one-click install) ─────────────────
        .route("/deps", get(routes::deps::get_deps))
        .route("/deps/install", post(routes::deps::install_dep))
```

- [ ] **Step 6: Build and test**

Run: `cd server && SUBFORGE_SUBSOURCE_API_KEY=ci-placeholder-key cargo check && SUBFORGE_SUBSOURCE_API_KEY=ci-placeholder-key cargo test`
Expected: compiles; existing tests PASS. Fix any non-chaining `Command::new` sites as described in Task 8 Step 5. If `AppError` is not importable as `crate::error::AppError`, make it `pub` in `error.rs` (it is already `pub struct`).

- [ ] **Step 7: Smoke-test the endpoint**

Run:
```bash
cd server && SUBFORGE_SUBSOURCE_API_KEY=ci-placeholder-key cargo run --release &
sleep 20; curl -s localhost:8080/api/deps | head -c 400; echo
curl -s -X POST localhost:8080/api/deps/install -H 'content-type: application/json' -d '{"id":"ffmpeg"}'; echo
curl -s -X POST localhost:8080/api/deps/install -H 'content-type: application/json' -d '{"id":"x; rm -rf /"}'; echo
kill %1
```
Expected: a JSON report; a 403 message (install disabled); a 400 "Unknown dependency".

- [ ] **Step 8: Commit**

```bash
git add server crates
git commit -m "feat(server): /api/deps check and gated install endpoint; reuse subforge-deps" -m "Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>"
```

---

### Task 10: Frontend data layer (API, store, helpers)

**Files:**
- Modify: `src/api/client.ts`, `src/api/schemas.ts`
- Create: `src/stores/depsStore.ts`, `src/stores/depsStore.test.ts`, `src/utils/openExternal.ts`

**Interfaces:**
- Consumes: `apiInvoke` (`check_dependencies`, `install_dependency`), `apiListen("dep-install-progress")`.
- Produces: types `DepStatus`, `DepReport`; store `useDepsStore()` with state `report`, `checking`, `installing`, `logs`, `errors`, `dialogDismissed`; getters `missing`, `missingRequired`, `showDialog`, `showBanner`; actions `check()`, `install(id): Promise<boolean>`, `installAll()`, `dismiss()`, `reopen()`, `listen(): Promise<() => void>`. `openExternal(url: string): Promise<void>`.

- [ ] **Step 1: Write the failing store tests**

`src/stores/depsStore.test.ts`:
```ts
import { describe, it, expect, vi, beforeEach } from "vitest";
import { setActivePinia, createPinia } from "pinia";
import { useDepsStore, type DepReport, type DepStatus } from "./depsStore";

vi.mock("../api/client", () => ({ apiInvoke: vi.fn() }));
vi.mock("../api/events", () => ({ apiListen: vi.fn() }));

import { apiInvoke } from "../api/client";
import { apiListen } from "../api/events";

function item(over: Partial<DepStatus> & Pick<DepStatus, "id">): DepStatus {
  return {
    label: over.id,
    severity: "required",
    state: "ok",
    purpose: "",
    download_url: "https://example.com",
    can_auto_install: false,
    ...over,
  };
}

function report(items: DepStatus[]): DepReport {
  return {
    os: "linux",
    items,
    has_missing_required: items.some((i) => i.severity === "required" && i.state !== "ok"),
  };
}

describe("depsStore", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    vi.clearAllMocks();
  });

  it("check stores the report and opens the dialog when required deps are missing", async () => {
    vi.mocked(apiInvoke).mockResolvedValue(report([item({ id: "ffmpeg", state: "missing" })]));
    const s = useDepsStore();
    await s.check();
    expect(apiInvoke).toHaveBeenCalledWith("check_dependencies");
    expect(s.report?.items).toHaveLength(1);
    expect(s.showDialog).toBe(true);
    expect(s.showBanner).toBe(false);
  });

  it("does not open for optional-only gaps", async () => {
    vi.mocked(apiInvoke).mockResolvedValue(
      report([item({ id: "faster_whisper", severity: "optional", state: "missing" })])
    );
    const s = useDepsStore();
    await s.check();
    expect(s.showDialog).toBe(false);
    expect(s.showBanner).toBe(false);
  });

  it("dismiss swaps the dialog for the banner and reopen reverses it", async () => {
    vi.mocked(apiInvoke).mockResolvedValue(report([item({ id: "ffmpeg", state: "missing" })]));
    const s = useDepsStore();
    await s.check();
    s.dismiss();
    expect(s.showDialog).toBe(false);
    expect(s.showBanner).toBe(true);
    s.reopen();
    expect(s.showDialog).toBe(true);
  });

  it("check failure leaves report null without throwing", async () => {
    vi.mocked(apiInvoke).mockRejectedValue(new Error("offline"));
    const s = useDepsStore();
    await s.check();
    expect(s.report).toBeNull();
    expect(s.showDialog).toBe(false);
  });

  it("install updates the report from the reply and clears installing", async () => {
    vi.mocked(apiInvoke)
      .mockResolvedValueOnce(report([item({ id: "ffmpeg", state: "missing", can_auto_install: true })]))
      .mockResolvedValueOnce(report([item({ id: "ffmpeg", state: "ok" })]));
    const s = useDepsStore();
    await s.check();
    const ok = await s.install("ffmpeg");
    expect(ok).toBe(true);
    expect(apiInvoke).toHaveBeenLastCalledWith("install_dependency", { id: "ffmpeg" });
    expect(s.report?.has_missing_required).toBe(false);
    expect(s.installing).toBeNull();
  });

  it("install failure records the message (string or Error) and re-checks", async () => {
    vi.mocked(apiInvoke)
      .mockResolvedValueOnce(report([item({ id: "ffmpeg", state: "missing", can_auto_install: true })]))
      .mockRejectedValueOnce("pkexec was dismissed")
      .mockResolvedValueOnce(report([item({ id: "ffmpeg", state: "missing", can_auto_install: true })]));
    const s = useDepsStore();
    await s.check();
    const ok = await s.install("ffmpeg");
    expect(ok).toBe(false);
    expect(s.errors["ffmpeg"]).toBe("pkexec was dismissed");
    expect(s.installing).toBeNull();
  });

  it("installAll installs in order and picks up items unlocked by earlier installs", async () => {
    vi.mocked(apiInvoke)
      // initial check: python missing (auto), pysubs2 missing (not yet auto)
      .mockResolvedValueOnce(
        report([
          item({ id: "python", state: "missing", can_auto_install: true }),
          item({ id: "pysubs2", state: "missing", can_auto_install: false }),
        ])
      )
      // after installing python, pysubs2 becomes installable
      .mockResolvedValueOnce(
        report([
          item({ id: "python", state: "ok" }),
          item({ id: "pysubs2", state: "missing", can_auto_install: true }),
        ])
      )
      // after installing pysubs2 everything is ok
      .mockResolvedValueOnce(report([item({ id: "python", state: "ok" }), item({ id: "pysubs2", state: "ok" })]));
    const s = useDepsStore();
    await s.check();
    await s.installAll();
    const installs = vi.mocked(apiInvoke).mock.calls.filter((c) => c[0] === "install_dependency");
    expect(installs.map((c) => (c[1] as { id: string }).id)).toEqual(["python", "pysubs2"]);
    expect(s.report?.has_missing_required).toBe(false);
  });

  it("installAll stops at the first failure", async () => {
    vi.mocked(apiInvoke)
      .mockResolvedValueOnce(
        report([
          item({ id: "mkvmerge", state: "missing", can_auto_install: true }),
          item({ id: "ffmpeg", state: "missing", can_auto_install: true }),
        ])
      )
      .mockRejectedValueOnce(new Error("boom"))
      .mockResolvedValueOnce(
        report([
          item({ id: "mkvmerge", state: "missing", can_auto_install: true }),
          item({ id: "ffmpeg", state: "missing", can_auto_install: true }),
        ])
      );
    const s = useDepsStore();
    await s.check();
    await s.installAll();
    const installs = vi.mocked(apiInvoke).mock.calls.filter((c) => c[0] === "install_dependency");
    expect(installs).toHaveLength(1);
  });

  it("listen appends installer output to the per-dependency log", async () => {
    let handler: (p: { job_id: string; message?: string }) => void = () => {};
    vi.mocked(apiListen).mockImplementation(async (_event, h) => {
      handler = h as typeof handler;
      return () => {};
    });
    const s = useDepsStore();
    await s.listen();
    handler({ job_id: "ffmpeg", message: "downloading" });
    handler({ job_id: "ffmpeg", message: "done" });
    expect(s.logs["ffmpeg"]).toEqual(["downloading", "done"]);
  });
});
```

- [ ] **Step 2: Run to verify failure**

Run: `npx vitest run src/stores/depsStore.test.ts`
Expected: FAIL (cannot resolve `./depsStore`).

- [ ] **Step 3: Implement the store**

`src/stores/depsStore.ts`:
```ts
import { defineStore } from "pinia";
import { computed, ref } from "vue";
import { apiInvoke } from "../api/client";
import { apiListen } from "../api/events";

export type DepState = "ok" | "missing" | "outdated";
export type DepSeverity = "required" | "optional";

export interface DepStatus {
  id: string;
  label: string;
  severity: DepSeverity;
  state: DepState;
  path?: string | null;
  version?: string | null;
  purpose: string;
  download_url: string;
  install_command?: string | null;
  can_auto_install: boolean;
}

export interface DepReport {
  os: string;
  items: DepStatus[];
  has_missing_required: boolean;
}

function messageOf(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

export const useDepsStore = defineStore("deps", () => {
  const report = ref<DepReport | null>(null);
  const checking = ref(false);
  const installing = ref<string | null>(null);
  const logs = ref<Record<string, string[]>>({});
  const errors = ref<Record<string, string>>({});
  const dialogDismissed = ref(false);

  const missing = computed(() => report.value?.items.filter((i) => i.state !== "ok") ?? []);
  const missingRequired = computed(() => missing.value.filter((i) => i.severity === "required"));
  const showDialog = computed(() => !!report.value?.has_missing_required && !dialogDismissed.value);
  const showBanner = computed(() => !!report.value?.has_missing_required && dialogDismissed.value);

  async function check() {
    checking.value = true;
    try {
      report.value = await apiInvoke<DepReport>("check_dependencies");
    } catch {
      // Backend unreachable: leave the previous report (or none) in place.
    } finally {
      checking.value = false;
    }
  }

  async function install(id: string): Promise<boolean> {
    installing.value = id;
    logs.value[id] = [];
    delete errors.value[id];
    try {
      report.value = await apiInvoke<DepReport>("install_dependency", { id });
      return true;
    } catch (e) {
      errors.value[id] = messageOf(e);
      await check();
      return false;
    } finally {
      installing.value = null;
    }
  }

  // Re-evaluates after every install: installing Python unlocks the pip packages.
  async function installAll() {
    const tried = new Set<string>();
    for (;;) {
      const next = missing.value.find((i) => i.can_auto_install && !tried.has(i.id));
      if (!next) return;
      tried.add(next.id);
      if (!(await install(next.id))) return;
    }
  }

  function dismiss() {
    dialogDismissed.value = true;
  }

  function reopen() {
    dialogDismissed.value = false;
  }

  async function listen(): Promise<() => void> {
    return apiListen<{ job_id: string; message?: string }>("dep-install-progress", (p) => {
      (logs.value[p.job_id] ??= []).push(p.message ?? "");
    });
  }

  return {
    report,
    checking,
    installing,
    logs,
    errors,
    dialogDismissed,
    missing,
    missingRequired,
    showDialog,
    showBanner,
    check,
    install,
    installAll,
    dismiss,
    reopen,
    listen,
  };
});
```

- [ ] **Step 4: API plumbing and link helper**

In `src/api/client.ts`:
- change `const GET_COMMANDS = new Set(["load_config", "get_tool_registry"]);` to
  `const GET_COMMANDS = new Set(["load_config", "get_tool_registry", "check_dependencies"]);`
- in `cmdToEndpoint`'s `map`, after `get_tool_registry: "/api/tools/registry",` add:
```ts
    check_dependencies: "/api/deps",
    install_dependency: "/api/deps/install",
```

In `src/api/schemas.ts`, after `ToolInfoSchema`:
```ts
const DepStatusSchema = z.looseObject({
  id: z.string(),
  label: z.string(),
  severity: z.enum(["required", "optional"]),
  state: z.enum(["ok", "missing", "outdated"]),
  path: z.string().nullish(),
  version: z.string().nullish(),
  purpose: z.string(),
  download_url: z.string(),
  install_command: z.string().nullish(),
  can_auto_install: z.boolean(),
});

const DepReportSchema = z.looseObject({
  os: z.string(),
  items: z.array(DepStatusSchema),
  has_missing_required: z.boolean(),
});
```
and inside `commandSchemas` after `get_tool_registry`:
```ts
  check_dependencies: DepReportSchema,
  install_dependency: DepReportSchema,
```

`src/utils/openExternal.ts`:
```ts
import { isTauri } from "../api/client";

/** Opens a link in the user's default browser (desktop) or a new tab (web UI). */
export async function openExternal(url: string): Promise<void> {
  if (isTauri()) {
    const { openUrl } = await import("@tauri-apps/plugin-opener");
    await openUrl(url);
  } else {
    window.open(url, "_blank", "noopener");
  }
}
```

- [ ] **Step 5: Run to verify pass**

Run: `npx vitest run src/stores/depsStore.test.ts && npx vue-tsc --noEmit`
Expected: 9 tests PASS; type-check clean.

- [ ] **Step 6: Commit**

```bash
git add src
git commit -m "feat(ui): dependency store, API mapping and external-link helper" -m "Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>"
```

---

### Task 11: Dependency UI (list, dialog, banner, settings panel, i18n)

**Files:**
- Create: `src/components/{DependencyList,DependencyDialog,DependencyBanner,DependenciesPanel}.vue`, `src/components/DependencyList.test.ts`, `src/components/DependencyDialog.test.ts`
- Modify: `src/i18n/en.json`, `src/i18n/ar.json`, `src/App.vue`, `src/views/SettingsView.vue`

**Interfaces:**
- Consumes: `useDepsStore`, `openExternal`, `Modal.vue` (`open`, `title`, slots `default`/`footer`, emits `close`).
- Produces: `<DependencyList :items :installing :logs :errors @install="id" />` with `data-testid`s `status-<id>`, `install-<id>`, `copy-<id>`, `download-<id>`, `error-<id>`; `<DependencyDialog />` (`data-testid="deps-dialog"`), `<DependencyBanner />`, `<DependenciesPanel />`.

- [ ] **Step 1: Add translations**

Create a scratch script and run it (it merges a `deps` block into both locale files, preserving key order):
```bash
node -e '
const fs = require("fs");
const en = {
  title: "Required Programs",
  intro: "SubForge needs a few free programs to do its job. Anything missing is listed below. Click Install and SubForge will set it up for you, or use the download link.",
  checking: "Checking your system…",
  allGood: "Everything SubForge needs is installed.",
  required: "Required",
  optional: "Optional",
  state: { ok: "Installed", missing: "Missing", outdated: "Too old" },
  install: "Install",
  installing: "Installing…",
  installAll: "Install all missing",
  recheck: "Re-check",
  later: "Continue anyway",
  copy: "Copy command",
  copied: "Copied",
  download: "Download page",
  command: "Or run this in a terminal:",
  manualHint: "This needs administrator rights, and no graphical password prompt (pkexec) was found. Run the command above in a terminal.",
  needPython: "Install Python first, then this can be installed automatically.",
  details: "Details",
  bannerText: "Some required programs are missing, so parts of SubForge will not work.",
  fixNow: "Fix now",
  purpose: {
    mkvmerge: "Merging subtitle and audio tracks into MKV files",
    mkvextract: "Extracting subtitle and audio tracks from MKV files",
    mkvpropedit: "Editing track names, languages and flags without remuxing",
    ffmpeg: "Extracting, converting and merging audio",
    ffprobe: "Reading audio track information",
    python: "Running the subtitle conversion, translation and transcription helpers",
    pysubs2: "Converting and retiming subtitle formats (SRT, ASS, VTT)",
    faster_whisper: "Speech-to-text transcription"
  }
};
const ar = {
  title: "البرامج المطلوبة",
  intro: "يحتاج SubForge إلى بعض البرامج المجانية ليعمل. البرامج الناقصة مذكورة أدناه. اضغط «تثبيت» وسيقوم SubForge بتثبيتها لك، أو استخدم رابط التنزيل.",
  checking: "جارٍ فحص جهازك…",
  allGood: "كل ما يحتاجه SubForge مثبّت.",
  required: "مطلوب",
  optional: "اختياري",
  state: { ok: "مثبّت", missing: "ناقص", outdated: "قديم جدًا" },
  install: "تثبيت",
  installing: "جارٍ التثبيت…",
  installAll: "تثبيت كل الناقص",
  recheck: "إعادة الفحص",
  later: "المتابعة على أي حال",
  copy: "نسخ الأمر",
  copied: "تم النسخ",
  download: "صفحة التنزيل",
  command: "أو نفّذ هذا الأمر في الطرفية:",
  manualHint: "يتطلب هذا صلاحيات المسؤول ولم يتم العثور على نافذة كلمة مرور رسومية (pkexec). نفّذ الأمر أعلاه في الطرفية.",
  needPython: "ثبّت بايثون أولًا، ثم يمكن تثبيت هذا تلقائيًا.",
  details: "التفاصيل",
  bannerText: "بعض البرامج المطلوبة ناقصة، لذلك لن تعمل بعض أجزاء SubForge.",
  fixNow: "إصلاح الآن",
  purpose: {
    mkvmerge: "دمج مسارات الترجمة والصوت في ملفات MKV",
    mkvextract: "استخراج مسارات الترجمة والصوت من ملفات MKV",
    mkvpropedit: "تعديل أسماء المسارات ولغاتها وخصائصها دون إعادة الدمج",
    ffmpeg: "استخراج الصوت وتحويله ودمجه",
    ffprobe: "قراءة معلومات المسارات الصوتية",
    python: "تشغيل أدوات تحويل الترجمة والترجمة الآلية والتفريغ النصي",
    pysubs2: "تحويل صيغ الترجمة (SRT وASS وVTT) وضبط توقيتها",
    faster_whisper: "تحويل الكلام إلى نص"
  }
};
for (const [file, block] of [["src/i18n/en.json", en], ["src/i18n/ar.json", ar]]) {
  const json = JSON.parse(fs.readFileSync(file, "utf8"));
  json.deps = block;
  fs.writeFileSync(file, JSON.stringify(json, null, 2) + "\n");
}
'
git diff --stat src/i18n
```
Expected: both files gain a `deps` block (check `git diff` shows only the addition; if the files had a different indentation/trailing newline, the diff will be larger — that is acceptable only if no keys changed, verify with `node -e` that key sets are equal apart from `deps`).

- [ ] **Step 2: Write the failing component tests**

`src/components/DependencyList.test.ts`:
```ts
import { describe, it, expect } from "vitest";
import { mount } from "@vue/test-utils";
import { i18n } from "../i18n";
import DependencyList from "./DependencyList.vue";
import type { DepStatus } from "../stores/depsStore";

function item(over: Partial<DepStatus> & Pick<DepStatus, "id">): DepStatus {
  return {
    label: over.id,
    severity: "required",
    state: "ok",
    purpose: "backend purpose",
    download_url: "https://example.com/dl",
    can_auto_install: false,
    ...over,
  };
}

function mountList(items: DepStatus[], extra: Record<string, unknown> = {}) {
  return mount(DependencyList, {
    props: { items, installing: null, logs: {}, errors: {}, ...extra },
    global: { plugins: [i18n] },
  });
}

describe("DependencyList", () => {
  it("shows an installed item without any actions", () => {
    const w = mountList([item({ id: "ffmpeg", version: "ffmpeg 7.1" })]);
    expect(w.get('[data-testid="status-ffmpeg"]').text()).toBe("Installed");
    expect(w.find('[data-testid="install-ffmpeg"]').exists()).toBe(false);
    expect(w.find('[data-testid="download-ffmpeg"]').exists()).toBe(false);
  });

  it("offers Install, Copy and Download for a missing auto-installable item", async () => {
    const w = mountList([
      item({ id: "ffmpeg", state: "missing", can_auto_install: true, install_command: "sudo pacman -S ffmpeg" }),
    ]);
    expect(w.get('[data-testid="status-ffmpeg"]').text()).toBe("Missing");
    expect(w.text()).toContain("sudo pacman -S ffmpeg");
    expect(w.find('[data-testid="copy-ffmpeg"]').exists()).toBe(true);
    expect(w.find('[data-testid="download-ffmpeg"]').exists()).toBe(true);
    await w.get('[data-testid="install-ffmpeg"]').trigger("click");
    expect(w.emitted("install")).toEqual([["ffmpeg"]]);
  });

  it("hides Install and explains the manual step when auto-install is impossible", () => {
    const w = mountList([
      item({ id: "ffmpeg", state: "missing", can_auto_install: false, install_command: "sudo pacman -S ffmpeg" }),
    ]);
    expect(w.find('[data-testid="install-ffmpeg"]').exists()).toBe(false);
    expect(w.text()).toContain("administrator rights");
  });

  it("tells the user to install Python first for blocked pip packages", () => {
    const w = mountList([item({ id: "pysubs2", state: "missing", can_auto_install: false })]);
    expect(w.text()).toContain("Install Python first");
  });

  it("disables buttons while any install runs and labels the active one", () => {
    const items = [
      item({ id: "ffmpeg", state: "missing", can_auto_install: true }),
      item({ id: "mkvmerge", state: "missing", can_auto_install: true }),
    ];
    const w = mountList(items, { installing: "ffmpeg" });
    expect(w.get('[data-testid="install-ffmpeg"]').text()).toContain("Installing");
    expect(w.get('[data-testid="install-ffmpeg"]').attributes("disabled")).toBeDefined();
    expect(w.get('[data-testid="install-mkvmerge"]').attributes("disabled")).toBeDefined();
  });

  it("shows the error and log for a failed install", () => {
    const w = mountList([item({ id: "ffmpeg", state: "missing", can_auto_install: true })], {
      errors: { ffmpeg: "pkexec was dismissed" },
      logs: { ffmpeg: ["$ pkexec pacman -S ffmpeg", "error: target not found"] },
    });
    expect(w.get('[data-testid="error-ffmpeg"]').text()).toContain("pkexec was dismissed");
    expect(w.text()).toContain("error: target not found");
  });

  it("marks optional items as optional", () => {
    const w = mountList([item({ id: "faster_whisper", severity: "optional", state: "missing" })]);
    expect(w.text()).toContain("Optional");
  });
});
```

`src/components/DependencyDialog.test.ts`:
```ts
import { describe, it, expect, vi, beforeEach } from "vitest";
import { mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { i18n } from "../i18n";

vi.mock("../api/client", () => ({ apiInvoke: vi.fn(), isTauri: () => false }));
vi.mock("../api/events", () => ({ apiListen: vi.fn() }));

import DependencyDialog from "./DependencyDialog.vue";
import DependencyBanner from "./DependencyBanner.vue";
import { useDepsStore } from "../stores/depsStore";

const missingFfmpeg = {
  os: "linux",
  has_missing_required: true,
  items: [
    {
      id: "ffmpeg", label: "FFmpeg", severity: "required", state: "missing", purpose: "p",
      download_url: "https://ffmpeg.org/download.html", install_command: "sudo pacman -S ffmpeg",
      can_auto_install: true,
    },
  ],
};

function setup() {
  const pinia = createPinia();
  setActivePinia(pinia);
  const store = useDepsStore();
  return { pinia, store };
}

describe("DependencyDialog", () => {
  beforeEach(() => {
    document.body.innerHTML = "";
  });

  it("is hidden when nothing is missing", () => {
    const { pinia } = setup();
    mount(DependencyDialog, { global: { plugins: [pinia, i18n] }, attachTo: document.body });
    expect(document.body.querySelector('[data-testid="deps-dialog"]')).toBeNull();
  });

  it("opens when a required dependency is missing and Continue anyway dismisses it", async () => {
    const { pinia, store } = setup();
    store.report = missingFfmpeg as never;
    const w = mount(DependencyDialog, { global: { plugins: [pinia, i18n] }, attachTo: document.body });
    expect(document.body.querySelector('[data-testid="deps-dialog"]')).not.toBeNull();
    (document.body.querySelector('[data-testid="deps-later"]') as HTMLElement).click();
    await w.vm.$nextTick();
    expect(document.body.querySelector('[data-testid="deps-dialog"]')).toBeNull();
    expect(store.dialogDismissed).toBe(true);
  });
});

describe("DependencyBanner", () => {
  it("only shows after the dialog was dismissed, and Fix now reopens it", async () => {
    const { pinia, store } = setup();
    store.report = missingFfmpeg as never;
    const w = mount(DependencyBanner, { global: { plugins: [pinia, i18n] } });
    expect(w.find('[data-testid="deps-banner"]').exists()).toBe(false);
    store.dismiss();
    await w.vm.$nextTick();
    expect(w.find('[data-testid="deps-banner"]').exists()).toBe(true);
    await w.get('[data-testid="deps-fix"]').trigger("click");
    expect(store.dialogDismissed).toBe(false);
  });
});
```

- [ ] **Step 3: Run to verify failure**

Run: `npx vitest run src/components/DependencyList.test.ts src/components/DependencyDialog.test.ts`
Expected: FAIL (components do not exist).

- [ ] **Step 4: Implement the components**

`src/components/DependencyList.vue`:
```vue
<script setup lang="ts">
import { ref } from "vue";
import { useI18n } from "vue-i18n";
import { Check, Copy, Download } from "lucide-vue-next";
import type { DepStatus } from "../stores/depsStore";
import { openExternal } from "../utils/openExternal";

defineProps<{
  items: DepStatus[];
  installing: string | null;
  logs: Record<string, string[]>;
  errors: Record<string, string>;
}>();

const emit = defineEmits<{ (e: "install", id: string): void }>();

const { t, te } = useI18n();
const copied = ref<string | null>(null);

function purpose(item: DepStatus): string {
  const key = `deps.purpose.${item.id}`;
  return te(key) ? t(key) : item.purpose;
}

function dotClass(item: DepStatus): string {
  if (item.state === "ok") return "bg-success";
  return item.severity === "required" ? "bg-error" : "bg-warning";
}

async function copy(item: DepStatus) {
  if (!item.install_command) return;
  try {
    await navigator.clipboard.writeText(item.install_command);
    copied.value = item.id;
    setTimeout(() => {
      if (copied.value === item.id) copied.value = null;
    }, 1500);
  } catch {
    // Clipboard unavailable (insecure context); the command is still visible to select.
  }
}
</script>

<template>
  <ul class="space-y-3">
    <li
      v-for="item in items"
      :key="item.id"
      class="p-3 rounded-lg bg-bg-tertiary border border-border space-y-2"
    >
      <div class="flex items-start justify-between gap-3">
        <div class="min-w-0">
          <div class="flex items-center gap-2">
            <span :class="['w-2 h-2 rounded-full shrink-0', dotClass(item)]" />
            <span class="text-sm font-medium text-text-primary">{{ item.label }}</span>
            <span
              v-if="item.severity === 'optional'"
              class="text-[10px] uppercase tracking-wide text-text-muted border border-border rounded px-1.5 py-0.5"
              >{{ t("deps.optional") }}</span
            >
          </div>
          <p class="text-xs text-text-secondary mt-1">{{ purpose(item) }}</p>
          <p v-if="item.version" class="text-[11px] text-text-muted mt-0.5 truncate">{{ item.version }}</p>
        </div>
        <span
          :data-testid="`status-${item.id}`"
          :class="['text-xs font-medium shrink-0', item.state === 'ok' ? 'text-success' : 'text-text-secondary']"
          >{{ t(`deps.state.${item.state}`) }}</span
        >
      </div>

      <template v-if="item.state !== 'ok'">
        <div v-if="item.install_command" class="space-y-1">
          <p class="text-[11px] text-text-muted">{{ t("deps.command") }}</p>
          <code
            class="block text-xs px-2 py-1.5 rounded bg-bg-primary border border-border text-text-primary break-all select-all"
            dir="ltr"
            >{{ item.install_command }}</code
          >
        </div>
        <p v-if="item.install_command && !item.can_auto_install" class="text-xs text-warning">
          {{ t("deps.manualHint") }}
        </p>
        <p v-else-if="!item.install_command && !item.can_auto_install && item.id !== 'python'" class="text-xs text-text-muted">
          {{ t("deps.needPython") }}
        </p>

        <div class="flex flex-wrap items-center gap-2">
          <button
            v-if="item.can_auto_install"
            :data-testid="`install-${item.id}`"
            :disabled="installing !== null"
            class="inline-flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-accent text-white text-xs font-medium hover:bg-accent-hover active:bg-accent-active transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
            @click="emit('install', item.id)"
          >
            <Download class="w-3.5 h-3.5" />
            {{ installing === item.id ? t("deps.installing") : t("deps.install") }}
          </button>
          <button
            v-if="item.install_command"
            :data-testid="`copy-${item.id}`"
            class="inline-flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-bg-secondary border border-border text-text-secondary text-xs hover:bg-bg-hover transition-colors"
            @click="copy(item)"
          >
            <component :is="copied === item.id ? Check : Copy" class="w-3.5 h-3.5" />
            {{ copied === item.id ? t("deps.copied") : t("deps.copy") }}
          </button>
          <button
            :data-testid="`download-${item.id}`"
            class="inline-flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-bg-secondary border border-border text-text-secondary text-xs hover:bg-bg-hover transition-colors"
            @click="openExternal(item.download_url)"
          >
            {{ t("deps.download") }}
          </button>
        </div>

        <p v-if="errors[item.id]" :data-testid="`error-${item.id}`" class="text-xs text-error break-words">
          {{ errors[item.id] }}
        </p>
        <details v-if="logs[item.id]?.length" class="text-xs text-text-muted">
          <summary class="cursor-pointer">{{ t("deps.details") }}</summary>
          <pre class="mt-1 max-h-40 overflow-auto whitespace-pre-wrap" dir="ltr">{{ logs[item.id].join("\n") }}</pre>
        </details>
      </template>
    </li>
  </ul>
</template>
```

`src/components/DependencyDialog.vue`:
```vue
<script setup lang="ts">
import { computed } from "vue";
import { useI18n } from "vue-i18n";
import { RefreshCw } from "lucide-vue-next";
import Modal from "./Modal.vue";
import DependencyList from "./DependencyList.vue";
import { useDepsStore } from "../stores/depsStore";

const { t } = useI18n();
const deps = useDepsStore();

// Required problems first, then optional ones, so the important rows lead.
const sorted = computed(() =>
  [...(deps.report?.items ?? [])]
    .filter((i) => i.state !== "ok")
    .sort((a, b) => Number(b.severity === "required") - Number(a.severity === "required"))
);
const canInstallAny = computed(() => deps.missing.some((i) => i.can_auto_install));
</script>

<template>
  <Modal :open="deps.showDialog" :title="t('deps.title')" @close="deps.dismiss()">
    <div data-testid="deps-dialog" class="space-y-4">
      <p class="text-sm text-text-secondary">{{ t("deps.intro") }}</p>
      <DependencyList
        :items="sorted"
        :installing="deps.installing"
        :logs="deps.logs"
        :errors="deps.errors"
        @install="deps.install($event)"
      />
    </div>
    <template #footer>
      <button
        class="inline-flex items-center gap-1.5 px-3 py-2 rounded-lg bg-bg-tertiary text-text-secondary text-xs font-medium hover:bg-bg-hover transition-colors"
        :disabled="deps.checking || deps.installing !== null"
        @click="deps.check()"
      >
        <RefreshCw :class="['w-3.5 h-3.5', deps.checking && 'animate-spin']" />
        {{ t("deps.recheck") }}
      </button>
      <button
        data-testid="deps-later"
        class="px-3 py-2 rounded-lg bg-bg-tertiary text-text-secondary text-xs font-medium hover:bg-bg-hover transition-colors"
        @click="deps.dismiss()"
      >
        {{ t("deps.later") }}
      </button>
      <button
        v-if="canInstallAny"
        data-testid="deps-install-all"
        class="px-4 py-2 rounded-lg bg-accent text-white text-xs font-medium hover:bg-accent-hover active:bg-accent-active transition-colors disabled:opacity-50"
        :disabled="deps.installing !== null"
        @click="deps.installAll()"
      >
        {{ t("deps.installAll") }}
      </button>
    </template>
  </Modal>
</template>
```

`src/components/DependencyBanner.vue`:
```vue
<script setup lang="ts">
import { useI18n } from "vue-i18n";
import { useDepsStore } from "../stores/depsStore";

const { t } = useI18n();
const deps = useDepsStore();
</script>

<template>
  <div
    v-if="deps.showBanner"
    data-testid="deps-banner"
    class="flex items-center justify-between gap-3 px-4 py-2 bg-warning/15 border-b border-warning/40 text-xs text-text-primary"
  >
    <span>{{ t("deps.bannerText") }}</span>
    <button
      data-testid="deps-fix"
      class="px-3 py-1 rounded-md bg-warning text-black font-medium hover:opacity-90 transition-opacity"
      @click="deps.reopen()"
    >
      {{ t("deps.fixNow") }}
    </button>
  </div>
</template>
```

`src/components/DependenciesPanel.vue`:
```vue
<script setup lang="ts">
import { useI18n } from "vue-i18n";
import { Package, RefreshCw } from "lucide-vue-next";
import { useDepsStore } from "../stores/depsStore";
import DependencyList from "./DependencyList.vue";

const { t } = useI18n();
const deps = useDepsStore();
</script>

<template>
  <section class="space-y-3">
    <div class="flex items-center justify-between">
      <div class="flex items-center gap-2">
        <Package class="w-4 h-4 text-accent" />
        <h2 class="text-sm font-semibold text-text-primary uppercase tracking-wide">{{ t("deps.title") }}</h2>
      </div>
      <button
        class="inline-flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-bg-tertiary text-text-secondary text-xs font-medium hover:bg-bg-hover transition-colors"
        :disabled="deps.checking || deps.installing !== null"
        @click="deps.check()"
      >
        <RefreshCw :class="['w-3.5 h-3.5', deps.checking && 'animate-spin']" />
        {{ t("deps.recheck") }}
      </button>
    </div>
    <div class="p-4 rounded-lg bg-bg-secondary border border-border space-y-3">
      <p v-if="!deps.report" class="text-xs text-text-muted">{{ t("deps.checking") }}</p>
      <template v-else>
        <p v-if="!deps.missing.length" class="text-xs text-success">{{ t("deps.allGood") }}</p>
        <DependencyList
          :items="deps.report.items"
          :installing="deps.installing"
          :logs="deps.logs"
          :errors="deps.errors"
          @install="deps.install($event)"
        />
      </template>
    </div>
  </section>
</template>
```

- [ ] **Step 5: Mount everything**

`src/App.vue`:
- add imports after the `ToastHost` import:
```ts
import { useDepsStore } from "./stores/depsStore";
import DependencyDialog from "./components/DependencyDialog.vue";
import DependencyBanner from "./components/DependencyBanner.vue";
```
- after `const toastStore = useToastStore();` add:
```ts
const depsStore = useDepsStore();
let unlistenDeps: (() => void) | null = null;
```
- in `onMounted`, right after `appStore.loadConfig();` add:
```ts
  depsStore.check();
  depsStore.listen().then((u) => (unlistenDeps = u)).catch(() => {});
```
- replace `onUnmounted(() => unlistenJobs?.());` with:
```ts
onUnmounted(() => {
  unlistenJobs?.();
  unlistenDeps?.();
});
```
- in the template, replace `<TitleBar />` with:
```html
    <TitleBar />
    <DependencyBanner />
    <DependencyDialog />
```

`src/views/SettingsView.vue`:
- add `import DependenciesPanel from "../components/DependenciesPanel.vue";` after the `SetupInstructions` import.
- insert `<DependenciesPanel />` on its own line immediately before the `<!-- Tool Paths -->` comment.

Also add `bg-warning` support check: `grep -n "warning" src/style.css` must show `--color-warning` (it does) — Tailwind v4 maps `--color-*` tokens in `@theme`; confirm `bg-warning`/`text-warning` resolve by viewing the app in Step 7.

- [ ] **Step 6: Run tests, lint, and type-check**

Run: `npx vitest run && npm run lint && npx vue-tsc --noEmit`
Expected: all tests PASS (existing + 8 new component tests + 9 store tests); lint and type-check clean.

- [ ] **Step 7: Look at it in the running app**

Build the frontend, then start the already-built server binary with an empty `PATH` so every tool looks missing:
```bash
npm run build
(cd server && SUBFORGE_SUBSOURCE_API_KEY=x cargo build --release)
cd server && env PATH=/tmp/empty-path ./target/release/subforge-server
```
Open `http://127.0.0.1:8080` in the browser pane. Expected: the "Required Programs" dialog lists the missing tools with copyable commands and download buttons; "Continue anyway" shows the yellow banner and "Fix now" reopens the dialog; Settings shows the same list. Check English and Arabic (RTL) and light and dark themes, and confirm `bg-warning` / `text-warning` render (if a Tailwind class has no colour, add the token to the theme in `src/style.css`). Stop the server afterwards.

- [ ] **Step 8: Commit**

```bash
git add src
git commit -m "feat(ui): startup dependency dialog, banner, settings panel and translations" -m "Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>"
```

---

### Task 12: Version check script, CI and release hardening

**Files:**
- Create: `scripts/check-versions.mjs`
- Modify: `.github/workflows/ci.yml`, `.github/workflows/release.yml`, `package.json`

**Interfaces:**
- Produces: `node scripts/check-versions.mjs [vX.Y.Z]` exits 1 if `package.json`, `src-tauri/tauri.conf.json`, `src-tauri/Cargo.toml`, `server/Cargo.toml` (and the optional tag) disagree.

- [ ] **Step 1: Write the script**

`scripts/check-versions.mjs`:
```js
#!/usr/bin/env node
// Fails if the app version differs between files (and the git tag, if given).
import { readFileSync } from "node:fs";

const read = (p) => readFileSync(new URL(`../${p}`, import.meta.url), "utf8");
const cargoVersion = (p) => /^version\s*=\s*"([^"]+)"/m.exec(read(p))?.[1];

const versions = {
  "package.json": JSON.parse(read("package.json")).version,
  "src-tauri/tauri.conf.json": JSON.parse(read("src-tauri/tauri.conf.json")).version,
  "src-tauri/Cargo.toml": cargoVersion("src-tauri/Cargo.toml"),
  "server/Cargo.toml": cargoVersion("server/Cargo.toml"),
};

const tag = process.argv[2];
if (tag) versions[`tag ${tag}`] = tag.replace(/^v/, "");

const unique = new Set(Object.values(versions));
for (const [file, v] of Object.entries(versions)) console.log(`${v ?? "MISSING"}  ${file}`);

if (unique.size !== 1 || unique.has(undefined)) {
  console.error("\nVersion mismatch: make every file agree before releasing.");
  process.exit(1);
}
console.log(`\nAll versions match: ${[...unique][0]}`);
```
Add to `package.json` scripts: `"check:versions": "node scripts/check-versions.mjs",`

- [ ] **Step 2: Verify it passes now and fails on mismatch**

Run: `node scripts/check-versions.mjs && node scripts/check-versions.mjs v9.9.9; echo "exit=$?"`
Expected: first run prints "All versions match: 0.1.0"; second prints the mismatch error and `exit=1`.

- [ ] **Step 3: Update CI**

Replace `.github/workflows/ci.yml` with:
```yaml
name: CI

on:
  push:
  pull_request:

jobs:
  frontend:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: actions/setup-node@v4
        with:
          node-version: 20
          cache: npm
          cache-dependency-path: package-lock.json
      - run: npm ci
      - run: npm run check:versions
      - run: npm run lint
      - run: npx vue-tsc --noEmit
      - run: npm test

  shared-crates:
    # Pure-Rust crates: test them on both platforms we ship for.
    strategy:
      fail-fast: false
      matrix:
        os: [ubuntu-latest, windows-latest]
    runs-on: ${{ matrix.os }}
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
        with:
          workspaces: |
            crates/subforge-deps
            crates/subforge-metadata
            crates/subforge-subtitle-sources
      - run: cargo test --features tokio
        working-directory: crates/subforge-deps
      - run: cargo test
        working-directory: crates/subforge-metadata
      - run: cargo test
        working-directory: crates/subforge-subtitle-sources

  rust-tauri:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - name: Install Tauri Linux deps
        run: |
          sudo apt-get update
          sudo apt-get install -y \
            libwebkit2gtk-4.1-dev \
            libappindicator3-dev \
            librsvg2-dev \
            libdbus-1-dev \
            patchelf
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
        with:
          workspaces: src-tauri
      - run: cargo test
        working-directory: src-tauri
        env:
          SUBFORGE_SUBSOURCE_API_KEY: ${{ secrets.SUBFORGE_SUBSOURCE_API_KEY || 'ci-placeholder-key' }}

  rust-tauri-windows:
    runs-on: windows-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
        with:
          workspaces: src-tauri
      - run: cargo check
        working-directory: src-tauri
        env:
          SUBFORGE_SUBSOURCE_API_KEY: ${{ secrets.SUBFORGE_SUBSOURCE_API_KEY || 'ci-placeholder-key' }}

  rust-server:
    strategy:
      fail-fast: false
      matrix:
        os: [ubuntu-latest, windows-latest]
    runs-on: ${{ matrix.os }}
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
        with:
          workspaces: server
      - run: cargo test
        working-directory: server
        env:
          SUBFORGE_SUBSOURCE_API_KEY: ${{ secrets.SUBFORGE_SUBSOURCE_API_KEY || 'ci-placeholder-key' }}
```

In `.github/workflows/release.yml`, add as the first step after `actions/checkout@v4`:
```yaml
      - uses: actions/setup-node@v4
        with:
          node-version: 20
          cache: npm
          cache-dependency-path: package-lock.json

      - name: Check version matches tag
        run: node scripts/check-versions.mjs ${{ github.ref_name }}
```
and delete the later duplicate `actions/setup-node@v4` step (keep one). Also change `releaseBody` to:
```yaml
          releaseBody: |
            ## Install
            - **Windows:** run the `.msi` or `-setup.exe`.
            - **Linux:** `.AppImage` (any distro), `.deb`, `.rpm`, or the AUR packages `subforge` / `subforge-bin`.

            SubForge needs MKVToolNix, FFmpeg and Python — it checks on first launch and can install them for you.
            See the [README](https://github.com/UPdullah895/subforge#readme) for details.
```

- [ ] **Step 4: Validate the workflow YAML parses**

Run: `node -e "const y=require('node:fs').readFileSync('.github/workflows/ci.yml','utf8'); console.log(y.split('\n').length+' lines ok')" && python3 -c "import yaml,sys;[yaml.safe_load(open(f)) for f in ['.github/workflows/ci.yml','.github/workflows/release.yml']];print('yaml ok')"`
Expected: `yaml ok` (if PyYAML is missing, `pip install --user pyyaml` or skip with a visual review).

- [ ] **Step 5: Commit**

```bash
git add scripts/check-versions.mjs package.json .github
git commit -m "ci: version check, Windows test jobs, release notes; add check:versions script" -m "Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>"
```

---

### Task 13: Package metadata and AUR packages

**Files:**
- Modify: `src-tauri/tauri.conf.json`, `src-tauri/Cargo.toml`, `server/Cargo.toml`
- Create: `packaging/linux/subforge.desktop`, `packaging/aur/subforge/PKGBUILD`, `packaging/aur/subforge-bin/PKGBUILD`, `packaging/aur/README.md`

**Interfaces:**
- Produces: deb/rpm that depend on the video tools and Python; two PKGBUILDs installing `/usr/bin/subforge` and `/usr/lib/subforge/scripts/*.py` (the layout `locate_script` already searches).

- [ ] **Step 1: Clean up Rust package metadata**

In `src-tauri/Cargo.toml` replace the `description`/`authors` lines with:
```toml
description = "Subtitle and audio toolkit: extract, merge, transcribe and translate subtitles"
authors = ["UPdullah895"]
license = "GPL-3.0-only"
repository = "https://github.com/UPdullah895/subforge"
```
In `server/Cargo.toml` `[package]` add:
```toml
description = "SubForge server: REST and WebSocket API for the SubForge subtitle toolkit"
authors = ["UPdullah895"]
license = "GPL-3.0-only"
repository = "https://github.com/UPdullah895/subforge"
```

- [ ] **Step 2: Bundle metadata and package dependencies**

In `src-tauri/tauri.conf.json`, replace the `"bundle"` object with:
```json
  "bundle": {
    "active": true,
    "targets": "all",
    "publisher": "UPdullah895",
    "copyright": "Copyright (c) UPdullah895. GPL-3.0-only.",
    "category": "Utility",
    "shortDescription": "Subtitle and audio toolkit",
    "longDescription": "Extract, merge, convert, transcribe and translate subtitles and audio tracks. Uses MKVToolNix, FFmpeg and Python.",
    "license": "GPL-3.0-only",
    "licenseFile": "../LICENSE",
    "icon": [
      "icons/32x32.png",
      "icons/128x128.png",
      "icons/128x128@2x.png",
      "icons/icon.icns",
      "icons/icon.ico"
    ],
    "resources": {
      "../scripts/subtitle_ops.py": "scripts/subtitle_ops.py",
      "../scripts/subtitle_lines.py": "scripts/subtitle_lines.py",
      "../scripts/transcribe.py": "scripts/transcribe.py"
    },
    "linux": {
      "deb": {
        "depends": ["mkvtoolnix", "ffmpeg", "python3", "python3-venv"]
      },
      "rpm": {
        "depends": ["mkvtoolnix", "python3"]
      }
    }
  }
```
(Listing the three scripts explicitly stops the old `../scripts/*` glob from shipping `__pycache__` and the test file.)

Run: `cd src-tauri && SUBFORGE_SUBSOURCE_API_KEY=x cargo check`
Expected: compiles (tauri-build validates the config; if it rejects a key, fix per the error message).

- [ ] **Step 3: Desktop entry for source packages**

`packaging/linux/subforge.desktop`:
```ini
[Desktop Entry]
Type=Application
Name=SubForge
Comment=Subtitle and audio toolkit
Exec=subforge
Icon=subforge
Terminal=false
Categories=AudioVideo;Utility;
StartupNotify=true
```

- [ ] **Step 4: `subforge` PKGBUILD (build from source)**

`packaging/aur/subforge/PKGBUILD`:
```bash
# Maintainer: UPdullah895 <abdullahbomozh@gmail.com>
pkgname=subforge
pkgver=0.1.0
pkgrel=1
pkgdesc='Subtitle and audio toolkit: extract, merge, transcribe and translate subtitles'
arch=('x86_64' 'aarch64')
url='https://github.com/UPdullah895/subforge'
license=('GPL-3.0-only')
depends=('webkit2gtk-4.1' 'gtk3' 'mkvtoolnix-cli' 'ffmpeg' 'python')
makedepends=('cargo' 'nodejs' 'npm')
optdepends=('python-pysubs2: subtitle conversion (SubForge installs it for you on first run if missing)'
            'ollama: local LLM subtitle translation')
source=("$pkgname-$pkgver.tar.gz::$url/archive/refs/tags/v$pkgver.tar.gz")
sha256sums=('SKIP') # replace with the real sum: run `updpkgsums` after the v$pkgver tag exists

prepare() {
  cd "$pkgname-$pkgver"
  npm ci
  (cd src-tauri && cargo fetch --locked --target "$CARCH-unknown-linux-gnu")
}

build() {
  cd "$pkgname-$pkgver"
  # No SUBFORGE_SUBSOURCE_API_KEY here: Subsource search is simply disabled.
  npm run tauri -- build --no-bundle
}

package() {
  cd "$pkgname-$pkgver"
  install -Dm755 src-tauri/target/release/subforge "$pkgdir/usr/bin/subforge"
  install -dm755 "$pkgdir/usr/lib/subforge/scripts"
  install -m644 scripts/subtitle_ops.py scripts/subtitle_lines.py scripts/transcribe.py \
    "$pkgdir/usr/lib/subforge/scripts/"
  install -Dm644 packaging/linux/subforge.desktop "$pkgdir/usr/share/applications/subforge.desktop"
  install -Dm644 src-tauri/icons/128x128.png "$pkgdir/usr/share/icons/hicolor/128x128/apps/subforge.png"
  install -Dm644 LICENSE "$pkgdir/usr/share/licenses/$pkgname/LICENSE"
}
```

- [ ] **Step 5: `subforge-bin` PKGBUILD (repackage the release .deb)**

`packaging/aur/subforge-bin/PKGBUILD`:
```bash
# Maintainer: UPdullah895 <abdullahbomozh@gmail.com>
pkgname=subforge-bin
_pkgname=subforge
pkgver=0.1.0
pkgrel=1
pkgdesc='Subtitle and audio toolkit (prebuilt release)'
arch=('x86_64')
url='https://github.com/UPdullah895/subforge'
license=('GPL-3.0-only')
depends=('webkit2gtk-4.1' 'gtk3' 'mkvtoolnix-cli' 'ffmpeg' 'python')
optdepends=('python-pysubs2: subtitle conversion (SubForge installs it for you on first run if missing)'
            'ollama: local LLM subtitle translation')
provides=("$_pkgname")
conflicts=("$_pkgname")
options=('!strip')
source_x86_64=("$_pkgname-$pkgver-amd64.deb::$url/releases/download/v$pkgver/${_pkgname}_${pkgver}_amd64.deb")
sha256sums_x86_64=('SKIP') # replace with the real sum: run `updpkgsums` after the release is published

package() {
  bsdtar -xf "$srcdir/$_pkgname-$pkgver-amd64.deb" data.tar.gz
  bsdtar -xf data.tar.gz -C "$pkgdir"
  # Tauri's .deb already contains /usr/bin/subforge, /usr/lib/subforge/scripts,
  # the desktop entry and icons. Put the license where Arch expects it.
  if [ -f "$pkgdir/usr/share/doc/$_pkgname/copyright" ]; then
    install -Dm644 "$pkgdir/usr/share/doc/$_pkgname/copyright" \
      "$pkgdir/usr/share/licenses/$pkgname/LICENSE"
  fi
}
```

- [ ] **Step 6: Packaging notes**

`packaging/aur/README.md`:
```markdown
# AUR packages

| Package | What it does |
|---|---|
| `subforge` | Builds from the tagged GitHub source (needs `cargo`, `nodejs`, `npm`). |
| `subforge-bin` | Repackages the `.deb` from the GitHub release (fast install). |

## Publishing (maintainer steps)

1. Push the git tag `vX.Y.Z` and wait for the Release workflow to publish the assets.
2. In each package directory: bump `pkgver`, reset `pkgrel=1`, then `updpkgsums`.
3. Test: `makepkg -sf` (and `namcap PKGBUILD *.pkg.tar.zst` if installed).
4. `makepkg --printsrcinfo > .SRCINFO`
5. Push `PKGBUILD` and `.SRCINFO` to `ssh://aur@aur.archlinux.org/<pkgname>.git`.

SubForge checks for MKVToolNix, FFmpeg, Python and `pysubs2` on startup, so a missing
optional package never leaves users guessing.
```

- [ ] **Step 7: Validate the PKGBUILDs**

Run:
```bash
bash -n packaging/aur/subforge/PKGBUILD && bash -n packaging/aur/subforge-bin/PKGBUILD && echo syntax-ok
(cd packaging/aur/subforge && makepkg --printsrcinfo | head -20)
(cd packaging/aur/subforge-bin && makepkg --printsrcinfo | head -20)
```
Expected: `syntax-ok`, and valid `.SRCINFO` output for both (no files are written, nothing is built).

- [ ] **Step 8: Commit**

```bash
git add packaging src-tauri/tauri.conf.json src-tauri/Cargo.toml server/Cargo.toml
git commit -m "build: package metadata, deb/rpm dependencies, AUR PKGBUILDs" -m "Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>"
```

---

### Task 14: GitHub-readiness docs and housekeeping

**Files:**
- Modify: `README.md`, `.gitignore`
- Create: `CONTRIBUTING.md`, `SECURITY.md`, `CHANGELOG.md`, `.github/ISSUE_TEMPLATE/bug_report.md`, `.github/ISSUE_TEMPLATE/feature_request.md`, `.github/pull_request_template.md`
- Delete: `SubForge.desktop` (repo root)

- [ ] **Step 1: Housekeeping**

Append to `.gitignore`:
```
__pycache__/
*.pyc
.superpowers/
.claude/
# AUR build output
packaging/aur/*/src/
packaging/aur/*/pkg/
packaging/aur/*/*.pkg.tar.*
packaging/aur/*/*.tar.gz
packaging/aur/*/*.deb
```
Run: `git rm SubForge.desktop`

- [ ] **Step 2: Rewrite `README.md`**

Keep the existing Features, Tech Stack, Architecture, Roadmap and Wayland troubleshooting content, but restructure the top half. Replace everything from `## Download` through the end of the `### Desktop Launcher (Linux)` section with:

````markdown
## Install

### Windows
Download the `.msi` or `-setup.exe` from the [Releases page](https://github.com/UPdullah895/subforge/releases) and run it.
Windows SmartScreen may warn about an unrecognised publisher — choose **More info → Run anyway**.

### Arch Linux / CachyOS / Manjaro (AUR)
```bash
yay -S subforge-bin   # prebuilt, fast
yay -S subforge       # builds from source
```

### Other Linux
Download the `.AppImage` (any distro), `.deb` (Debian/Ubuntu) or `.rpm` (Fedora/openSUSE) from the Releases page.
```bash
chmod +x SubForge_*.AppImage && ./SubForge_*.AppImage
```

### macOS
Not published. Build from source (below); support is best-effort.

---

## First launch: what SubForge needs

SubForge relies on a few free programs. **On startup it checks for them and tells you exactly what is missing** — with an **Install** button, the exact command to copy, and a link to the official download page. You can also review them any time under **Settings → Required Programs**.

| Program | Needed for | Required? | Where to get it |
|---|---|---|---|
| MKVToolNix (`mkvmerge`, `mkvextract`) | Reading, extracting and merging tracks | Yes | [mkvtoolnix.download](https://mkvtoolnix.download/downloads.html) |
| FFmpeg (`ffmpeg`, `ffprobe`) | Audio extraction, conversion, merging | Yes | [ffmpeg.org](https://ffmpeg.org/download.html) |
| Python 3.9+ | Subtitle, translation and transcription helpers | Yes | [python.org](https://www.python.org/downloads/) |
| `pysubs2` (Python package) | Subtitle format conversion and retiming | Yes | installed for you |
| `faster-whisper` (Python package) | Speech-to-text | Optional | installed for you |
| `mkvpropedit` (MKVToolNix) | Editing track names/flags in place | Optional | comes with MKVToolNix |

**How the one-click install works**
- **Windows:** uses `winget` for MKVToolNix, FFmpeg and Python.
- **Linux:** uses your package manager (`pacman`, `apt`, `dnf`, `zypper`) through a graphical password prompt (`pkexec`). If `pkexec` is not available, SubForge shows the `sudo` command to run yourself.
- **macOS:** uses Homebrew if installed.
- **Python packages** go into a private environment (`~/.local/share/subforge/venv`, or `%LOCALAPPDATA%\subforge\venv` on Windows), so your system Python is never modified.

Prefer to do it by hand?
```bash
# Arch / CachyOS
sudo pacman -S mkvtoolnix-cli ffmpeg python
# Debian / Ubuntu
sudo apt install mkvtoolnix ffmpeg python3 python3-venv python3-pip
# Fedora
sudo dnf install mkvtoolnix ffmpeg-free python3 python3-pip
# Windows (PowerShell)
winget install MoritzBunkus.MKVToolNix Gyan.FFmpeg Python.Python.3.12
# Python packages (any OS)
pip install --user pysubs2 faster-whisper
```

Run `cargo run --example check` inside `crates/subforge-deps` to print the same dependency report in your terminal.

### Server mode
`subforge-server` shows the same dependency dialog in the web UI. Because installing software on a server is sensitive, the Install button is **disabled by default**; set `SUBFORGE_ALLOW_INSTALL=1` to enable it.

---

## Build from source

Prerequisites: [Node.js](https://nodejs.org/) 18+, [Rust](https://rustup.rs/), the Tauri system libraries for your OS ([guide](https://tauri.app/start/prerequisites/)), plus the programs in the table above.

```bash
git clone https://github.com/UPdullah895/subforge.git
cd subforge
npm install
npm run tauri dev        # development window
npm run tauri build      # installers in src-tauri/target/release/bundle
```
On Wayland, if you see a protocol error, use `npm run tauri:dev:wayland`.

Optional: set `SUBFORGE_SUBSOURCE_API_KEY` at build time to enable Subsource subtitle search. Without it everything else works and Subsource search is silently skipped.

### Linux app-menu entry (development checkouts)
```bash
./scripts/install-desktop-launcher.sh
```
This registers a launcher that runs a release build if present, otherwise `npm run tauri:dev`.
````

Also: in the Project Structure block add `│   ├── subforge-deps/          # Dependency detection + one-click install` under `crates/`; change the license line to `GPL-3.0-only — see [LICENSE](LICENSE).`; delete the old `## Prerequisites (building from source)` and `## Development` sections (their content now lives above); and in Troubleshooting add:
````markdown
### "Required Programs" keeps showing something as missing
Click **Re-check** after installing. On Windows, if you installed a tool in a custom location, add its folder to `PATH` and restart SubForge. Run `cargo run --example check` in `crates/subforge-deps` to see exactly what SubForge detects.

### Windows: installed tool not found right after `winget install`
Close and reopen SubForge — new `PATH` entries are only picked up by new processes.
````

- [ ] **Step 3: Community files**

`CONTRIBUTING.md`:
````markdown
# Contributing to SubForge

Thanks for helping! Bug reports, translations and pull requests are all welcome.

## Setup
```bash
git clone https://github.com/UPdullah895/subforge.git && cd subforge
npm install
npm run tauri dev
```
You also need MKVToolNix, FFmpeg and Python (see the README); SubForge will tell you what is missing.

## Before opening a PR
```bash
npm run lint && npx vue-tsc --noEmit && npm test
(cd crates/subforge-deps && cargo test)
(cd src-tauri && cargo test)
(cd server && cargo test)
```
Set `SUBFORGE_SUBSOURCE_API_KEY=ci-placeholder-key` for the Tauri/server builds if you have no key.

## Guidelines
- Keep changes focused; add or update tests for behaviour changes.
- User-visible text goes in `src/i18n/en.json` **and** `ar.json`.
- The desktop app and `server/` share the same frontend; new backend features need both a Tauri command and a REST route.
- Never build shell command strings from user input — pass argument arrays.

## Releasing (maintainers)
1. Bump the version in `package.json`, `src-tauri/tauri.conf.json`, `src-tauri/Cargo.toml`, `server/Cargo.toml` (`npm run check:versions` verifies).
2. Update `CHANGELOG.md`, commit, tag `vX.Y.Z`, push the tag — the Release workflow builds Windows and Linux installers.
3. Update the AUR packages (see `packaging/aur/README.md`).
````

`SECURITY.md`:
````markdown
# Security Policy

## Reporting a vulnerability
Please **do not** open a public issue. Use GitHub's
[private vulnerability reporting](https://github.com/UPdullah895/subforge/security/advisories/new)
on this repository. Expect an acknowledgement within a few days.

## Scope notes
- SubForge runs local tools (`mkvmerge`, `ffmpeg`, Python helpers) on files you choose.
- Installing dependencies uses a fixed allowlist of packages and never a shell.
- `subforge-server` binds to `127.0.0.1` by default. Do not expose it to untrusted networks;
  software installation through its API is off unless `SUBFORGE_ALLOW_INSTALL=1`.
````

`CHANGELOG.md`:
````markdown
# Changelog

All notable changes to this project are documented here. Format based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions follow [SemVer](https://semver.org/).

## [Unreleased]

### Added
- Startup dependency check with install guidance (MKVToolNix, FFmpeg, Python, pysubs2, faster-whisper).
- One-click install via winget (Windows), the system package manager through pkexec (Linux), Homebrew (macOS), and a private virtualenv for Python packages.
- "Required Programs" panel in Settings and a banner when something required is missing.
- AUR packages `subforge` and `subforge-bin`; `.deb`/`.rpm` now declare their dependencies.
- Windows CI jobs.

### Changed
- Tools are located in well-known install folders as well as `PATH` (fixes detection right after `winget install` on Windows).
- Child processes no longer flash a console window on Windows.
- Helper scripts are found in standard Linux package locations.

## [0.1.0]
- Initial release.
````

`.github/ISSUE_TEMPLATE/bug_report.md`:
````markdown
---
name: Bug report
about: Something is not working
labels: bug
---

**What happened?**

**What did you expect?**

**Steps to reproduce**
1.

**Environment**
- SubForge version / install method (AUR, AppImage, .msi, source):
- OS and version:
- Output of `cargo run --example check` in `crates/subforge-deps` (shows detected tools), if relevant:

**Logs / screenshots**
````

`.github/ISSUE_TEMPLATE/feature_request.md`:
````markdown
---
name: Feature request
about: Suggest an improvement
labels: enhancement
---

**What problem would this solve?**

**What would you like SubForge to do?**

**Alternatives you considered**
````

`.github/pull_request_template.md`:
````markdown
## What and why

## How I tested it
- [ ] `npm run lint && npx vue-tsc --noEmit && npm test`
- [ ] `cargo test` in the crates/apps I touched
- [ ] Strings added to both `en.json` and `ar.json` (if UI text changed)
````

- [ ] **Step 4: Check the docs for stale references**

Run: `grep -rn "SubForge.desktop\|nested subforge" README.md CONTRIBUTING.md docs/superpowers/specs docs/superpowers/plans | head`
Expected: no README/CONTRIBUTING hits (spec/plan mentions are fine — they document the removal).

- [ ] **Step 5: Commit**

```bash
git add -A README.md .gitignore CONTRIBUTING.md SECURITY.md CHANGELOG.md .github
git commit -m "docs: install guide, dependency table, community files; remove machine-specific launcher" -m "Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>"
```

---

### Task 15: Final verification

**Files:** none (verification only; fix and commit anything found).

- [ ] **Step 1: Everything green**

Run:
```bash
npm run check:versions && npm run lint && npx vue-tsc --noEmit && npm test
(cd crates/subforge-deps && cargo test --features tokio && cargo run --example check | head -30)
(cd crates/subforge-metadata && cargo test) && (cd crates/subforge-subtitle-sources && cargo test)
(cd src-tauri && SUBFORGE_SUBSOURCE_API_KEY=x cargo test)
(cd server && SUBFORGE_SUBSOURCE_API_KEY=x cargo test)
```
Expected: all PASS; the example prints a JSON report with `ffmpeg`/`mkvmerge`/`python` `"ok"`.

- [ ] **Step 2: Behaviour check with missing tools (real binary)**

Build and run the desktop binary with a stripped `PATH`:
```bash
npm run tauri -- build --no-bundle
env PATH=/tmp/empty-path ./src-tauri/target/release/subforge
```
Expected: window opens; the dialog lists mkvmerge, mkvextract, ffmpeg, ffprobe, Python (and pysubs2 as "install Python first") with correct install commands for this distro (`sudo pacman -S ...`). Click **Re-check** with the real `PATH` restored in a second run → dialog does not open. Test the Install button end-to-end for `pysubs2` by first removing the venv (`rm -rf ~/.local/share/subforge/venv`) and temporarily hiding the system package (e.g. `PYTHONNOUSERSITE=1` run with a venv python lacking it) — expected: venv created, `pip install` output streams into the Details log, status flips to Installed.

- [ ] **Step 3: Package install smoke test**

Run the AUR-style layout check against the built binary:
```bash
mkdir -p /tmp/sf-root/usr/bin /tmp/sf-root/usr/lib/subforge/scripts
cp src-tauri/target/release/subforge /tmp/sf-root/usr/bin/
cp scripts/subtitle_ops.py scripts/subtitle_lines.py scripts/transcribe.py /tmp/sf-root/usr/lib/subforge/scripts/
cd /tmp && cargo run --manifest-path /home/updullah/Documents/Projects/subforge/crates/subforge-deps/Cargo.toml --example check >/dev/null && echo deps-ok
```
Expected: `deps-ok`. Also confirm `locate_script_in("transcribe.py", None, Some("/tmp/sf-root/usr/bin"))` resolves — covered by the `finds_linux_package_layout` test already passing.

- [ ] **Step 4: Review the whole diff**

Run: `git log --oneline main..HEAD 2>/dev/null || git log --oneline -20` and `git status --short`
Expected: only the commits from this plan; working tree clean (no stray `__pycache__`, `target/`, `.SRCINFO`, tarballs).

- [ ] **Step 5: Hand-off summary to the user**

Report: what was built; what was verified locally; that Windows behaviour is covered by the CI Windows jobs only (not run locally); and the manual steps left to the user — push, tag `v0.1.0`, publish the release, then follow `packaging/aur/README.md` (needs their AUR SSH key). Do not push or tag.
