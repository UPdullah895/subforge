//! Hidden `--self-test` entry point, used by CI against installed builds:
//!
//! `subforge --self-test <fixtures_dir> <out_dir> [--whisper-cache <dir>]`
//!
//! Runs `subforge_core::selftest` with the bundled tools (resolved like a normal start: the
//! resource dir plus `bin`), prints the report, writes `<out_dir>/selftest.json` and exits with
//! 0 (all steps passed), 1 (a step failed) or 2 (bad arguments). No window is created.

use std::ffi::OsString;
use std::path::PathBuf;

use subforge_core::tools::ToolPaths;

pub const FLAG: &str = "--self-test";
const USAGE: &str = "usage: subforge --self-test <fixtures_dir> <out_dir> [--whisper-cache <dir>]";

#[derive(Debug, PartialEq, Eq)]
pub struct Args {
    pub fixtures: PathBuf,
    pub out: PathBuf,
    pub whisper_cache: Option<PathBuf>,
}

/// `None` when `--self-test` is not the first argument (a normal start); otherwise the parsed
/// arguments or a usage error.
pub fn parse(args: impl IntoIterator<Item = OsString>) -> Option<Result<Args, String>> {
    let mut args = args.into_iter();
    if args.next()? != FLAG {
        return None;
    }
    let mut positional = Vec::new();
    let mut whisper_cache = None;
    while let Some(a) = args.next() {
        if a == "--whisper-cache" {
            match args.next() {
                Some(dir) => whisper_cache = Some(PathBuf::from(dir)),
                None => return Some(Err(format!("--whisper-cache needs a directory\n{USAGE}"))),
            }
        } else {
            positional.push(PathBuf::from(a));
        }
    }
    Some(match <[PathBuf; 2]>::try_from(positional) {
        Ok([fixtures, out]) => Ok(Args {
            fixtures,
            out,
            whisper_cache,
        }),
        Err(_) => Err(USAGE.to_string()),
    })
}

/// Runs the self-test and returns the process exit code.
pub fn run(args: Result<Args, String>, package_info: &tauri::PackageInfo) -> i32 {
    attach_parent_console();
    // A CI runner has no session keyring; API keys are irrelevant here and a
    // Secret Service client can stall trying to reach one.
    std::env::set_var("SUBFORGE_DISABLE_KEYRING", "1");
    let args = match args {
        Ok(a) => a,
        Err(e) => {
            eprintln!("{e}");
            return 2;
        }
    };
    // Same rule as the normal start-up in `lib.rs`: `<resource dir>/bin`, if it exists.
    let bundled_dir = tauri::utils::platform::resource_dir(package_info, &tauri::Env::default())
        .ok()
        .map(|r| r.join("bin"))
        .filter(|bin| bin.is_dir());
    if let Some(bin) = &bundled_dir {
        crate::tools::set_bundled_dir(bin.clone());
    }
    let paths = ToolPaths {
        custom: Default::default(),
        bundled_dir,
    };
    let cache = args
        .whisper_cache
        .unwrap_or_else(subforge_core::whisper::default_cache_dir);
    let report = subforge_core::selftest::run(&paths, &args.fixtures, &args.out, &cache);
    match serde_json::to_string_pretty(&report) {
        Ok(json) => println!("{json}"),
        Err(e) => eprintln!("could not print the report: {e}"),
    }
    if report.ok {
        0
    } else {
        1
    }
}

/// Release builds on Windows use the GUI subsystem, which starts with no console. When the
/// self-test is launched from a terminal without redirection, attach to that terminal so the
/// report is visible. Redirected output (a file or pipe) is left alone.
#[cfg(windows)]
fn attach_parent_console() {
    use std::ffi::c_void;
    #[link(name = "kernel32")]
    extern "system" {
        fn GetStdHandle(n_std_handle: u32) -> *mut c_void;
        fn AttachConsole(dw_process_id: u32) -> i32;
    }
    const STD_OUTPUT_HANDLE: u32 = -11i32 as u32;
    const ATTACH_PARENT_PROCESS: u32 = u32::MAX;
    // SAFETY: plain Win32 calls with constant arguments; failure is harmless (no console).
    unsafe {
        if GetStdHandle(STD_OUTPUT_HANDLE).is_null() {
            AttachConsole(ATTACH_PARENT_PROCESS);
        }
    }
}

#[cfg(not(windows))]
fn attach_parent_console() {}

#[cfg(test)]
mod tests {
    use super::*;

    fn os(args: &[&str]) -> Vec<OsString> {
        args.iter().map(OsString::from).collect()
    }

    #[test]
    fn normal_start_is_not_a_self_test() {
        assert_eq!(parse(os(&[])), None);
        assert_eq!(parse(os(&["/some/file.mkv"])), None);
    }

    #[test]
    fn parses_dirs_and_whisper_cache() {
        let got = parse(os(&[
            "--self-test",
            "fixtures dir",
            "مجلد out",
            "--whisper-cache",
            "cache",
        ]));
        assert_eq!(
            got,
            Some(Ok(Args {
                fixtures: "fixtures dir".into(),
                out: "مجلد out".into(),
                whisper_cache: Some("cache".into()),
            }))
        );
        let got = parse(os(&["--self-test", "--whisper-cache", "c", "f", "o"]))
            .unwrap()
            .unwrap();
        assert_eq!(got.fixtures, PathBuf::from("f"));
        assert_eq!(got.whisper_cache, Some(PathBuf::from("c")));
    }

    #[test]
    fn missing_arguments_are_a_usage_error() {
        assert!(matches!(parse(os(&["--self-test"])), Some(Err(_))));
        assert!(matches!(parse(os(&["--self-test", "f"])), Some(Err(_))));
        assert!(matches!(
            parse(os(&["--self-test", "f", "o", "--whisper-cache"])),
            Some(Err(_))
        ));
        assert!(matches!(
            parse(os(&["--self-test", "f", "o", "extra"])),
            Some(Err(_))
        ));
    }
}
