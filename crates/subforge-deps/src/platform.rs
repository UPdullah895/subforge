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
