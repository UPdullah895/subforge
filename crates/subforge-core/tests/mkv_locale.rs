//! MKVToolNix decodes its command line with the locale's charset. Under the C/POSIX locale
//! (services, CI runners, `env -i`) a non-ASCII path used to reach it mangled. This file holds a
//! single test because it changes the process locale variables.

use std::path::Path;

use subforge_core::tools::mkvtoolnix;
use subforge_core::tools::{resolve, ToolId, ToolPaths};

#[cfg(unix)]
#[test]
fn mkvtoolnix_handles_arabic_paths_under_the_c_locale() {
    let paths = ToolPaths::default();
    if resolve(ToolId::Mkvmerge, &paths).is_err() || resolve(ToolId::Mkvextract, &paths).is_err() {
        eprintln!("skipped: MKVToolNix not installed");
        return;
    }
    // Only test in this binary, so nothing else reads the environment concurrently.
    std::env::set_var("LC_ALL", "C");
    std::env::set_var("LANG", "C");
    std::env::remove_var("LC_CTYPE");

    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("مجلد اختبار");
    std::fs::create_dir(&dir).unwrap();
    let mkv = dir.join("عينة clip.mkv");
    std::fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/media/sample.mkv"),
        &mkv,
    )
    .unwrap();

    let info = mkvtoolnix::enumerate_tracks(&paths, mkv.to_str().unwrap()).unwrap();
    assert_eq!(info.tracks.len(), 4);

    let out = dir.join("ترجمة.srt");
    mkvtoolnix::extract_subtitle_track(&paths, mkv.to_str().unwrap(), 3, out.to_str().unwrap())
        .unwrap();
    assert!(std::fs::read_to_string(&out)
        .unwrap()
        .contains("Hello world"));
}
