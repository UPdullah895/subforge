use std::path::PathBuf;
use std::process::Command;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum Pysubs2Error {
    #[error("Python not found in PATH")]
    PythonNotFound,
    #[error("pysubs2 script failed: {0}")]
    ScriptFailed(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

fn find_python() -> Option<PathBuf> {
    for cmd in ["python3", "python"] {
        if let Ok(paths) = std::env::var("PATH") {
            for dir in paths.split(':') {
                let full = PathBuf::from(dir).join(cmd);
                if full.is_file() {
                    return Some(full);
                }
            }
        }
    }
    None
}

fn get_python() -> Result<PathBuf, Pysubs2Error> {
    find_python().ok_or(Pysubs2Error::PythonNotFound)
}

fn script_path() -> String {
    // Assume the script is in the project root's scripts/ directory
    // In dev mode, this is relative to the CWD; in production, we'd bundle it.
    std::env::current_dir()
        .map(|cwd| cwd.join("scripts").join("subtitle_ops.py").to_string_lossy().to_string())
        .unwrap_or_else(|_| "scripts/subtitle_ops.py".to_string())
}

pub fn convert_format(
    input: &str,
    output: &str,
    target_format: &str,
) -> Result<(), Pysubs2Error> {
    let python = get_python()?;
    let script = script_path();

    let output_cmd = Command::new(&python)
        .args([
            &script,
            "convert",
            input,
            output,
            "--format",
            target_format,
        ])
        .output()?;

    if !output_cmd.status.success() {
        let stderr = String::from_utf8_lossy(&output_cmd.stderr).to_string();
        return Err(Pysubs2Error::ScriptFailed(stderr));
    }

    Ok(())
}

pub fn shift_subtitles(
    input: &str,
    output: &str,
    milliseconds: i64,
) -> Result<(), Pysubs2Error> {
    let python = get_python()?;
    let script = script_path();

    let output_cmd = Command::new(&python)
        .args([
            &script,
            "shift",
            input,
            output,
            "--ms",
            &milliseconds.to_string(),
        ])
        .output()?;

    if !output_cmd.status.success() {
        let stderr = String::from_utf8_lossy(&output_cmd.stderr).to_string();
        return Err(Pysubs2Error::ScriptFailed(stderr));
    }

    Ok(())
}
