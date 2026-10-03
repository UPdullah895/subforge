use crate::Episode;

/// Sanitize a filename component against Windows-reserved characters,
/// even on Linux/macOS builds, since renamed files may later be moved
/// to or shared with a Windows machine.
pub fn sanitize_filename(name: &str) -> String {
    name.chars()
        .map(|c| match c {
            '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c => c,
        })
        .collect::<String>()
        .trim()
        .to_string()
}

/// Apply a naming pattern with tokens: {title} {s} {e} {episodeTitle} {quality}.
/// {s}/{e} are zero-padded to 2 digits.
pub fn apply_pattern(pattern: &str, title: &str, episode: &Episode, quality: &str) -> String {
    pattern
        .replace("{title}", title)
        .replace("{s}", &format!("{:02}", episode.season))
        .replace("{e}", &format!("{:02}", episode.episode))
        .replace("{episodeTitle}", &episode.title)
        .replace("{quality}", quality)
}

/// Build a sanitized output filename (including extension) from a pattern.
pub fn build_output_name(
    pattern: &str,
    title: &str,
    episode: &Episode,
    tags: &[String],
    extension: &str,
) -> String {
    let quality = tags.first().map(String::as_str).unwrap_or("");
    let name = apply_pattern(pattern, title, episode, quality);
    format!("{}.{}", sanitize_filename(&name), extension.trim_start_matches('.'))
}

/// Checks a batch of `(old_path, new_path)` renames *before* touching disk:
/// every rename must stay inside its folder (a typed name like `../x` or
/// `a/b` must not move a file), the source must still exist, no two renames
/// may produce the same target, and nothing already on disk may be replaced
/// (`std::fs::rename` silently overwrites on Unix).
pub fn validate_rename_mapping(mapping: &[(String, String)]) -> Result<(), String> {
    use std::collections::HashSet;
    use std::path::Path;

    let mut targets: HashSet<String> = HashSet::new();
    for (old, new) in mapping {
        let (old_p, new_p) = (Path::new(old), Path::new(new));

        if old_p.parent() != new_p.parent() {
            return Err(format!(
                "\"{}\" would be moved to another folder; names can't contain slashes.",
                old_p.file_name().and_then(|n| n.to_str()).unwrap_or(old)
            ));
        }
        let new_name = new_p.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if new_name.is_empty() || new_name == "." || new_name == ".." {
            return Err(format!("Invalid new name for \"{old}\"."));
        }
        if !old_p.exists() {
            return Err(format!("\"{old}\" no longer exists. Rescan the folder and try again."));
        }
        if old == new {
            continue;
        }
        if !targets.insert(new.to_lowercase()) {
            return Err(format!("Two files would both be renamed to \"{new_name}\"."));
        }
        // Case-only renames ("a.mkv" -> "A.mkv") hit the same file on
        // case-insensitive filesystems; that's fine, anything else is a clash.
        let case_only = old.to_lowercase() == new.to_lowercase();
        if new_p.exists() && !case_only {
            return Err(format!("\"{new_name}\" already exists; it would be overwritten."));
        }
    }
    Ok(())
}

/// Validates, then performs the renames. Returns how many were renamed.
pub fn apply_rename_mapping(mapping: &[(String, String)]) -> Result<usize, String> {
    validate_rename_mapping(mapping)?;
    let mut renamed = 0;
    for (old, new) in mapping {
        if old == new {
            continue;
        }
        std::fs::rename(old, new).map_err(|e| format!("Failed to rename {old}: {e}"))?;
        renamed += 1;
    }
    Ok(renamed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitizes_reserved_characters() {
        assert_eq!(sanitize_filename(r#"Show: The "Return"? <2>|3\4/5*"#), "Show_ The _Return__ _2__3_4_5_");
    }

    #[test]
    fn applies_pattern_tokens() {
        let ep = Episode {
            season: 1,
            episode: 2,
            title: "Pilot".to_string(),
        };
        let out = apply_pattern("{title} - S{s}E{e} - {episodeTitle}", "My Show", &ep, "1080p");
        assert_eq!(out, "My Show - S01E02 - Pilot");
    }

    #[test]
    fn builds_sanitized_output_name() {
        let ep = Episode {
            season: 1,
            episode: 2,
            title: "Colon: Test".to_string(),
        };
        let out = build_output_name(
            "{title} - S{s}E{e} - {episodeTitle}",
            "My Show",
            &ep,
            &["1080p".to_string()],
            "srt",
        );
        assert_eq!(out, "My Show - S01E02 - Colon_ Test.srt");
    }

    fn tmp_dir(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("subforge-rename-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn pair(a: &std::path::Path, b: &std::path::Path) -> (String, String) {
        (a.to_string_lossy().into(), b.to_string_lossy().into())
    }

    #[test]
    fn rejects_rename_that_would_leave_the_folder() {
        let d = tmp_dir("escape");
        let a = d.join("a.mkv");
        std::fs::write(&a, b"x").unwrap();
        let err = validate_rename_mapping(&[pair(&a, &d.join("sub").join("b.mkv"))]).unwrap_err();
        assert!(err.contains("another folder"), "{err}");
    }

    #[test]
    fn rejects_overwriting_existing_file_and_duplicate_targets() {
        let d = tmp_dir("clash");
        let (a, b, c) = (d.join("a.mkv"), d.join("b.mkv"), d.join("c.mkv"));
        for f in [&a, &b] {
            std::fs::write(f, b"x").unwrap();
        }
        assert!(validate_rename_mapping(&[pair(&a, &b)]).unwrap_err().contains("already exists"));
        assert!(validate_rename_mapping(&[pair(&a, &c), pair(&b, &c)])
            .unwrap_err()
            .contains("Two files"));
    }

    #[test]
    fn renames_when_valid() {
        let d = tmp_dir("ok");
        let (a, b) = (d.join("a.mkv"), d.join("b.mkv"));
        std::fs::write(&a, b"x").unwrap();
        assert_eq!(apply_rename_mapping(&[pair(&a, &b)]).unwrap(), 1);
        assert!(b.exists() && !a.exists());
    }
}
