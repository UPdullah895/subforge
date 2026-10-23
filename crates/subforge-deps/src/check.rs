use crate::context::Context;
use crate::detect::tool_version;
use crate::install::plan_for;
use crate::model::{DepId, DepReport, DepSource, DepState, DepStatus};
use std::path::PathBuf;

/// How one tool resolved, as decided by the caller's resolution rules
/// (custom path -> bundled -> system).
#[derive(Debug, Clone)]
pub enum Resolution {
    Found {
        path: PathBuf,
        source: DepSource,
    },
    Missing,
    /// A custom path that cannot be used; `message` names the setting.
    Invalid {
        path: PathBuf,
        message: String,
    },
}

/// Checks every dependency on the system `PATH` / well-known locations only.
pub fn check_all(ctx: &Context) -> DepReport {
    check_all_with(ctx, |id| match ctx.find_executable(id.as_str()) {
        Some(path) => Resolution::Found {
            path,
            source: DepSource::System,
        },
        None => Resolution::Missing,
    })
}

/// Checks every dependency using `resolve` to locate it. Install help is only
/// offered for tools that are truly missing.
pub fn check_all_with(ctx: &Context, resolve: impl Fn(DepId) -> Resolution) -> DepReport {
    let items = DepId::ALL
        .into_iter()
        .map(|id| check_one(ctx, id, resolve(id)))
        .collect();
    DepReport::new(ctx.os.as_str().to_string(), items)
}

fn check_one(ctx: &Context, id: DepId, resolution: Resolution) -> DepStatus {
    let spec = id.spec();

    let (state, source, path, version, error) = match resolution {
        Resolution::Found { path, source } => {
            let version = tool_version(&path, id.version_arg());
            (
                DepState::Ok,
                Some(source),
                Some(path.to_string_lossy().into_owned()),
                version,
                None,
            )
        }
        Resolution::Missing => (DepState::Missing, None, None, None, None),
        Resolution::Invalid { path, message } => (
            DepState::Invalid,
            Some(DepSource::Custom),
            Some(path.to_string_lossy().into_owned()),
            None,
            Some(message),
        ),
    };

    let plan = if state == DepState::Missing {
        plan_for(id, ctx).ok()
    } else {
        None
    };

    DepStatus {
        id,
        label: spec.label.to_string(),
        severity: spec.severity,
        state,
        source,
        path,
        version,
        purpose: spec.purpose.to_string(),
        download_url: spec.download_url.to_string(),
        install_command: plan.as_ref().map(|p| p.display_command.clone()),
        can_auto_install: plan.as_ref().map(|p| p.auto_runnable).unwrap_or(false),
        error,
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::detect::SearchEnv;
    use crate::model::{DepSource, DepState, Severity};
    use crate::platform::{Os, PackageManager};
    use crate::testutil::make_script;
    use tempfile::tempdir;

    fn ctx(dir: &std::path::Path) -> Context {
        Context {
            os: Os::Linux,
            env: SearchEnv {
                dirs: vec![dir.to_path_buf()],
                os: Os::Linux,
            },
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
    }

    #[test]
    fn healthy_system_has_no_missing_required() {
        let tmp = tempdir().unwrap();
        install_all_tools(tmp.path());
        let report = check_all(&ctx(tmp.path()));

        assert!(!report.has_missing_required);
        assert_eq!(report.os, "linux");
        assert_eq!(report.items.len(), 5);

        let mkv = item(&report, DepId::Mkvmerge);
        assert_eq!(mkv.state, DepState::Ok);
        assert_eq!(mkv.version.as_deref(), Some("mkvmerge version 1.0"));
        assert!(mkv.path.is_some());
        assert!(mkv.install_command.is_none());

        // Optional gaps are reported but do not count as missing-required.
        let propedit = item(&report, DepId::Mkvpropedit);
        assert_eq!(propedit.state, DepState::Missing);
        assert_eq!(propedit.severity, Severity::Optional);
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
        assert_eq!(
            ff.install_command.as_deref(),
            Some("sudo pacman -S --needed --noconfirm ffmpeg")
        );
        assert!(ff.can_auto_install);
        assert_eq!(ff.download_url, "https://ffmpeg.org/download.html");
    }

    #[test]
    fn invalid_custom_path_is_a_failure_without_install_help() {
        let tmp = tempdir().unwrap();
        install_all_tools(tmp.path());
        let bad = tmp.path().join("nope").join("ffmpeg");
        let report = check_all_with(&ctx(tmp.path()), |id| {
            if id == DepId::Ffmpeg {
                Resolution::Invalid {
                    path: bad.clone(),
                    message: "ffmpeg path set in Settings does not exist".into(),
                }
            } else {
                Resolution::Found {
                    path: tmp.path().join(id.as_str()),
                    source: DepSource::Bundled,
                }
            }
        });
        assert!(report.has_invalid_custom);
        assert!(!report.has_missing_required);
        let ff = item(&report, DepId::Ffmpeg);
        assert_eq!(ff.state, DepState::Invalid);
        assert_eq!(ff.source, Some(DepSource::Custom));
        assert!(ff.error.as_deref().unwrap().contains("Settings"));
        assert!(ff.install_command.is_none());
        assert!(!ff.can_auto_install);
        let mkv = item(&report, DepId::Mkvmerge);
        assert_eq!(mkv.source, Some(DepSource::Bundled));
        assert_eq!(mkv.version.as_deref(), Some("mkvmerge version 1.0"));
    }

    #[test]
    fn report_has_no_python_entries() {
        let tmp = tempdir().unwrap();
        install_all_tools(tmp.path());
        let json = serde_json::to_string(&check_all(&ctx(tmp.path())))
            .unwrap()
            .to_lowercase();
        for word in ["python", "pysubs2", "whisper", "venv"] {
            assert!(!json.contains(word), "report mentions {word}: {json}");
        }
    }
}
