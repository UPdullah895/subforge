pub mod hide;
pub mod model;
pub mod platform;

pub use hide::HideWindow;
pub use model::{DepId, DepReport, DepSpec, DepState, DepStatus, Severity};
pub use platform::{detect_package_manager, Os, PackageManager, SystemPackage};
pub mod detect;
#[cfg(test)]
pub(crate) mod testutil;

pub use detect::{find_executable, find_executables, tool_version, SearchEnv};
pub mod context;
pub use context::Context;
pub mod install;
pub use install::{plan_for, run_plan, InstallError, InstallPlan};
pub mod check;
pub use check::check_all;
