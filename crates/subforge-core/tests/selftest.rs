use std::path::{Path, PathBuf};

use subforge_core::selftest::{self, SelfTestReport, StepResult};
use subforge_core::tools::ToolPaths;

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn read_report(out: &Path) -> serde_json::Value {
    let text = std::fs::read_to_string(out.join("selftest.json")).unwrap();
    serde_json::from_str(&text).unwrap()
}

#[test]
fn selftest_report_fails_when_tool_not_bundled() {
    let out = tempfile::tempdir().unwrap();
    let cache = tempfile::tempdir().unwrap();
    let report = selftest::run(&ToolPaths::default(), &fixtures(), out.path(), cache.path());

    assert!(!report.ok);
    let first_failure = report.steps.iter().find(|s| !s.ok).expect("a failing step");
    assert_eq!(first_failure.name, "tools_bundled");
    assert!(
        first_failure.detail.contains("ffmpeg"),
        "detail names the tool: {}",
        first_failure.detail
    );
    // Nothing else runs against tools that are not the bundled ones.
    assert_eq!(report.steps.len(), 1);

    let json = read_report(out.path());
    assert_eq!(json["ok"], false);
    assert_eq!(json["steps"][0]["name"], "tools_bundled");
}

#[test]
fn selftest_json_shape() {
    let report = SelfTestReport {
        steps: vec![StepResult {
            name: "list_tracks".into(),
            ok: true,
            millis: 12,
            detail: "4 tracks".into(),
        }],
        ok: true,
        device_used: Some("cpu".into()),
    };
    let v = serde_json::to_value(&report).unwrap();
    let obj = v.as_object().unwrap();
    let mut keys: Vec<_> = obj.keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(keys, ["device_used", "ok", "steps"]);
    assert_eq!(v["device_used"], "cpu");
    let step = v["steps"][0].as_object().unwrap();
    let mut keys: Vec<_> = step.keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(keys, ["detail", "millis", "name", "ok"]);
    assert_eq!(v["steps"][0]["millis"], 12);

    let back: SelfTestReport = serde_json::from_value(v).unwrap();
    assert_eq!(back, report);
}

/// Full scenario with the machine's own ffmpeg/MKVToolNix posing as the bundled copies.
/// Needs those tools on PATH and network access (or a filled cache) for the `tiny` model:
/// `SUBFORGE_WHISPER_CACHE=/path cargo test --test selftest -- --include-ignored`.
#[cfg(unix)]
#[test]
#[ignore = "needs ffmpeg/MKVToolNix on PATH and the tiny whisper model (network)"]
fn selftest_full_run_with_local_tools() {
    use subforge_core::tools::{resolve, ToolId};

    let bin = tempfile::tempdir().unwrap();
    for id in ToolId::ALL {
        let system = resolve(id, &ToolPaths::default()).expect("tool on PATH");
        std::os::unix::fs::symlink(&system.path, bin.path().join(id.exe_name())).unwrap();
    }
    let paths = ToolPaths {
        bundled_dir: Some(bin.path().to_path_buf()),
        ..ToolPaths::default()
    };
    let out = tempfile::tempdir().unwrap();
    // SUBFORGE_SELFTEST_OUT keeps the outputs for inspection.
    let out_dir = std::env::var_os("SUBFORGE_SELFTEST_OUT")
        .map(PathBuf::from)
        .unwrap_or_else(|| out.path().join("self test out"));
    let cache = std::env::var_os("SUBFORGE_WHISPER_CACHE")
        .map(PathBuf::from)
        .unwrap_or_else(subforge_core::whisper::default_cache_dir);

    let report = selftest::run(&paths, &fixtures(), &out_dir, &cache);
    for s in &report.steps {
        eprintln!(
            "{:<22} {:<5} {:>6} ms  {}",
            s.name, s.ok, s.millis, s.detail
        );
    }
    assert!(report.ok);
    let names: Vec<_> = report.steps.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, selftest::STEP_NAMES);
    assert!(report.device_used.is_some());
    assert_eq!(read_report(&out_dir)["ok"], true);
}
