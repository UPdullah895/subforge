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
