pub mod model;
pub mod platform;
pub mod hide;
pub mod scripts;

pub use model::{DepId, DepReport, DepSpec, DepState, DepStatus, Severity};
pub use platform::{detect_package_manager, Os, PackageManager, SystemPackage};
pub use hide::HideWindow;
pub use scripts::{locate_script, locate_script_in};
