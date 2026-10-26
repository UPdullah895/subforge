use std::path::Path;

/// True when `path` is an existing directory. Used by drag and drop, which
/// receives bare paths with no type information: a folder named `Lazarus` has
/// no extension and would otherwise be treated as a media file.
#[tauri::command]
pub async fn path_is_dir(path: String) -> bool {
    Path::new(&path).is_dir()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn detects_directories_only() {
        let dir = std::env::temp_dir();
        assert!(path_is_dir(dir.to_string_lossy().into()).await);
        let file = dir.join("subforge-path-kind-test.txt");
        std::fs::write(&file, b"x").unwrap();
        assert!(!path_is_dir(file.to_string_lossy().into()).await);
        assert!(!path_is_dir("/definitely/not/here".into()).await);
        let _ = std::fs::remove_file(file);
    }
}
