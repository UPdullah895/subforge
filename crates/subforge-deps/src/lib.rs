pub mod model;
pub mod platform;
pub mod hide;
pub mod scripts;

pub use model::{DepId, DepReport, DepSpec, DepState, DepStatus, Severity};
pub use platform::{detect_package_manager, Os, PackageManager, SystemPackage};
pub use hide::HideWindow;
pub use scripts::{locate_script, locate_script_in};
#[cfg(test)]
pub(crate) mod testutil;
pub mod detect;

pub use detect::{
    find_executable, find_python, parse_major_minor, probe_python, python_has_venv,
    python_package_version, tool_version, venv_python_path, PythonInfo, SearchEnv, MIN_PYTHON,
};
pub mod context;
pub use context::{default_venv_dir, Context};
pub mod install;
pub use install::{plan_for, run_plan, InstallError, InstallPlan};
