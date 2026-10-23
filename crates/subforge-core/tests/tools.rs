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
                assert_eq!(summary, "bad file");
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
    fn invalid_custom_path_surfaces_through_wrappers() {
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
    }

    #[test]
    fn ffmpeg_failure_summary_is_the_error_line_not_the_banner() {
        let _g = serial();
        let d = tempfile::tempdir().unwrap();
        let log = d.path().join("args.txt");
        let body = format!(
            "for a in \"$@\"; do echo \"$a\" >> '{}'; done\n\
             echo 'ffmpeg version 6.1 Copyright (c) 2000-2023 the FFmpeg developers' >&2\n\
             echo '  built with gcc 13' >&2\n\
             echo '  configuration: --enable-gpl' >&2\n\
             echo '  libavutil      58. 29.100 / 58. 29.100' >&2\n\
             echo 'input.mkv: No such file or directory' >&2\n\
             exit 1",
            log.display()
        );
        let paths = paths_with(d.path(), ToolId::Ffmpeg, &body);
        let err = ffmpeg::merge_audio(&paths, "input.mkv", "a.aac", "out.mkv").unwrap_err();
        match &err {
            ToolError::Failed {
                tool,
                summary,
                details,
            } => {
                assert_eq!(*tool, ToolId::Ffmpeg);
                assert_eq!(summary, "input.mkv: No such file or directory");
                assert!(details.contains("ffmpeg version"));
            }
            other => panic!("expected Failed, got {other:?}"),
        }
        assert_eq!(
            err.to_string(),
            "ffmpeg failed: input.mkv: No such file or directory"
        );
        let args = std::fs::read_to_string(&log).unwrap();
        assert!(args.starts_with("-hide_banner\n-v\nerror\n"), "{args}");
    }

    fn failed_parts(err: ToolError) -> (ToolId, String, String) {
        match err {
            ToolError::Failed {
                tool,
                summary,
                details,
            } => (tool, summary, details),
            other => panic!("expected Failed, got {other:?}"),
        }
    }

    // Output captured from the real mkvmerge v102.0 (`mkvmerge -o out.mkv junk.txt`).
    #[test]
    fn mkv_failure_summary_skips_banner_and_uses_error_line() {
        let _g = serial();
        let d = tempfile::tempdir().unwrap();
        let body = "echo \"mkvmerge v102.0 ('Little Houses') 64-bit\"\n\
                    echo \"Error: The type of file 'junk.txt' could not be recognized.\"\n\
                    exit 2";
        let paths = paths_with(d.path(), ToolId::Mkvmerge, body);
        let err = mkvtoolnix::remove_tracks(&paths, "junk.txt", &[2], "out.mkv").unwrap_err();
        assert_eq!(err.details().map(|s| s.contains("v102.0")), Some(true));
        let (tool, summary, details) = failed_parts(err);
        assert_eq!(tool, ToolId::Mkvmerge);
        assert_eq!(
            summary,
            "The type of file 'junk.txt' could not be recognized."
        );
        assert!(details.contains("mkvmerge v102.0"), "{details}");
    }

    // `--gui-mode` output of the same failure.
    #[test]
    fn mkv_failure_summary_understands_gui_mode_errors() {
        let _g = serial();
        let d = tempfile::tempdir().unwrap();
        let body = "echo \"mkvmerge v102.0 ('Little Houses') 64-bit\"\n\
                    echo \"#GUI#warning something minor\"\n\
                    echo \"#GUI#error The type of file 'junk.txt' could not be recognized.\"\n\
                    exit 2";
        let paths = paths_with(d.path(), ToolId::Mkvmerge, body);
        let err = mkvtoolnix::remove_tracks(&paths, "junk.txt", &[2], "out.mkv").unwrap_err();
        let (_, summary, _) = failed_parts(err);
        assert_eq!(
            summary,
            "The type of file 'junk.txt' could not be recognized."
        );
    }

    // `mkvmerge -J nonexist.mkv`: exit 2 with the reason in the JSON `errors[]`.
    #[test]
    fn mkv_identify_failure_summary_comes_from_json_errors() {
        let _g = serial();
        let d = tempfile::tempdir().unwrap();
        let body = r#"cat <<'JSON'
{
  "errors": [
    "The file 'nonexist.mkv' could not be opened for reading: open file error.\n"
  ],
  "warnings": []
}
JSON
exit 2"#;
        let paths = paths_with(d.path(), ToolId::Mkvmerge, body);
        let err = mkvtoolnix::enumerate_tracks(&paths, "nonexist.mkv").unwrap_err();
        let (_, summary, details) = failed_parts(err);
        assert_eq!(
            summary,
            "The file 'nonexist.mkv' could not be opened for reading: open file error."
        );
        assert!(details.contains("\"errors\""), "{details}");
    }

    // `-J` exits 0 but reports errors: an error, not "no tracks".
    #[test]
    fn mkv_identify_exit_0_with_errors_is_a_failure() {
        let _g = serial();
        let d = tempfile::tempdir().unwrap();
        let body = r#"echo '{"errors":["Some reason."],"warnings":[],"tracks":[]}'"#;
        let paths = paths_with(d.path(), ToolId::Mkvmerge, body);
        let err = mkvtoolnix::enumerate_tracks(&paths, "x.mkv").unwrap_err();
        let (_, summary, _) = failed_parts(err);
        assert_eq!(summary, "Some reason.");
    }

    // Real mkvmerge v102 on a text file: exit 0, `errors: []`, `recognized: false`.
    #[test]
    fn mkv_identify_unrecognized_file_is_a_failure() {
        let _g = serial();
        let d = tempfile::tempdir().unwrap();
        let body = r#"cat <<'JSON'
{
  "container": {
    "recognized": false,
    "supported": false
  },
  "errors": [],
  "file_name": "junk.txt",
  "identification_format_version": 20,
  "warnings": []
}
JSON"#;
        let paths = paths_with(d.path(), ToolId::Mkvmerge, body);
        let err = mkvtoolnix::enumerate_tracks(&paths, "junk.txt").unwrap_err();
        let (_, summary, _) = failed_parts(err);
        assert!(summary.contains("junk.txt"), "{summary}");
        assert!(summary.contains("not recognized"), "{summary}");
    }

    /// Same checks against the installed mkvmerge, when there is one.
    #[test]
    fn real_mkvmerge_errors_are_summarised() {
        let Some(real) = subforge_deps::find_executable("mkvmerge", &SearchEnv::from_process())
        else {
            eprintln!("mkvmerge not installed; skipping");
            return;
        };
        let d = tempfile::tempdir().unwrap();
        let junk = d.path().join("junk file.txt");
        std::fs::write(&junk, "not a video").unwrap();
        let paths = ToolPaths {
            custom: HashMap::from([(ToolId::Mkvmerge, real)]),
            bundled_dir: None,
        };
        let junk_s = junk.to_string_lossy().into_owned();
        let (_, summary, _) =
            failed_parts(mkvtoolnix::enumerate_tracks(&paths, &junk_s).unwrap_err());
        assert!(summary.contains("junk file.txt"), "{summary}");
        let missing = d.path().join("missing.mkv").to_string_lossy().into_owned();
        let (_, summary, _) =
            failed_parts(mkvtoolnix::enumerate_tracks(&paths, &missing).unwrap_err());
        assert!(summary.contains("could not be opened"), "{summary}");
        let out = d.path().join("out.mkv").to_string_lossy().into_owned();
        let (_, summary, _) =
            failed_parts(mkvtoolnix::remove_tracks(&paths, &junk_s, &[1], &out).unwrap_err());
        assert!(!summary.starts_with("mkvmerge v"), "{summary}");
        // remove_tracks identifies the file first, so either wording may come back.
        assert!(summary.contains("recognized"), "{summary}");
    }

    /// Fake mkvmerge: answers `-J` with four tracks (0 video, 1 audio, 2 and 3
    /// subtitles) and logs every other invocation's argv, one arg per line.
    fn argv_logging_mkvmerge(d: &Path) -> (ToolPaths, PathBuf) {
        let log = d.join("argv.txt");
        let body = format!(
            "if [ \"$1\" = \"-J\" ]; then echo '{}'; exit 0; fi\n\
             : > '{log}'\n\
             for a in \"$@\"; do echo \"$a\" >> '{log}'; done",
            r#"{"tracks":[{"id":0,"type":"video","codec":"V"},{"id":1,"type":"audio","codec":"A"},{"id":2,"type":"subtitles","codec":"S"},{"id":3,"type":"subtitles","codec":"S"}]}"#,
            log = log.display()
        );
        (paths_with(d, ToolId::Mkvmerge, &body), log)
    }

    fn logged(log: &Path) -> Vec<String> {
        std::fs::read_to_string(log)
            .unwrap()
            .lines()
            .map(String::from)
            .collect()
    }

    #[test]
    fn remove_tracks_builds_keep_lists() {
        let _g = serial();
        let d = tempfile::tempdir().unwrap();
        let (paths, log) = argv_logging_mkvmerge(d.path());

        mkvtoolnix::remove_tracks(&paths, "in.mkv", &[2], "out.mkv").unwrap();
        assert_eq!(
            logged(&log),
            ["-o", "out.mkv", "--subtitle-tracks", "3", "in.mkv"]
        );

        // Removing every audio track uses --no-audio.
        mkvtoolnix::remove_tracks(&paths, "in.mkv", &[1], "out.mkv").unwrap();
        assert_eq!(logged(&log), ["-o", "out.mkv", "--no-audio", "in.mkv"]);

        // Removing all subtitles uses --no-subtitles; both kinds combine.
        mkvtoolnix::remove_tracks(&paths, "in.mkv", &[1, 2, 3], "out.mkv").unwrap();
        assert_eq!(
            logged(&log),
            ["-o", "out.mkv", "--no-subtitles", "--no-audio", "in.mkv"]
        );

        // Ids of other types (video) or unknown ids are ignored.
        mkvtoolnix::remove_tracks(&paths, "in.mkv", &[0, 9], "out.mkv").unwrap();
        assert_eq!(logged(&log), ["-o", "out.mkv", "in.mkv"]);
    }

    #[test]
    fn merge_args_flag_sdh_and_default() {
        let _g = serial();
        let d = tempfile::tempdir().unwrap();
        let (paths, log) = argv_logging_mkvmerge(d.path());
        let subs = [SubtitleMergeSpec {
            path: "s.srt".into(),
            language: Some("eng".into()),
            track_name: Some("English SDH".into()),
            hearing_impaired: true,
            make_default: true,
        }];

        mkvtoolnix::merge_subtitles(&paths, "v.mkv", &subs, "out.mkv").unwrap();
        assert_eq!(
            logged(&log),
            [
                "-o",
                "out.mkv",
                "v.mkv",
                "--language",
                "0:eng",
                "--track-name",
                "0:English SDH",
                "--default-track-flag",
                "0:yes",
                "--hearing-impaired-flag",
                "0:yes",
                "s.srt"
            ]
        );
    }

    #[tokio::test]
    async fn async_merge_args_start_with_gui_mode() {
        let _g = serial();
        let d = tempfile::tempdir().unwrap();
        let (paths, log) = argv_logging_mkvmerge(d.path());
        let subs = [SubtitleMergeSpec::new("s.srt".into(), Some("eng".into()))];
        mkvtoolnix::merge_subtitles_async(&paths, "v.mkv", &subs, "out.mkv", None, |_| {})
            .await
            .unwrap();
        let args = logged(&log);
        assert_eq!(&args[..4], ["--gui-mode", "-o", "out.mkv", "v.mkv"]);
        assert!(args.contains(&"0:no".to_string()), "{args:?}");
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

#[cfg(unix)]
mod dependency_report {
    use super::*;
    use subforge_core::tools::dependency_report_with;
    use subforge_deps::{Context, DepId, DepSource, DepState};

    fn ctx_with(env: SearchEnv) -> Context {
        Context {
            os: Os::Linux,
            env,
            package_manager: None,
            has_pkexec: false,
        }
    }

    fn bundled_all(dir: &Path) {
        for id in ToolId::ALL {
            fake_tool(dir, id, &format!("echo '{} version 9.9'", id.name()));
        }
    }

    #[test]
    fn bundled_only_tools_count_as_present() {
        let bundled = tempfile::tempdir().unwrap();
        bundled_all(bundled.path());
        let paths = ToolPaths {
            custom: HashMap::new(),
            bundled_dir: Some(bundled.path().to_path_buf()),
        };
        // Nothing on the system PATH.
        let report = dependency_report_with(&paths, &ctx_with(empty_env()));
        assert!(!report.has_missing_required, "{report:?}");
        assert!(!report.has_invalid_custom);
        for item in &report.items {
            assert_eq!(item.state, DepState::Ok, "{item:?}");
            assert_eq!(item.source, Some(DepSource::Bundled), "{item:?}");
        }
        let mkv = report
            .items
            .iter()
            .find(|i| i.id == DepId::Mkvmerge)
            .unwrap();
        assert_eq!(mkv.version.as_deref(), Some("mkvmerge version 9.9"));
    }

    #[test]
    fn invalid_custom_path_is_a_failure_even_with_bundled_copy() {
        let bundled = tempfile::tempdir().unwrap();
        bundled_all(bundled.path());
        let missing = bundled.path().join("gone").join("mkvpropedit");
        let paths = ToolPaths {
            custom: HashMap::from([(ToolId::Mkvpropedit, missing.clone())]),
            bundled_dir: Some(bundled.path().to_path_buf()),
        };
        let report = dependency_report_with(&paths, &ctx_with(empty_env()));
        assert!(report.has_invalid_custom);
        let item = report
            .items
            .iter()
            .find(|i| i.id == DepId::Mkvpropedit)
            .unwrap();
        assert_eq!(item.state, DepState::Invalid);
        assert_eq!(item.source, Some(DepSource::Custom));
        assert_eq!(
            item.path.as_deref(),
            Some(missing.to_string_lossy().as_ref())
        );
        assert!(item.error.as_deref().unwrap().contains("Fix or clear it"));
        assert!(item.install_command.is_none());
    }

    #[test]
    fn system_and_custom_sources_are_reported() {
        let sys = tempfile::tempdir().unwrap();
        let custom = tempfile::tempdir().unwrap();
        bundled_all(sys.path());
        let c = fake_tool(
            custom.path(),
            ToolId::Ffmpeg,
            "echo 'ffmpeg version custom'",
        );
        let paths = ToolPaths {
            custom: HashMap::from([(ToolId::Ffmpeg, c)]),
            bundled_dir: None,
        };
        let env = SearchEnv {
            dirs: vec![sys.path().to_path_buf()],
            os: Os::current(),
        };
        let report = dependency_report_with(&paths, &ctx_with(env));
        let src = |id: DepId| report.items.iter().find(|i| i.id == id).unwrap().source;
        assert_eq!(src(DepId::Ffmpeg), Some(DepSource::Custom));
        assert_eq!(src(DepId::Ffprobe), Some(DepSource::System));
    }

    #[test]
    fn nothing_anywhere_is_missing_required() {
        let report = dependency_report_with(&ToolPaths::default(), &ctx_with(empty_env()));
        assert!(report.has_missing_required);
        assert!(!report.has_invalid_custom);
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

#[test]
#[ignore = "needs real ffmpeg and mkvtoolnix; run with SUBFORGE_REAL_TOOLS=1 --include-ignored"]
fn tools_handle_unicode_space_paths() {
    assert_eq!(
        std::env::var("SUBFORGE_REAL_TOOLS").as_deref(),
        Ok("1"),
        "this test needs real ffmpeg and MKVToolNix: set SUBFORGE_REAL_TOOLS=1"
    );
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
