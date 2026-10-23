use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Required,
    Optional,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DepState {
    Ok,
    Missing,
    Outdated,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DepId {
    Mkvmerge,
    Mkvextract,
    Mkvpropedit,
    Ffmpeg,
    Ffprobe,
}

/// Static, human-readable facts about a dependency.
#[derive(Debug, Clone, Copy)]
pub struct DepSpec {
    pub label: &'static str,
    pub severity: Severity,
    pub purpose: &'static str,
    pub download_url: &'static str,
}

impl DepId {
    /// Display / check order.
    pub const ALL: [DepId; 5] = [
        DepId::Mkvmerge,
        DepId::Mkvextract,
        DepId::Mkvpropedit,
        DepId::Ffmpeg,
        DepId::Ffprobe,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            DepId::Mkvmerge => "mkvmerge",
            DepId::Mkvextract => "mkvextract",
            DepId::Mkvpropedit => "mkvpropedit",
            DepId::Ffmpeg => "ffmpeg",
            DepId::Ffprobe => "ffprobe",
        }
    }

    pub fn parse(s: &str) -> Option<DepId> {
        DepId::ALL.into_iter().find(|id| id.as_str() == s)
    }

    /// Argument that makes an executable print its version.
    pub fn version_arg(self) -> &'static str {
        match self {
            DepId::Ffmpeg | DepId::Ffprobe => "-version",
            _ => "--version",
        }
    }

    pub fn spec(self) -> DepSpec {
        const MKV_URL: &str = "https://mkvtoolnix.download/downloads.html";
        const FFMPEG_URL: &str = "https://ffmpeg.org/download.html";
        match self {
            DepId::Mkvmerge => DepSpec {
                label: "mkvmerge (MKVToolNix)",
                severity: Severity::Required,
                purpose: "Merging subtitle and audio tracks into MKV files",
                download_url: MKV_URL,
            },
            DepId::Mkvextract => DepSpec {
                label: "mkvextract (MKVToolNix)",
                severity: Severity::Required,
                purpose: "Extracting subtitle and audio tracks from MKV files",
                download_url: MKV_URL,
            },
            DepId::Mkvpropedit => DepSpec {
                label: "mkvpropedit (MKVToolNix)",
                severity: Severity::Optional,
                purpose: "Editing track names, languages and flags without remuxing",
                download_url: MKV_URL,
            },
            DepId::Ffmpeg => DepSpec {
                label: "FFmpeg",
                severity: Severity::Required,
                purpose: "Extracting, converting and merging audio",
                download_url: FFMPEG_URL,
            },
            DepId::Ffprobe => DepSpec {
                label: "ffprobe (FFmpeg)",
                severity: Severity::Required,
                purpose: "Reading audio track information",
                download_url: FFMPEG_URL,
            },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DepStatus {
    pub id: DepId,
    pub label: String,
    pub severity: Severity,
    pub state: DepState,
    pub path: Option<String>,
    pub version: Option<String>,
    pub purpose: String,
    pub download_url: String,
    /// Exact command a user can paste into a terminal (shown/copyable in the UI).
    pub install_command: Option<String>,
    /// True when SubForge can run the install itself right now.
    pub can_auto_install: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DepReport {
    pub os: String,
    pub items: Vec<DepStatus>,
    pub has_missing_required: bool,
}

impl DepReport {
    pub fn new(os: String, items: Vec<DepStatus>) -> Self {
        let has_missing_required = items
            .iter()
            .any(|i| i.severity == Severity::Required && i.state != DepState::Ok);
        Self {
            os,
            items,
            has_missing_required,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_round_trip_through_strings() {
        for id in DepId::ALL {
            assert_eq!(DepId::parse(id.as_str()), Some(id));
        }
    }

    #[test]
    fn parse_rejects_unknown_and_injection_attempts() {
        assert_eq!(DepId::parse("nope"), None);
        assert_eq!(DepId::parse("ffmpeg; rm -rf /"), None);
        assert_eq!(DepId::parse(""), None);
    }

    #[test]
    fn serde_names_match_as_str() {
        let json = serde_json::to_string(&DepId::Mkvpropedit).unwrap();
        assert_eq!(json, "\"mkvpropedit\"");
        assert_eq!(DepId::Mkvpropedit.as_str(), "mkvpropedit");
    }

    #[test]
    fn report_has_no_python_entries() {
        let ids: Vec<&str> = DepId::ALL.iter().map(|i| i.as_str()).collect();
        assert_eq!(
            ids,
            ["mkvmerge", "mkvextract", "mkvpropedit", "ffmpeg", "ffprobe"]
        );
        for id in DepId::ALL {
            let s = id.spec();
            let text = format!("{} {} {}", s.label, s.purpose, s.download_url).to_lowercase();
            assert!(
                !text.contains("python") && !text.contains("pysubs2") && !text.contains("whisper"),
                "{text}"
            );
        }
        assert_eq!(DepId::parse("python"), None);
        assert_eq!(DepId::parse("pysubs2"), None);
        assert_eq!(DepId::parse("faster_whisper"), None);
    }

    fn status(id: DepId, state: DepState) -> DepStatus {
        let spec = id.spec();
        DepStatus {
            id,
            label: spec.label.to_string(),
            severity: spec.severity,
            state,
            path: None,
            version: None,
            purpose: spec.purpose.to_string(),
            download_url: spec.download_url.to_string(),
            install_command: None,
            can_auto_install: false,
        }
    }

    #[test]
    fn report_flags_missing_required_only() {
        let optional_missing = DepReport::new(
            "linux".into(),
            vec![
                status(DepId::Ffmpeg, DepState::Ok),
                status(DepId::Mkvpropedit, DepState::Missing),
            ],
        );
        assert!(!optional_missing.has_missing_required);

        let required_missing = DepReport::new(
            "linux".into(),
            vec![status(DepId::Ffmpeg, DepState::Missing)],
        );
        assert!(required_missing.has_missing_required);

        let outdated = DepReport::new(
            "linux".into(),
            vec![status(DepId::Ffprobe, DepState::Outdated)],
        );
        assert!(outdated.has_missing_required);
    }
}
