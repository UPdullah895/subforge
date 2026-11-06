//! Sonarr/Radarr webhooks: the second trigger, and the cheapest one to get right.
//!
//! The *arrs post the whole episode, including `mediaInfo.subtitles` and `mediaInfo.audioLanguages`,
//! so "does this file already have Arabic?" can be answered from the payload — before an mkvmerge
//! call, a whisper load or a GPU second is spent. That is the difference between a pipeline that is
//! free to run on every import and one that costs something per episode.
//!
//! Two designs are deliberately not copied from Bazarr: the payload is taken whole (Bazarr keeps
//! only `episodeFiles[].id` and skips the event when its own database has not caught up, which is
//! why "the webhook fired and nothing happened" is a recurring complaint), and the handler is
//! stateless, so a first-contact install works.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use subforge_pipeline::{normalize_language, Policy};

use crate::error::AppError;
use crate::pipeline::PipelineConfig;

/// Events worth acting on. A grab is not a file yet; a delete has nothing left to subtitle.
const INTERESTING_EVENTS: &[&str] = &["Download", "Upgrade", "Rename"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ArrKind {
    Sonarr,
    Radarr,
}

impl ArrKind {
    pub fn from_path(path: &str) -> Option<Self> {
        let path = path.trim_end_matches('/');
        if path.ends_with("sonarr") {
            Some(Self::Sonarr)
        } else if path.ends_with("radarr") {
            Some(Self::Radarr)
        } else {
            None
        }
    }
}

/// One file the payload points at, with what the *arr already knows about its tracks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HookFile {
    pub path: String,
    /// Subtitle languages the *arr reports inside the file, normalised.
    #[serde(default)]
    pub embedded_subtitles: Vec<String>,
    /// Audio languages, normalised.
    #[serde(default)]
    pub audio_languages: Vec<String>,
}

/// The parsed event, independent of which *arr sent it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HookEvent {
    pub kind: ArrKind,
    pub event_type: String,
    pub title: Option<String>,
    pub files: Vec<HookFile>,
    /// Target languages from the policy that no file in this event already carries.
    pub missing: Vec<String>,
    /// True when every file already has every target language.
    pub already_present: bool,
}

fn languages(value: Option<&Value>, key: &str) -> Vec<String> {
    value
        .and_then(|info| info.get(key))
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .filter_map(Value::as_str)
                .map(normalize_language)
                .filter(|code| !code.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

fn file_from(value: &Value) -> Option<HookFile> {
    let path = value.get("path")?.as_str()?.to_string();
    let media_info = value.get("mediaInfo");
    Some(HookFile {
        path,
        embedded_subtitles: languages(media_info, "subtitles"),
        audio_languages: languages(media_info, "audioLanguages"),
    })
}

/// Reads either *arr's payload into one shape. Unknown fields are ignored on purpose: the *arrs
/// add fields between versions, and a webhook that 400s on an extra key is worse than one that
/// ignores it.
pub fn parse(kind: ArrKind, payload: &Value) -> Result<HookEvent, String> {
    let event_type = payload
        .get("eventType")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    if event_type.is_empty() {
        return Err("the payload has no eventType".to_string());
    }

    let (title, files) = match kind {
        ArrKind::Sonarr => {
            let title = payload
                .get("series")
                .and_then(|series| series.get("title"))
                .and_then(Value::as_str)
                .map(str::to_string);
            // Always an array, even for a single episode: older builds sent one object.
            let files = payload
                .get("episodeFiles")
                .and_then(Value::as_array)
                .map(|list| list.iter().filter_map(file_from).collect())
                .unwrap_or_default();
            (title, files)
        }
        ArrKind::Radarr => {
            let title = payload
                .get("movie")
                .and_then(|movie| movie.get("title"))
                .and_then(Value::as_str)
                .map(str::to_string);
            let files = payload
                .get("movieFile")
                .and_then(file_from)
                .into_iter()
                .collect();
            (title, files)
        }
    };

    Ok(HookEvent {
        kind,
        event_type,
        title,
        files,
        missing: Vec::new(),
        already_present: false,
    })
}

/// Applies the policy: which target languages are missing, and whether the event is a no-op.
pub fn apply_policy(mut event: HookEvent, policy: &Policy) -> HookEvent {
    let targets: Vec<String> = policy
        .target_languages
        .iter()
        .map(|code| normalize_language(code))
        .filter(|code| !code.is_empty())
        .collect();

    let mut missing: Vec<String> = Vec::new();
    for target in &targets {
        let present_everywhere = !event.files.is_empty()
            && event
                .files
                .iter()
                .all(|file| file.embedded_subtitles.iter().any(|have| have == target));
        if !present_everywhere && !missing.contains(target) {
            missing.push(target.clone());
        }
    }

    event.already_present = !event.files.is_empty() && missing.is_empty();
    event.missing = missing;
    event
}

fn file_has(file: &HookFile, targets: &[String]) -> bool {
    targets.iter().all(|target| {
        let wanted = normalize_language(target);
        file.embedded_subtitles.iter().any(|have| have == &wanted)
    })
}

/// `POST /api/hooks/sonarr` and `POST /api/hooks/radarr`.
pub async fn post_hook(
    Path(kind): Path<String>,
    State(state): State<crate::AppState>,
    body: String,
) -> Result<Json<Value>, AppError> {
    let Some(kind) = ArrKind::from_path(&kind) else {
        return Err(AppError::new(
            StatusCode::NOT_FOUND,
            "unknown webhook source (use sonarr or radarr)",
        ));
    };
    let payload: Value = serde_json::from_str(&body).map_err(|error| {
        AppError::new(
            StatusCode::BAD_REQUEST,
            format!("the webhook body is not JSON: {error}"),
        )
    })?;

    let config = PipelineConfig::load();
    let parsed =
        parse(kind, &payload).map_err(|error| AppError::new(StatusCode::BAD_REQUEST, error))?;
    let event = apply_policy(parsed, &config.policy);

    if !INTERESTING_EVENTS.contains(&event.event_type.as_str()) {
        return Ok(Json(serde_json::json!({
            "accepted": 0,
            "reason": format!("event {} needs no subtitles", event.event_type),
            "event": event,
        })));
    }
    if event.files.is_empty() {
        // *arr "Test" buttons send an event with no file: answer 200 so they go green.
        return Ok(Json(serde_json::json!({
            "accepted": 0,
            "reason": "the payload names no file",
            "event": event,
        })));
    }

    let handle = tokio::runtime::Handle::current();
    let mut queued = 0usize;
    let mut skipped = 0usize;
    for file in &event.files {
        if file_has(file, &config.policy.target_languages) {
            skipped += 1;
            continue;
        }
        crate::pipeline::watch::handle_file(
            std::path::Path::new(&file.path),
            &config.policy,
            &state.scheduler,
            &state.job_queue,
            &handle,
        );
        queued += 1;
    }

    Ok(Json(serde_json::json!({
        "accepted": queued,
        "skipped": skipped,
        "event": event,
    })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sonarr_payload(event: &str, subtitles: &[&str]) -> Value {
        json!({
            "eventType": event,
            "series": {"title": "Bleach", "tvdbId": 74796, "tmdbId": 30984, "type": "Anime"},
            "episodes": [{"seasonNumber": 17, "episodeNumber": 46, "tvdbId": 1}],
            "episodeFiles": [{
                "path": "/media/tv/Bleach/S17E46.mkv",
                "relativePath": "S17E46.mkv",
                "mediaInfo": {"subtitles": subtitles, "audioLanguages": ["ja", "ar"]}
            }],
            "isUpgrade": false
        })
    }

    #[test]
    fn a_sonarr_payload_yields_the_file_and_its_track_languages() {
        let event = parse(ArrKind::Sonarr, &sonarr_payload("Download", &["eng"])).unwrap();
        assert_eq!(event.event_type, "Download");
        assert_eq!(event.title.as_deref(), Some("Bleach"));
        assert_eq!(event.files.len(), 1);
        assert_eq!(event.files[0].path, "/media/tv/Bleach/S17E46.mkv");
        assert_eq!(event.files[0].embedded_subtitles, vec!["en"]);
        assert_eq!(event.files[0].audio_languages, vec!["ja", "ar"]);
    }

    #[test]
    fn a_radarr_payload_uses_its_singular_movie_file() {
        let payload = json!({
            "eventType": "Download",
            "movie": {"title": "Dune", "tmdbId": 438631},
            "movieFile": {"path": "/media/movies/Dune.mkv", "mediaInfo": {"subtitles": []}}
        });
        let event = parse(ArrKind::Radarr, &payload).unwrap();
        assert_eq!(event.title.as_deref(), Some("Dune"));
        assert_eq!(event.files.len(), 1);
        assert_eq!(event.files[0].path, "/media/movies/Dune.mkv");
    }

    #[test]
    fn a_season_pack_yields_every_file() {
        let payload = json!({
            "eventType": "Download",
            "series": {"title": "Show"},
            "episodeFiles": [
                {"path": "/media/tv/Show/E01.mkv", "mediaInfo": {"subtitles": ["eng"]}},
                {"path": "/media/tv/Show/E02.mkv", "mediaInfo": {"subtitles": []}}
            ]
        });
        let event = parse(ArrKind::Sonarr, &payload).unwrap();
        assert_eq!(event.files.len(), 2);
    }

    #[test]
    fn an_episode_that_already_has_the_language_is_a_no_op() {
        let event = parse(ArrKind::Sonarr, &sonarr_payload("Download", &["ara"])).unwrap();
        let event = apply_policy(event, &Policy::arabic());
        assert!(event.already_present);
        assert!(event.missing.is_empty());
    }

    #[test]
    fn a_file_without_the_language_is_missing_it() {
        let event = parse(ArrKind::Sonarr, &sonarr_payload("Download", &["eng"])).unwrap();
        let event = apply_policy(event, &Policy::arabic());
        assert!(!event.already_present);
        assert_eq!(event.missing, vec!["ar"]);
    }

    #[test]
    fn a_season_pack_needs_work_if_any_episode_does() {
        let payload = json!({
            "eventType": "Download",
            "series": {"title": "Show"},
            "episodeFiles": [
                {"path": "/media/tv/Show/E01.mkv", "mediaInfo": {"subtitles": ["ar"]}},
                {"path": "/media/tv/Show/E02.mkv", "mediaInfo": {"subtitles": ["en"]}}
            ]
        });
        let event = apply_policy(parse(ArrKind::Sonarr, &payload).unwrap(), &Policy::arabic());
        assert!(!event.already_present, "one episode is enough to need work");
        assert_eq!(event.missing, vec!["ar"]);
    }

    #[test]
    fn per_file_decisions_do_not_submit_the_satisfied_episode() {
        let satisfied = HookFile {
            path: "/media/tv/Show/E01.mkv".to_string(),
            embedded_subtitles: vec!["ar".to_string()],
            audio_languages: vec!["ja".to_string()],
        };
        let needs = HookFile {
            path: "/media/tv/Show/E02.mkv".to_string(),
            embedded_subtitles: vec!["en".to_string()],
            audio_languages: vec!["ja".to_string()],
        };
        let targets = vec!["ar".to_string()];
        assert!(file_has(&satisfied, &targets));
        assert!(!file_has(&needs, &targets));
    }

    #[test]
    fn an_event_without_a_type_is_an_error_not_a_guess() {
        assert!(parse(ArrKind::Sonarr, &json!({"series": {}})).is_err());
    }

    #[test]
    fn extra_fields_from_a_newer_arr_version_are_ignored() {
        let mut payload = sonarr_payload("Download", &["eng"]);
        payload["somethingNew"] = json!({"nested": [1, 2, 3]});
        payload["episodeFiles"][0]["newField"] = json!("x");
        assert!(parse(ArrKind::Sonarr, &payload).is_ok());
    }

    #[test]
    fn the_path_names_the_source() {
        assert_eq!(ArrKind::from_path("sonarr"), Some(ArrKind::Sonarr));
        assert_eq!(ArrKind::from_path("radarr/"), Some(ArrKind::Radarr));
        assert_eq!(ArrKind::from_path("lidarr"), None);
    }
}
