use crate::context::Context;
use crate::detect::{python_package_version, tool_version, PythonInfo};
use crate::install::plan_for;
use crate::model::{DepId, DepReport, DepState, DepStatus};

pub fn check_all(ctx: &Context) -> DepReport {
    let python = ctx.find_python();
    let items = DepId::ALL
        .into_iter()
        .map(|id| check_one(ctx, id, python.as_ref()))
        .collect();
    DepReport::new(ctx.os.as_str().to_string(), items)
}

fn check_one(ctx: &Context, id: DepId, python: Option<&PythonInfo>) -> DepStatus {
    let spec = id.spec();

    let (state, path, version) = match id {
        DepId::Python => match python {
            Some(info) => (
                if info.meets_minimum() { DepState::Ok } else { DepState::Outdated },
                Some(info.path.to_string_lossy().into_owned()),
                Some(info.version_string()),
            ),
            None => (DepState::Missing, None, None),
        },
        DepId::Pysubs2 | DepId::FasterWhisper => {
            let dist = id.pip_name().expect("python package without a pip name");
            match python.and_then(|p| python_package_version(&p.path, dist)) {
                Some(v) => (DepState::Ok, None, Some(v)),
                None => (DepState::Missing, None, None),
            }
        }
        _ => match ctx.find_executable(id.as_str()) {
            Some(p) => {
                let version = tool_version(&p, id.version_arg());
                (DepState::Ok, Some(p.to_string_lossy().into_owned()), version)
            }
            None => (DepState::Missing, None, None),
        },
    };

    let plan = if state == DepState::Ok { None } else { plan_for(id, ctx).ok() };

    DepStatus {
        id,
        label: spec.label.to_string(),
        severity: spec.severity,
        state,
        path,
        version,
        purpose: spec.purpose.to_string(),
        download_url: spec.download_url.to_string(),
        install_command: plan.as_ref().map(|p| p.display_command.clone()),
        can_auto_install: plan.as_ref().map(|p| p.auto_runnable).unwrap_or(false),
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::detect::SearchEnv;
    use crate::model::{DepState, Severity};
    use crate::platform::{Os, PackageManager};
    use crate::testutil::make_script;
    use tempfile::tempdir;

    fn ctx(dir: &std::path::Path) -> Context {
        Context {
            os: Os::Linux,
            env: SearchEnv { dirs: vec![dir.to_path_buf()], os: Os::Linux },
            venv_dir: dir.join("no-venv"),
            package_manager: Some(PackageManager::Pacman),
            has_pkexec: true,
        }
    }

    fn item(report: &DepReport, id: DepId) -> &crate::model::DepStatus {
        report.items.iter().find(|i| i.id == id).unwrap()
    }

    fn install_all_tools(dir: &std::path::Path) {
        for t in ["mkvmerge", "mkvextract", "ffmpeg", "ffprobe"] {
            make_script(dir, t, &format!("echo '{t} version 1.0'"));
        }
        make_script(dir, "python3", r#"case "$*" in *faster-whisper*) exit 1;; esac; echo 3.11"#);
    }

    #[test]
    fn healthy_system_has_no_missing_required() {
        let tmp = tempdir().unwrap();
        install_all_tools(tmp.path());
        let report = check_all(&ctx(tmp.path()));

        assert!(!report.has_missing_required);
        assert_eq!(report.os, "linux");
        assert_eq!(report.items.len(), 8);

        let mkv = item(&report, DepId::Mkvmerge);
        assert_eq!(mkv.state, DepState::Ok);
        assert_eq!(mkv.version.as_deref(), Some("mkvmerge version 1.0"));
        assert!(mkv.path.is_some());
        assert!(mkv.install_command.is_none());

        // Optional gaps are reported but do not count as missing-required.
        let whisper = item(&report, DepId::FasterWhisper);
        assert_eq!(whisper.state, DepState::Missing);
        assert_eq!(whisper.severity, Severity::Optional);
        assert_eq!(item(&report, DepId::Mkvpropedit).state, DepState::Missing);
    }

    #[test]
    fn missing_tool_gets_exact_install_command() {
        let tmp = tempdir().unwrap();
        install_all_tools(tmp.path());
        std::fs::remove_file(tmp.path().join("ffmpeg")).unwrap();
        let report = check_all(&ctx(tmp.path()));

        assert!(report.has_missing_required);
        let ff = item(&report, DepId::Ffmpeg);
        assert_eq!(ff.state, DepState::Missing);
        assert_eq!(ff.install_command.as_deref(), Some("sudo pacman -S --needed --noconfirm ffmpeg"));
        assert!(ff.can_auto_install);
        assert_eq!(ff.download_url, "https://ffmpeg.org/download.html");
    }

    #[test]
    fn missing_python_blocks_package_auto_install() {
        let tmp = tempdir().unwrap();
        let report = check_all(&ctx(tmp.path()));
        assert_eq!(item(&report, DepId::Python).state, DepState::Missing);
        let p = item(&report, DepId::Pysubs2);
        assert_eq!(p.state, DepState::Missing);
        assert!(!p.can_auto_install);
        assert!(p.install_command.is_none());
        // ...but Python itself can be installed.
        assert!(item(&report, DepId::Python).can_auto_install);
    }

    #[test]
    fn old_python_is_reported_outdated() {
        let tmp = tempdir().unwrap();
        make_script(tmp.path(), "python3", "echo 3.8");
        let report = check_all(&ctx(tmp.path()));
        let py = item(&report, DepId::Python);
        assert_eq!(py.state, DepState::Outdated);
        assert_eq!(py.version.as_deref(), Some("3.8"));
        assert!(report.has_missing_required);
    }
}
