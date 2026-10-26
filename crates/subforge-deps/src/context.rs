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
