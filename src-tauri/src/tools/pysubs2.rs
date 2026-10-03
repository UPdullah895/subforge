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

fn get_python() -> Result<PathBuf, Pysubs2Error> {
    super::find_python().ok_or(Pysubs2Error::PythonNotFound)
}

pub fn convert_format(
    input: &str,
    output: &str,
    target_format: &str,
) -> Result<(), Pysubs2Error> {
    let python = get_python()?;
    let script = super::script_path("subtitle_ops.py");

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
    let script = super::script_path("subtitle_ops.py");

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

pub fn stretch_framerate(
    input: &str,
    output: &str,
    from_fps: f64,
    to_fps: f64,
) -> Result<(), Pysubs2Error> {
    let python = get_python()?;
    let script = super::script_path("subtitle_ops.py");

    let output_cmd = Command::new(&python)
        .args([
            &script,
            "stretch",
            input,
            output,
            "--from-fps",
            &from_fps.to_string(),
            "--to-fps",
            &to_fps.to_string(),
        ])
        .output()?;

    if !output_cmd.status.success() {
        let stderr = String::from_utf8_lossy(&output_cmd.stderr).to_string();
        return Err(Pysubs2Error::ScriptFailed(stderr));
    }

    Ok(())
}
