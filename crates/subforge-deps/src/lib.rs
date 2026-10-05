pub mod model;
pub mod platform;

pub use model::{DepId, DepReport, DepSpec, DepState, DepStatus, Severity};
pub use platform::{detect_package_manager, Os, PackageManager, SystemPackage};
