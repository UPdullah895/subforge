use crate::context::Context;
use crate::hide::HideWindow;
use crate::model::DepId;
use crate::platform::{Os, SystemPackage};
use std::io::{BufRead, BufReader, Read};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread::{self, JoinHandle};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum InstallError {
    #[error("No supported package manager was found on this system. Use the download page to install it manually.")]
    NoPackageManager,
    #[error("Could not start the installer: {0}")]
    Spawn(String),
    #[error("The installer step failed ({}): {command}", describe_code(*code))]
    StepFailed { command: String, code: Option<i32> },
    #[error("Authorisation was cancelled or denied. Run the command shown in the dialog in a terminal instead.")]
    AuthorizationDenied,
    #[error("This needs administrator rights and no graphical password prompt (pkexec) was found. Run this in a terminal: {0}")]
    NeedsManualInstall(String),
}

fn describe_code(code: Option<i32>) -> String {
    match code {
        Some(c) => format!("exit code {c}"),
        None => "unknown exit code".to_string(),
    }
}

/// pkexec exits 126 when the user dismisses the prompt / is not authorised and
/// 127 when authorisation could not be obtained; report those as a denial.
fn classify_failure(program: &str, command: String, code: Option<i32>) -> InstallError {
    let is_pkexec = Path::new(program).file_name().is_some_and(|n| n == "pkexec");
    if is_pkexec && matches!(code, Some(126 | 127)) {
        InstallError::AuthorizationDenied
    } else {
        InstallError::StepFailed { command, code }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct InstallPlan {
    pub dep: DepId,
    /// argv lists run in order (already prefixed with `pkexec` where needed).
    pub steps: Vec<Vec<String>>,
    /// What a user would paste into a terminal.
    pub display_command: String,
    /// False when elevation is required but `pkexec` is unavailable.
    pub auto_runnable: bool,
}

struct Cmd {
    argv: Vec<String>,
    elevated: bool,
}

pub fn plan_for(id: DepId, ctx: &Context) -> Result<InstallPlan, InstallError> {
    match id {
        DepId::Mkvmerge | DepId::Mkvextract | DepId::Mkvpropedit => system_plan(id, SystemPackage::Mkvtoolnix, ctx),
        DepId::Ffmpeg | DepId::Ffprobe => system_plan(id, SystemPackage::Ffmpeg, ctx),
    }
}

fn system_plan(id: DepId, pkg: SystemPackage, ctx: &Context) -> Result<InstallPlan, InstallError> {
    let pm = ctx.package_manager.ok_or(InstallError::NoPackageManager)?;
    let cmds = pm
        .install_commands(pkg)
        .into_iter()
        .map(|argv| Cmd { argv, elevated: pm.needs_elevation() })
        .collect();
    Ok(finalize(id, cmds, ctx))
}

fn quote(arg: &str) -> String {
    if arg.contains(' ') {
        format!("\"{arg}\"")
    } else {
        arg.to_string()
    }
}

fn finalize(id: DepId, cmds: Vec<Cmd>, ctx: &Context) -> InstallPlan {
    let mut auto_runnable = true;
    let mut steps = Vec::new();
    let mut shown = Vec::new();

    for cmd in cmds {
        let line = cmd.argv.iter().map(|a| quote(a)).collect::<Vec<_>>().join(" ");
        if cmd.elevated && ctx.os == Os::Linux {
            shown.push(format!("sudo {line}"));
            if ctx.has_pkexec {
                let mut argv = vec!["pkexec".to_string()];
                let mut rest = cmd.argv.into_iter();
                if let Some(program) = rest.next() {
                    // pkexec runs with a minimal PATH; give it the absolute program.
                    let resolved = ctx.find_executable(&program);
                    argv.push(resolved.map(|p| p.to_string_lossy().into_owned()).unwrap_or(program));
                }
                argv.extend(rest);
                steps.push(argv);
            } else {
                auto_runnable = false;
                steps.push(cmd.argv);
            }
        } else {
            shown.push(line);
            steps.push(cmd.argv);
        }
    }

    InstallPlan { dep: id, steps, display_command: shown.join(" && "), auto_runnable }
}

/// Runs every step, passing each output line (stdout and stderr) to `on_line`.
/// Stops at the first failing step.
pub fn run_plan(plan: &InstallPlan, mut on_line: impl FnMut(&str)) -> Result<(), InstallError> {
    if !plan.auto_runnable {
        return Err(InstallError::NeedsManualInstall(plan.display_command.clone()));
    }
    for step in &plan.steps {
        on_line(&format!("$ {}", step.join(" ")));
        run_step(step, &mut on_line)?;
    }
    Ok(())
}

/// Forwards lines from `r` to `tx`, decoding lossily so non-UTF-8 output never
/// stops the drain (which would leave the child writing to a closed pipe).
fn spawn_reader(r: impl Read + Send + 'static, tx: mpsc::Sender<String>) -> JoinHandle<()> {
    thread::spawn(move || {
        let mut reader = BufReader::new(r);
        let mut buf = Vec::new();
        loop {
            buf.clear();
            match reader.read_until(b'\n', &mut buf) {
                Ok(0) | Err(_) => break,
                Ok(_) => {
                    while matches!(buf.last(), Some(b'\n' | b'\r')) {
                        buf.pop();
                    }
                    if tx.send(String::from_utf8_lossy(&buf).into_owned()).is_err() {
                        break;
                    }
                }
            }
        }
    })
}

fn run_step(step: &[String], on_line: &mut impl FnMut(&str)) -> Result<(), InstallError> {
    let (program, args) = step
        .split_first()
        .ok_or_else(|| InstallError::Spawn("empty command".into()))?;

    let mut child = Command::new(program)
        .hide_window()
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| InstallError::Spawn(format!("{program}: {e}")))?;

    let (tx, rx) = mpsc::channel::<String>();
    let mut readers = Vec::new();
    if let Some(out) = child.stdout.take() {
        readers.push(spawn_reader(out, tx.clone()));
    }
    if let Some(err) = child.stderr.take() {
        readers.push(spawn_reader(err, tx.clone()));
    }
    drop(tx);

    for line in rx {
        on_line(&line);
    }
    for r in readers {
        let _ = r.join();
    }

    let status = child.wait().map_err(|e| InstallError::Spawn(e.to_string()))?;
    if status.success() {
        Ok(())
    } else {
        Err(classify_failure(program, step.join(" "), status.code()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detect::SearchEnv;
    use crate::platform::{Os, PackageManager};

    fn ctx(os: Os, pm: Option<PackageManager>, pkexec: bool) -> Context {
        Context {
            os,
            env: SearchEnv { dirs: vec![], os },
            package_manager: pm,
            has_pkexec: pkexec,
        }
    }

    #[test]
    fn pacman_plan_runs_through_pkexec_and_displays_sudo() {
        let plan = plan_for(DepId::Mkvmerge, &ctx(Os::Linux, Some(PackageManager::Pacman), true)).unwrap();
        assert_eq!(
            plan.steps,
            vec![vec!["pkexec", "pacman", "-S", "--needed", "--noconfirm", "mkvtoolnix-cli"]]
        );
        assert_eq!(plan.display_command, "sudo pacman -S --needed --noconfirm mkvtoolnix-cli");
        assert!(plan.auto_runnable);
    }

    #[test]
    fn without_pkexec_the_plan_is_display_only() {
        let plan = plan_for(DepId::Ffmpeg, &ctx(Os::Linux, Some(PackageManager::Apt), false)).unwrap();
        assert!(!plan.auto_runnable);
        assert_eq!(plan.display_command, "sudo apt-get install -y ffmpeg");
    }

    #[test]
    fn winget_needs_no_elevation_prefix() {
        let plan = plan_for(DepId::Ffprobe, &ctx(Os::Windows, Some(PackageManager::Winget), false)).unwrap();
        assert!(plan.auto_runnable);
        assert_eq!(plan.steps[0][0], "winget");
        assert!(!plan.display_command.starts_with("sudo"));
        assert!(plan.display_command.contains("Gyan.FFmpeg"));
    }

    #[test]
    fn shared_packages_give_the_same_command() {
        let c = ctx(Os::Linux, Some(PackageManager::Pacman), true);
        let a = plan_for(DepId::Mkvmerge, &c).unwrap();
        let b = plan_for(DepId::Mkvextract, &c).unwrap();
        assert_eq!(a.steps, b.steps);
    }

    #[test]
    fn no_package_manager_is_an_error() {
        let err = plan_for(DepId::Ffmpeg, &ctx(Os::Linux, None, true)).unwrap_err();
        assert!(matches!(err, InstallError::NoPackageManager));
    }

    #[test]
    fn classify_failure_maps_pkexec_denial() {
        let c = |prog: &str, code| classify_failure(prog, "cmd".into(), code);
        assert!(matches!(c("pkexec", Some(126)), InstallError::AuthorizationDenied));
        assert!(matches!(c("pkexec", Some(127)), InstallError::AuthorizationDenied));
        assert!(matches!(c("pkexec", Some(1)), InstallError::StepFailed { .. }));
        assert!(matches!(c("sh", Some(126)), InstallError::StepFailed { .. }));
        assert!(matches!(c("pkexec", None), InstallError::StepFailed { .. }));
    }

    #[test]
    fn step_failed_message_shows_plain_exit_code() {
        let e = InstallError::StepFailed { command: "x".into(), code: Some(3) };
        let msg = e.to_string();
        assert!(msg.contains("exit code 3"), "{msg}");
        assert!(!msg.contains("Some"), "{msg}");
        let e = InstallError::StepFailed { command: "x".into(), code: None };
        assert!(e.to_string().contains("unknown exit code"));
    }

    #[cfg(unix)]
    mod unix {
        use super::*;
        use crate::testutil::make_script;
        use tempfile::tempdir;

        #[test]
        fn run_plan_streams_stdout_and_stderr() {
            let plan = InstallPlan {
                dep: DepId::Ffmpeg,
                steps: vec![vec!["sh".into(), "-c".into(), "echo out-line; echo err-line 1>&2".into()]],
                display_command: String::new(),
                auto_runnable: true,
            };
            let mut lines = Vec::new();
            run_plan(&plan, |l| lines.push(l.to_string())).unwrap();
            assert!(lines.iter().any(|l| l == "out-line"));
            assert!(lines.iter().any(|l| l == "err-line"));
        }

        #[test]
        fn run_plan_reports_failing_step_and_stops() {
            let plan = InstallPlan {
                dep: DepId::Ffmpeg,
                steps: vec![
                    vec!["sh".into(), "-c".into(), "exit 3".into()],
                    vec!["sh".into(), "-c".into(), "echo should-not-run".into()],
                ],
                display_command: String::new(),
                auto_runnable: true,
            };
            let mut lines = Vec::new();
            let err = run_plan(&plan, |l| lines.push(l.to_string())).unwrap_err();
            assert!(matches!(err, InstallError::StepFailed { code: Some(3), .. }));
            assert!(!lines.iter().any(|l| l == "should-not-run"));
        }

        #[test]
        fn run_plan_reports_unstartable_program() {
            let plan = InstallPlan {
                dep: DepId::Ffmpeg,
                steps: vec![vec!["/definitely/not/a/program".into()]],
                display_command: String::new(),
                auto_runnable: true,
            };
            assert!(matches!(run_plan(&plan, |_| {}).unwrap_err(), InstallError::Spawn(_)));
        }

        #[test]
        fn run_plan_survives_non_utf8_output() {
            let plan = InstallPlan {
                dep: DepId::Ffmpeg,
                steps: vec![vec!["sh".into(), "-c".into(), "printf 'bad\\377byte\\nsecond-line\\n'".into()]],
                display_command: String::new(),
                auto_runnable: true,
            };
            let mut lines = Vec::new();
            run_plan(&plan, |l| lines.push(l.to_string())).unwrap();
            assert!(lines.iter().any(|l| l.contains("bad")));
            assert!(lines.iter().any(|l| l == "second-line"));
        }

        #[test]
        fn run_plan_refuses_non_auto_runnable_plans() {
            let tmp = tempdir().unwrap();
            let marker = tmp.path().join("marker");
            let plan = InstallPlan {
                dep: DepId::Ffmpeg,
                steps: vec![vec!["touch".into(), marker.to_string_lossy().into_owned()]],
                display_command: "sudo apt-get install -y ffmpeg".into(),
                auto_runnable: false,
            };
            let err = run_plan(&plan, |_| {}).unwrap_err();
            assert!(matches!(err, InstallError::NeedsManualInstall(_)));
            assert!(!marker.exists());
        }

        #[test]
        fn pkexec_step_uses_absolute_package_manager_path() {
            let tmp = tempdir().unwrap();
            let bin = tmp.path().join("bin");
            let pacman = make_script(&bin, "pacman", "exit 0");
            let c = Context {
                os: Os::Linux,
                env: SearchEnv { dirs: vec![bin], os: Os::Linux },
                package_manager: Some(PackageManager::Pacman),
                has_pkexec: true,
            };
            let plan = plan_for(DepId::Mkvmerge, &c).unwrap();
            assert_eq!(plan.steps[0][0], "pkexec");
            assert_eq!(plan.steps[0][1], pacman.to_string_lossy());
            assert!(plan.display_command.starts_with("sudo pacman "));
        }
    }
}
