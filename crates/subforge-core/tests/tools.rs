use std::collections::HashMap;
use std::path::{Path, PathBuf};

use subforge_core::tools::ffmpeg;
use subforge_core::tools::mkvtoolnix::{self, SubtitleMergeSpec};
use subforge_core::tools::{resolve, resolve_with_env, ToolError, ToolId, ToolPaths, ToolSource};
use subforge_deps::{Os, SearchEnv};

/// Writes an executable `#!/bin/sh` script called after `id` into `dir`.
#[cfg(unix)]
fn fake_tool(dir: &Path, id: ToolId, body: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let p = dir.join(id.exe_name());
    std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    p
}

fn empty_env() -> SearchEnv {
    SearchEnv {
        dirs: vec![],
        os: Os::current(),
    }
}

#[cfg(unix)]
mod resolution {
    use super::*;

    #[test]
    fn custom_path_wins() {
        let custom = tempfile::tempdir().unwrap();
        let bundled = tempfile::tempdir().unwrap();
        let sys = tempfile::tempdir().unwrap();
        let c = fake_tool(custom.path(), ToolId::Mkvmerge, "exit 0");
        fake_tool(bundled.path(), ToolId::Mkvmerge, "exit 0");
        fake_tool(sys.path(), ToolId::Mkvmerge, "exit 0");
        let paths = ToolPaths {
            custom: HashMap::from([(ToolId::Mkvmerge, c.clone())]),
            bundled_dir: Some(bundled.path().to_path_buf()),
        };
        let env = SearchEnv {
            dirs: vec![sys.path().to_path_buf()],
            os: Os::current(),
        };
        let r = resolve_with_env(ToolId::Mkvmerge, &paths, &env).unwrap();
        assert_eq!(r.source, ToolSource::Custom);
        assert_eq!(r.path, c);
        assert_eq!(r.id, ToolId::Mkvmerge);
    }

    #[test]
    fn bundled_used_when_no_custom() {
        let bundled = tempfile::tempdir().unwrap();
        let sys = tempfile::tempdir().unwrap();
        let b = fake_tool(bundled.path(), ToolId::Ffmpeg, "exit 0");
        fake_tool(sys.path(), ToolId::Ffmpeg, "exit 0");
        let paths = ToolPaths {
            custom: HashMap::new(),
            bundled_dir: Some(bundled.path().to_path_buf()),
        };
        let env = SearchEnv {
            dirs: vec![sys.path().to_path_buf()],
            os: Os::current(),
        };
        let r = resolve_with_env(ToolId::Ffmpeg, &paths, &env).unwrap();
        assert_eq!(r.source, ToolSource::Bundled);
        assert_eq!(r.path, b);
    }

    #[test]
    fn system_used_when_no_bundled() {
        let sys = tempfile::tempdir().unwrap();
        let empty_bundled = tempfile::tempdir().unwrap();
        let s = fake_tool(sys.path(), ToolId::Ffprobe, "exit 0");
        let paths = ToolPaths {
            custom: HashMap::new(),
            bundled_dir: Some(empty_bundled.path().to_path_buf()),
        };
        let env = SearchEnv {
            dirs: vec![sys.path().to_path_buf()],
            os: Os::current(),
        };
        let r = resolve_with_env(ToolId::Ffprobe, &paths, &env).unwrap();
        assert_eq!(r.source, ToolSource::System);
        assert_eq!(r.path, s);
    }

    #[test]
    fn not_found_when_nowhere() {
        let err =
            resolve_with_env(ToolId::Mkvextract, &ToolPaths::default(), &empty_env()).unwrap_err();
        assert!(matches!(err, ToolError::NotFound(ToolId::Mkvextract)));
        assert!(err.to_string().contains("mkvextract"));
    }

    #[test]
    fn custom_path_missing_is_error_not_fallback() {
        let bundled = tempfile::tempdir().unwrap();
        fake_tool(bundled.path(), ToolId::Mkvmerge, "exit 0");
        let missing = bundled.path().join("gone").join("mkvmerge");
        let paths = ToolPaths {
            custom: HashMap::from([(ToolId::Mkvmerge, missing.clone())]),
            bundled_dir: Some(bundled.path().to_path_buf()),
        };
        let sys = SearchEnv {
            dirs: vec![bundled.path().to_path_buf()],
            os: Os::current(),
        };
        let err = resolve_with_env(ToolId::Mkvmerge, &paths, &sys).unwrap_err();
        match &err {
            ToolError::CustomPathInvalid { tool, path } => {
                assert_eq!(*tool, ToolId::Mkvmerge);
                assert_eq!(path, &missing);
            }
            other => panic!("expected CustomPathInvalid, got {other:?}"),
        }
        let msg = err.to_string();
        assert!(msg.contains("mkvmerge path set in Settings"), "{msg}");
        assert!(msg.contains(missing.to_string_lossy().as_ref()), "{msg}");
        assert!(msg.contains("Fix or clear it"), "{msg}");
    }

    #[test]
    fn custom_path_that_is_a_directory_is_invalid() {
        let d = tempfile::tempdir().unwrap();
        let paths = ToolPaths {
            custom: HashMap::from([(ToolId::Ffmpeg, d.path().to_path_buf())]),
            bundled_dir: None,
        };
        let err = resolve_with_env(ToolId::Ffmpeg, &paths, &empty_env()).unwrap_err();
        assert!(matches!(err, ToolError::CustomPathInvalid { .. }));
    }

    #[test]
    fn public_resolve_reports_invalid_custom_path() {
        let paths = ToolPaths {
            custom: HashMap::from([(
                ToolId::Ffprobe,
                PathBuf::from("/definitely/not/here/ffprobe"),
            )]),
            bundled_dir: None,
        };
        assert!(matches!(
            resolve(ToolId::Ffprobe, &paths),
            Err(ToolError::CustomPathInvalid { .. })
        ));
    }
}

#[cfg(unix)]
#[allow(clippy::await_holding_lock)] // intentional: serialises exec of fake tools
mod fake_mkv {
    use super::*;
    use std::sync::{Mutex, MutexGuard};

    /// Spawning while another thread has just written a script can fail with
    /// ETXTBSY ("text file busy"), so tests that exec fake tools run serially.
    static EXEC_LOCK: Mutex<()> = Mutex::new(());
    fn serial() -> MutexGuard<'static, ()> {
        EXEC_LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn paths_with(dir: &Path, id: ToolId, body: &str) -> ToolPaths {
        let p = fake_tool(dir, id, body);
        ToolPaths {
            custom: HashMap::from([(id, p)]),
            bundled_dir: None,
        }
    }

    #[test]
    fn mkv_exit_code_1_is_success_with_warnings() {
        let _g = serial();
        let d = tempfile::tempdir().unwrap();
        let paths = paths_with(
            d.path(),
            ToolId::Mkvmerge,
            r#"echo '{"tracks":[]}'; echo 'Warning: something' >&2; exit 1"#,
        );
        let info = mkvtoolnix::enumerate_tracks(&paths, "whatever.mkv").expect("exit 1 is success");
        assert!(info.tracks.is_empty());
        mkvtoolnix::remove_all_subtitles(&paths, "in.mkv", "out.mkv").expect("exit 1 is success");
    }

    #[test]
    fn mkv_exit_code_2_is_failure_with_stdout_and_stderr_in_details() {
        let _g = serial();
        let d = tempfile::tempdir().unwrap();
        let paths = paths_with(
            d.path(),
            ToolId::Mkvmerge,
            "echo 'Error: bad file'; echo 'stderr text' >&2; exit 2",
        );
        let err = mkvtoolnix::enumerate_tracks(&paths, "x.mkv").unwrap_err();
        match err {
            ToolError::Failed {
                tool,
                summary,
                details,
            } => {
                assert_eq!(tool, ToolId::Mkvmerge);
                assert!(summary.contains("Error: bad file"), "{summary}");
                assert!(details.contains("Error: bad file"), "{details}");
                assert!(details.contains("stderr text"), "{details}");
            }
            other => panic!("expected Failed, got {other:?}"),
        }
    }

    #[test]
    fn set_track_flags_builds_mkvpropedit_args_and_accepts_exit_1() {
        let _g = serial();
        let d = tempfile::tempdir().unwrap();
        let log = d.path().join("args.txt");
        let body = format!(
            "for a in \"$@\"; do echo \"$a\" >> '{}'; done; exit 1",
            log.display()
        );
        let paths = paths_with(d.path(), ToolId::Mkvpropedit, &body);
        mkvtoolnix::set_track_flags(
            &paths,
            Path::new("/m/فيلم x.mkv"),
            3,
            &[
                ("name".into(), "Arabic".into()),
                ("flag-default".into(), "1".into()),
            ],
        )
        .unwrap();
        let args: Vec<String> = std::fs::read_to_string(&log)
            .unwrap()
            .lines()
            .map(String::from)
            .collect();
        assert_eq!(
            args,
            [
                "/m/فيلم x.mkv",
                "--edit",
                "track:@3",
                "--set",
                "name=Arabic",
                "--set",
                "flag-default=1"
            ]
        );
    }

    #[test]
    fn missing_tool_surfaces_not_found() {
        let paths = ToolPaths::default();
        // No custom/bundled; the host PATH may have the real tool, so only check
        // the invalid-custom branch deterministically.
        let bad = ToolPaths {
            custom: HashMap::from([(ToolId::Ffprobe, PathBuf::from("/nope/ffprobe"))]),
            bundled_dir: None,
        };
        assert!(matches!(
            ffmpeg::probe_audio_tracks(&bad, "x.mkv"),
            Err(ToolError::CustomPathInvalid {
                tool: ToolId::Ffprobe,
                ..
            })
        ));
        let _ = paths;
    }

    #[tokio::test]
    async fn merge_async_reports_progress_and_accepts_exit_1() {
        let _g = serial();
        let d = tempfile::tempdir().unwrap();
        let paths = paths_with(
            d.path(),
            ToolId::Mkvmerge,
            "echo '#GUI#progress 10%'; echo '#GUI#progress 100%'; exit 1",
        );
        let mut seen = Vec::new();
        mkvtoolnix::merge_subtitles_async(
            &paths,
            "v.mkv",
            &[SubtitleMergeSpec::new("s.srt".into(), Some("eng".into()))],
            "out.mkv",
            None,
            |p| seen.push(p),
        )
        .await
        .unwrap();
        assert_eq!(seen, vec![10.0, 100.0]);
    }

    #[tokio::test]
    async fn merge_async_failure_carries_output() {
        let _g = serial();
        let d = tempfile::tempdir().unwrap();
        let paths = paths_with(
            d.path(),
            ToolId::Mkvmerge,
            "echo 'Error: nope'; echo 'boom' >&2; exit 2",
        );
        let err = mkvtoolnix::merge_subtitles_async(&paths, "v.mkv", &[], "out.mkv", None, |_| {})
            .await
            .unwrap_err();
        match err {
            ToolError::Failed { details, .. } => {
                assert!(
                    details.contains("Error: nope") && details.contains("boom"),
                    "{details}"
                )
            }
            other => panic!("expected Failed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn merge_async_cancel_kills_child() {
        let _g = serial();
        let d = tempfile::tempdir().unwrap();
        let paths = paths_with(d.path(), ToolId::Mkvmerge, "sleep 30");
        let token = subforge_core::CancellationToken::new();
        token.cancel();
        let err = mkvtoolnix::merge_subtitles_async(
            &paths,
            "v.mkv",
            &[],
            "out.mkv",
            Some(&token),
            |_| {},
        )
        .await
        .unwrap_err();
        assert!(matches!(err, ToolError::Cancelled), "{err:?}");
    }
}

#[test]
fn tool_ids_have_platform_exe_names() {
    let n = ToolId::Mkvpropedit.exe_name();
    if cfg!(windows) {
        assert_eq!(n, "mkvpropedit.exe");
    } else {
        assert_eq!(n, "mkvpropedit");
    }
}

#[test]
fn tool_status_reports_error_for_invalid_custom_path() {
    let paths = ToolPaths {
        custom: HashMap::from([(ToolId::Mkvmerge, PathBuf::from("/nope/mkvmerge"))]),
        bundled_dir: None,
    };
    let st = subforge_core::tools::tool_status(&paths);
    assert_eq!(st.len(), 5);
    let m = st.iter().find(|s| s.id == ToolId::Mkvmerge).unwrap();
    assert!(m.source.is_none() && m.path.is_none() && m.version.is_none());
    assert!(m.error.as_deref().unwrap().contains("Fix or clear it"));
}

fn real_tools_enabled() -> bool {
    std::env::var("SUBFORGE_REAL_TOOLS").as_deref() == Ok("1")
}

#[test]
#[ignore = "needs real ffmpeg and mkvtoolnix; run with SUBFORGE_REAL_TOOLS=1 --include-ignored"]
fn tools_handle_unicode_space_paths() {
    if !real_tools_enabled() {
        eprintln!("SUBFORGE_REAL_TOOLS != 1; skipping");
        return;
    }
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/media/sample.mkv");
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("مجلد اختبار");
    std::fs::create_dir_all(&dir).unwrap();
    let video = dir.join("فيلم تجريبي.mkv");
    std::fs::copy(&fixture, &video).unwrap();
    let paths = ToolPaths::default();
    let video_s = video.to_str().unwrap();

    // mkvmerge -J
    let info = mkvtoolnix::enumerate_tracks(&paths, video_s).unwrap();
    let subs: Vec<_> = info
        .tracks
        .iter()
        .filter(|t| t.track_type == "subtitles")
        .collect();
    assert_eq!(subs.len(), 2);
    assert_eq!(
        info.tracks
            .iter()
            .filter(|t| t.track_type == "audio")
            .count(),
        1
    );

    // ffprobe + ffmpeg extraction into a Unicode/space output path
    let audio = ffmpeg::probe_audio_tracks(&paths, video_s).unwrap();
    assert_eq!(audio.len(), 1);
    let out_audio = dir.join("صوت مستخرج.aac");
    ffmpeg::extract_audio(
        &paths,
        video_s,
        audio[0].index,
        out_audio.to_str().unwrap(),
        Some("aac"),
    )
    .unwrap();
    assert!(std::fs::metadata(&out_audio).unwrap().len() > 0);

    // mkvextract all subtitles
    let extracted =
        mkvtoolnix::extract_all_subtitles(&paths, video_s, dir.to_str().unwrap()).unwrap();
    assert_eq!(extracted.len(), 2);
    for f in &extracted {
        assert!(std::fs::metadata(f).unwrap().len() > 0, "{f}");
    }

    // mkvmerge remove + merge, then mkvpropedit
    let stripped = dir.join("بدون ترجمة.mkv");
    mkvtoolnix::remove_all_subtitles(&paths, video_s, stripped.to_str().unwrap()).unwrap();
    let merged = dir.join("مع ترجمة.mkv");
    mkvtoolnix::merge_subtitles(
        &paths,
        stripped.to_str().unwrap(),
        &[SubtitleMergeSpec::new(
            extracted[0].clone(),
            Some("ara".into()),
        )],
        merged.to_str().unwrap(),
    )
    .unwrap();
    let merged_info = mkvtoolnix::enumerate_tracks(&paths, merged.to_str().unwrap()).unwrap();
    assert_eq!(
        merged_info
            .tracks
            .iter()
            .filter(|t| t.track_type == "subtitles")
            .count(),
        1
    );
    mkvtoolnix::set_track_flags(&paths, &merged, 3, &[("name".into(), "عربي".into())]).unwrap();
    let after = mkvtoolnix::enumerate_tracks(&paths, merged.to_str().unwrap()).unwrap();
    let sub = after
        .tracks
        .iter()
        .find(|t| t.track_type == "subtitles")
        .unwrap();
    assert_eq!(
        sub.properties.as_ref().unwrap().track_name.as_deref(),
        Some("عربي")
    );
}
