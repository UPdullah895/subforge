//! Online translation providers (Ollama, LibreTranslate, DeepL, OpenAI,
//! Google). This is the single shared copy; it never reads application
//! config and never shells out. Subtitle extraction/rebuild lives in
//! [`crate::lines`].

use crate::lines::SubtitleLine;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum TranslateError {
    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("API error: {0}")]
    Api(String),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    /// Stable, locale-agnostic error code for a refused/unreachable connection
    /// (e.g. the target service isn't running). The frontend maps this exact
    /// string to a friendly, localized message rather than showing the raw
    /// reqwest error.
    #[error("connect_failed")]
    ConnectFailed,
}

/// Send a request, distinguishing a connection-refused/unreachable-host
/// failure from other HTTP errors so callers can surface a friendly message
/// instead of a raw reqwest error string.
async fn send_checked(
    request: reqwest::RequestBuilder,
) -> Result<reqwest::Response, TranslateError> {
    match request.send().await {
        Ok(resp) => Ok(resp),
        Err(e) if e.is_connect() => Err(TranslateError::ConnectFailed),
        Err(e) => Err(TranslateError::Http(e)),
    }
}

fn client(timeout_secs: u64) -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(timeout_secs))
        .build()
        .unwrap_or_default()
}

const OPENAI_BASE: &str = "https://api.openai.com";
const GOOGLE_BASE: &str = "https://translation.googleapis.com";
const DEEPL_FREE_BASE: &str = "https://api-free.deepl.com";
const DEEPL_PAID_BASE: &str = "https://api.deepl.com";

fn deepl_base(api_key: &str) -> &'static str {
    if api_key.ends_with(":fx") {
        DEEPL_FREE_BASE
    } else {
        DEEPL_PAID_BASE
    }
}

// ─── Ollama ───

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OllamaModel {
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OllamaTagResponse {
    pub models: Vec<OllamaModel>,
}

/// Parse Ollama response content into a Vec of translated strings.
/// Attempts multiple parsing strategies:
/// 1. Direct JSON array
/// 2. Array within markdown fences (```json...```)
/// 3. Array extracted from text by finding [ and ]
/// 4. Plain lines (if count matches expected)
fn parse_ollama_translations(content: &str, expected: usize) -> Result<Vec<String>, String> {
    let trimmed = content
        .trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();

    if trimmed.is_empty() {
        return Err("empty response".to_string());
    }

    // Try direct JSON array parse
    if let Ok(translations) = serde_json::from_str::<Vec<String>>(trimmed) {
        return Ok(translations);
    }

    // Try extracting array from text (find [ and ])
    if let Some(start) = trimmed.find('[') {
        if let Some(end) = trimmed.rfind(']') {
            if start < end {
                let array_str = &trimmed[start..=end];
                if let Ok(translations) = serde_json::from_str::<Vec<String>>(array_str) {
                    return Ok(translations);
                }
            }
        }
    }

    // Fallback: split into lines and use if count matches
    let lines: Vec<String> = trimmed
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .map(|l| l.to_string())
        .collect();
    if lines.len() == expected {
        return Ok(lines);
    }

    // All attempts failed. Truncate by characters, not bytes — model output
    // is often Arabic/CJK, and a byte slice at a non-boundary would panic.
    let snippet: String = trimmed.chars().take(120).collect();
    let ellipsis = if trimmed.chars().count() > 120 {
        "..."
    } else {
        ""
    };
    Err(format!("unparseable response: {}{}", snippet, ellipsis))
}

pub async fn detect_ollama(host: &str) -> Result<OllamaTagResponse, TranslateError> {
    let url = format!("{}/api/tags", host.trim_end_matches('/'));
    let resp = send_checked(client(10).get(&url)).await?;
    if !resp.status().is_success() {
        return Err(TranslateError::Api(format!(
            "Ollama returned {}",
            resp.status()
        )));
    }
    let data: OllamaTagResponse = resp.json().await?;
    Ok(data)
}

/// Language label for an LLM prompt: models understand "Arabic" better than "ar".
fn lang_label(code: &str) -> &str {
    subforge_metadata::languages::english_name(code).unwrap_or(code)
}

fn lines_as_json(lines: &[SubtitleLine]) -> String {
    let texts: Vec<&str> = lines.iter().map(|l| l.text.as_str()).collect();
    serde_json::to_string(&texts).unwrap_or_else(|_| "[]".to_string())
}

/// Run `call` over `lines`; when a batch comes back with the wrong number of
/// entries (models merge or drop cues) or unparseable, split it in half and
/// retry, down to single cues, so one bad batch never misaligns or fails the
/// whole file. Connection/HTTP errors are not retried.
async fn translate_resilient<'a, F, Fut>(
    lines: &'a [SubtitleLine],
    mut call: F,
) -> Result<Vec<String>, TranslateError>
where
    F: FnMut(&'a [SubtitleLine]) -> Fut,
    Fut: std::future::Future<Output = Result<Vec<String>, TranslateError>>,
{
    let mut out = Vec::with_capacity(lines.len());
    let mut queue: std::collections::VecDeque<&'a [SubtitleLine]> =
        std::collections::VecDeque::new();
    queue.push_back(lines);
    while let Some(chunk) = queue.pop_front() {
        if chunk.is_empty() {
            continue;
        }
        match call(chunk).await {
            Ok(t) if t.len() == chunk.len() => out.extend(t),
            Ok(t) if chunk.len() == 1 && !t.is_empty() => out.push(t.join(" ")),
            Ok(_) | Err(TranslateError::Api(_)) | Err(TranslateError::Json(_))
                if chunk.len() > 1 =>
            {
                let (a, b) = chunk.split_at(chunk.len() / 2);
                queue.push_front(b);
                queue.push_front(a);
            }
            Ok(_) => {
                return Err(TranslateError::Api(
                    "model returned no translation for a cue".to_string(),
                ))
            }
            Err(e) => return Err(e),
        }
    }
    Ok(out)
}

pub async fn translate_ollama(
    host: &str,
    model: &str,
    lines: &[SubtitleLine],
    source: &str,
    target: &str,
) -> Result<Vec<String>, TranslateError> {
    translate_resilient(lines, |chunk| {
        translate_ollama_batch(host, model, chunk, source, target)
    })
    .await
}

pub async fn translate_openai(
    api_key: &str,
    model: &str,
    lines: &[SubtitleLine],
    source: &str,
    target: &str,
) -> Result<Vec<String>, TranslateError> {
    translate_openai_with_base(OPENAI_BASE, api_key, model, lines, source, target).await
}

pub(crate) async fn translate_openai_with_base(
    base_url: &str,
    api_key: &str,
    model: &str,
    lines: &[SubtitleLine],
    source: &str,
    target: &str,
) -> Result<Vec<String>, TranslateError> {
    translate_resilient(lines, |chunk| {
        translate_openai_batch(base_url, api_key, model, chunk, source, target)
    })
    .await
}

/// Error text for a failed provider call. Always names the HTTP status, since
/// several providers (and proxies) reject requests with an empty body.
fn api_error_message(provider: &str, status: reqwest::StatusCode, body: &str) -> String {
    let body = body.trim();
    if body.is_empty() {
        format!("{provider} error: HTTP {status} (empty response)")
    } else {
        format!("{provider} error: HTTP {status}: {body}")
    }
}

/// Request body for Ollama's OpenAI-compatible chat endpoint.
///
/// Reasoning is switched off: "thinking" models (e.g. gemma4) otherwise emit
/// thousands of hidden reasoning tokens per batch, which on a single-slot local
/// server pushes a 25-line batch past the client timeout with no progress.
fn ollama_chat_body(model: &str, prompt: &str) -> serde_json::Value {
    serde_json::json!({
        "model": model,
        "messages": [{"role": "user", "content": prompt}],
        "temperature": 0.3,
        "reasoning_effort": "none",
    })
}

async fn translate_ollama_batch(
    host: &str,
    model: &str,
    lines: &[SubtitleLine],
    source: &str,
    target: &str,
) -> Result<Vec<String>, TranslateError> {
    let target = lang_label(target);
    let source = lang_label(source);
    // Local model inference on a 25-line batch routinely exceeds 30s;
    // extend timeout to 5 minutes for more reliable inference.
    let client = client(300);
    let url = format!("{}/v1/chat/completions", host.trim_end_matches('/'));

    let texts = lines_as_json(lines);
    let n = lines.len();
    let prompt = format!(
        "Translate the following subtitle lines from {} to {}. \
Preserve the meaning, tone, and approximate length. \
Input is a JSON array of {n} strings, one subtitle cue each. Return ONLY a JSON array of exactly {n} translated strings in the same order: never merge, split, drop or add entries, and keep each translation on a single line. \
Do not add explanations or markdown formatting.\n\nLines:\n{texts}\n\nReturn format: [\"translation1\", \"translation2\", ...]",
        source,
        target,
    );

    let body = ollama_chat_body(model, &prompt);

    let resp = send_checked(client.post(&url).json(&body)).await?;
    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(TranslateError::Api(api_error_message(
            "Ollama", status, &text,
        )));
    }

    let body_text = resp.text().await.map_err(TranslateError::Http)?;
    if body_text.trim().is_empty() {
        return Err(TranslateError::Api(format!(
            "Ollama returned an empty response from model '{}'",
            model
        )));
    }

    let json_resp: serde_json::Value = serde_json::from_str(&body_text).map_err(|e| {
        TranslateError::Api(format!(
            "Ollama returned invalid JSON from model '{}': {}",
            model, e
        ))
    })?;

    let content = json_resp["choices"][0]["message"]["content"]
        .as_str()
        .ok_or_else(|| TranslateError::Api("Missing response content".to_string()))?;

    parse_ollama_translations(content, lines.len()).map_err(|err| {
        TranslateError::Api(format!(
            "Ollama returned an unparseable response from model '{}': {}",
            model, err
        ))
    })
}

// ─── LibreTranslate ───

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibreLanguage {
    pub code: String,
    pub name: String,
}

pub async fn detect_libretranslate(host: &str) -> Result<Vec<LibreLanguage>, TranslateError> {
    let client = client(10);
    let base = host.trim_end_matches('/');
    for path in &["/languages", "/api/languages"] {
        if let Ok(resp) = client.get(format!("{}{}", base, path)).send().await {
            if resp.status().is_success() {
                if let Ok(data) = resp.json::<Vec<LibreLanguage>>().await {
                    return Ok(data);
                }
            }
        }
    }
    Err(TranslateError::Api(
        "LibreTranslate not reachable on /languages or /api/languages".to_string(),
    ))
}

pub async fn translate_libretranslate(
    host: &str,
    lines: &[SubtitleLine],
    source: &str,
    target: &str,
) -> Result<Vec<String>, TranslateError> {
    let client = client(120);
    let url = format!("{}/translate", host.trim_end_matches('/'));

    // Send all lines in one request using numbered markers to preserve line identity
    let joined = lines
        .iter()
        .enumerate()
        .map(|(i, l)| format!("[{}]{}", i, l.text))
        .collect::<Vec<_>>()
        .join("\n");

    let body = serde_json::json!({
        "q": joined,
        "source": source,
        "target": target,
        "format": "text",
    });

    let resp = client.post(&url).json(&body).send().await?;
    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(TranslateError::Api(api_error_message(
            "LibreTranslate",
            status,
            &text,
        )));
    }

    let json_resp: serde_json::Value = resp.json().await?;
    let block = json_resp["translatedText"]
        .as_str()
        .ok_or_else(|| TranslateError::Api("Missing translatedText".to_string()))?;

    let mut results = Vec::with_capacity(lines.len());
    for (i, line) in lines.iter().enumerate() {
        let marker = format!("[{}]", i);
        let next_marker = format!("[{}]", i + 1);
        if let Some(start) = block.find(&marker) {
            let from = start + marker.len();
            let to = block[from..]
                .find(&next_marker)
                .map(|e| from + e)
                .unwrap_or(block.len());
            results.push(block[from..to].trim().to_string());
        } else {
            // Fallback: keep original text if marker not found
            results.push(line.text.clone());
        }
    }

    Ok(results)
}

// ─── DeepL ───

/// Languages DeepL's v2 API actually supports, out of the codes SubForge's
/// UI offers. Hindi (hi), Persian (fa), and Urdu (ur) are NOT supported by
/// DeepL and produce a 400 "source_lang not supported" if sent.
const DEEPL_LANGS: &[&str] = &[
    "en", "ar", "es", "fr", "de", "ja", "ko", "zh", "ru", "pt", "it", "tr", "nl", "pl", "sv", "id",
];

pub async fn translate_deepl(
    api_key: &str,
    lines: &[SubtitleLine],
    source: &str,
    target: &str,
) -> Result<Vec<String>, TranslateError> {
    translate_deepl_with_base(deepl_base(api_key), api_key, lines, source, target).await
}

pub(crate) async fn translate_deepl_with_base(
    base_url: &str,
    api_key: &str,
    lines: &[SubtitleLine],
    source: &str,
    target: &str,
) -> Result<Vec<String>, TranslateError> {
    // Validate that source and target languages are supported by DeepL
    for code in &[source, target] {
        if !DEEPL_LANGS.contains(&code.to_lowercase().as_str()) {
            return Err(TranslateError::Api(format!(
                "deepl_unsupported_lang:{}",
                code
            )));
        }
    }

    let url = format!("{}/v2/translate", base_url.trim_end_matches('/'));
    let texts: Vec<&str> = lines.iter().map(|l| l.text.as_str()).collect();

    let body = serde_json::json!({
        "text": texts,
        "source_lang": source.to_uppercase(),
        "target_lang": target.to_uppercase(),
    });

    let resp = client(30)
        .post(url)
        .header("Authorization", format!("DeepL-Auth-Key {}", api_key))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(TranslateError::Api(api_error_message(
            "DeepL", status, &text,
        )));
    }

    let json_resp: serde_json::Value = resp.json().await?;
    let translations: Vec<String> = json_resp["translations"]
        .as_array()
        .ok_or_else(|| TranslateError::Api("Missing translations array".to_string()))?
        .iter()
        .filter_map(|t| t["text"].as_str().map(|s| s.to_string()))
        .collect();

    Ok(translations)
}

// ─── OpenAI ───

async fn translate_openai_batch(
    base_url: &str,
    api_key: &str,
    model: &str,
    lines: &[SubtitleLine],
    source: &str,
    target: &str,
) -> Result<Vec<String>, TranslateError> {
    let target = lang_label(target);
    let source = lang_label(source);
    let url = format!("{}/v1/chat/completions", base_url.trim_end_matches('/'));

    let texts = lines_as_json(lines);
    let n = lines.len();
    let prompt = format!(
        "Translate the following subtitle lines from {} to {}. \
Preserve meaning, tone, and approximate length. \
Input is a JSON array of {n} strings, one subtitle cue each. Return ONLY a JSON array of exactly {n} translated strings in the same order: never merge, split, drop or add entries. \
No markdown, no explanations.\n\n{texts}\n\nReturn: [\"...\", \"...\"]",
        source,
        target,
    );

    let body = serde_json::json!({
        "model": model,
        "messages": [{"role": "user", "content": prompt}],
        "temperature": 0.3,
    });

    let resp = client(30)
        .post(url)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(TranslateError::Api(api_error_message(
            "OpenAI", status, &text,
        )));
    }

    let json_resp: serde_json::Value = resp.json().await?;
    let content = json_resp["choices"][0]["message"]["content"]
        .as_str()
        .ok_or_else(|| TranslateError::Api("Missing response content".to_string()))?;

    let cleaned = content
        .trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();

    let translations: Vec<String> = serde_json::from_str(cleaned)?;
    Ok(translations)
}

// ─── Google Translate ───

pub async fn translate_google(
    api_key: &str,
    lines: &[SubtitleLine],
    source: &str,
    target: &str,
) -> Result<Vec<String>, TranslateError> {
    translate_google_with_base(GOOGLE_BASE, api_key, lines, source, target).await
}

pub(crate) async fn translate_google_with_base(
    base_url: &str,
    api_key: &str,
    lines: &[SubtitleLine],
    source: &str,
    target: &str,
) -> Result<Vec<String>, TranslateError> {
    let url = format!(
        "{}/language/translate/v2?key={api_key}",
        base_url.trim_end_matches('/')
    );

    let texts: Vec<&str> = lines.iter().map(|l| l.text.as_str()).collect();
    let body = serde_json::json!({
        "q": texts,
        "source": source,
        "target": target,
        "format": "text",
    });

    let resp = client(30).post(&url).json(&body).send().await?;
    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(TranslateError::Api(api_error_message(
            "Google Translate",
            status,
            &text,
        )));
    }

    let json_resp: serde_json::Value = resp.json().await?;
    let translations: Vec<String> = json_resp["data"]["translations"]
        .as_array()
        .ok_or_else(|| TranslateError::Api("Missing translations array".to_string()))?
        .iter()
        .filter_map(|t| t["translatedText"].as_str().map(|s| s.to_string()))
        .collect();

    Ok(translations)
}

// ─── Settings "Test" button validation calls (lightest official endpoint per provider) ───

pub async fn test_deepl_key(api_key: &str) -> Result<(), TranslateError> {
    test_deepl_key_with_base(deepl_base(api_key), api_key).await
}

pub(crate) async fn test_deepl_key_with_base(
    base_url: &str,
    api_key: &str,
) -> Result<(), TranslateError> {
    let url = format!("{}/v2/usage", base_url.trim_end_matches('/'));
    let resp = send_checked(
        client(10)
            .get(url)
            .header("Authorization", format!("DeepL-Auth-Key {api_key}")),
    )
    .await?;
    if resp.status().is_success() {
        Ok(())
    } else {
        Err(TranslateError::Api(format!(
            "DeepL returned {}",
            resp.status()
        )))
    }
}

pub async fn test_openai_key(api_key: &str) -> Result<(), TranslateError> {
    test_openai_key_with_base(OPENAI_BASE, api_key).await
}

pub(crate) async fn test_openai_key_with_base(
    base_url: &str,
    api_key: &str,
) -> Result<(), TranslateError> {
    let url = format!("{}/v1/models", base_url.trim_end_matches('/'));
    let resp = send_checked(
        client(10)
            .get(url)
            .header("Authorization", format!("Bearer {api_key}")),
    )
    .await?;
    if resp.status().is_success() {
        Ok(())
    } else {
        Err(TranslateError::Api(format!(
            "OpenAI returned {}",
            resp.status()
        )))
    }
}

pub async fn test_google_key(api_key: &str) -> Result<(), TranslateError> {
    test_google_key_with_base(GOOGLE_BASE, api_key).await
}

pub(crate) async fn test_google_key_with_base(
    base_url: &str,
    api_key: &str,
) -> Result<(), TranslateError> {
    let url = format!(
        "{}/language/translate/v2/languages?key={api_key}",
        base_url.trim_end_matches('/')
    );
    let resp = send_checked(client(10).get(&url)).await?;
    if resp.status().is_success() {
        Ok(())
    } else {
        Err(TranslateError::Api(format!(
            "Google Translate returned {}",
            resp.status()
        )))
    }
}

#[cfg(test)]
mod ollama_parse_tests {
    use super::parse_ollama_translations;

    #[test]
    fn test_plain_json_array() {
        let result = parse_ollama_translations(r#"["Hola", "Mundo"]"#, 2).unwrap();
        assert_eq!(result, vec!["Hola", "Mundo"]);
    }

    #[test]
    fn test_fenced_json_array() {
        let content = "```json\n[\"Hola\", \"Mundo\"]\n```";
        let result = parse_ollama_translations(content, 2).unwrap();
        assert_eq!(result, vec!["Hola", "Mundo"]);
    }

    #[test]
    fn test_array_embedded_in_prose() {
        let content = "Here are the translations:\n[\"Hola\", \"Mundo\"]\nHope this helps!";
        let result = parse_ollama_translations(content, 2).unwrap();
        assert_eq!(result, vec!["Hola", "Mundo"]);
    }

    #[test]
    fn test_plain_lines_fallback() {
        let result = parse_ollama_translations("Hola\nMundo", 2).unwrap();
        assert_eq!(result, vec!["Hola", "Mundo"]);
    }

    #[test]
    fn test_empty_content() {
        let result = parse_ollama_translations("", 2);
        assert!(result.unwrap_err().contains("empty response"));
    }

    #[test]
    fn test_long_multibyte_content_does_not_panic() {
        // Arabic model output longer than the 120-char snippet cap — the
        // error snippet must truncate by characters, not bytes.
        let content = "ترجمة غير صالحة ".repeat(20);
        let result = parse_ollama_translations(&content, 2);
        assert!(result.unwrap_err().contains("unparseable response"));
    }
}

#[cfg(test)]
mod resilient_tests {
    use super::{translate_resilient, TranslateError};
    use crate::lines::SubtitleLine;

    fn lines(n: usize) -> Vec<SubtitleLine> {
        (0..n)
            .map(|i| SubtitleLine {
                id: i,
                start: 0,
                end: 0,
                text: format!("line {i}"),
            })
            .collect()
    }

    #[tokio::test]
    async fn splits_batches_the_model_gets_wrong() {
        let input = lines(8);
        // A model that merges cues whenever it sees more than 2 of them at once.
        let out = translate_resilient(&input, |chunk| {
            let texts: Vec<String> = chunk.iter().map(|l| l.text.to_uppercase()).collect();
            async move {
                if texts.len() > 2 {
                    Ok(vec![texts.join(" ")])
                } else {
                    Ok(texts)
                }
            }
        })
        .await
        .unwrap();
        assert_eq!(out.len(), 8);
        assert_eq!(out[0], "LINE 0");
        assert_eq!(out[7], "LINE 7");
    }

    #[tokio::test]
    async fn connection_errors_are_not_retried() {
        let input = lines(4);
        let res =
            translate_resilient(&input, |_| async { Err(TranslateError::ConnectFailed) }).await;
        assert!(matches!(res, Err(TranslateError::ConnectFailed)));
    }
}

#[cfg(test)]
mod ollama_body_tests {
    use super::{api_error_message, ollama_chat_body};

    #[test]
    fn api_error_includes_status_even_when_body_is_empty() {
        let m = api_error_message("DeepL", reqwest::StatusCode::FORBIDDEN, "");
        assert!(m.contains("403") && m.contains("DeepL"), "{m}");
    }

    #[test]
    fn disables_reasoning_so_batches_do_not_stall() {
        let b = ollama_chat_body("gemma4:12b", "hi");
        assert_eq!(b["reasoning_effort"], "none");
        assert_eq!(b["model"], "gemma4:12b");
        assert_eq!(b["messages"][0]["content"], "hi");
    }
}

#[cfg(test)]
mod http_tests {
    use super::*;
    use axum::{
        http::StatusCode,
        routing::{get, post},
        Json, Router,
    };
    use std::sync::{Arc, Mutex};

    async fn serve(app: Router) -> String {
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = l.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
        format!("http://{addr}")
    }

    fn lines() -> Vec<SubtitleLine> {
        vec![SubtitleLine {
            id: 0,
            start: 0,
            end: 1000,
            text: "Hello".into(),
        }]
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn openai_prompt_names_source_language() {
        let seen: Arc<Mutex<String>> = Arc::default();
        let s = seen.clone();
        let app = Router::new().route(
            "/v1/chat/completions",
            post(move |Json(b): Json<serde_json::Value>| {
                let s = s.clone();
                async move {
                    *s.lock().unwrap() = b["messages"][0]["content"].as_str().unwrap().to_string();
                    Json(serde_json::json!({"choices":[{"message":{"content":"[\"Hola\"]"}}]}))
                }
            }),
        );
        let base = serve(app).await;
        let out = translate_openai_with_base(&base, "k", "m", &lines(), "en", "es")
            .await
            .unwrap();
        assert_eq!(out, vec!["Hola"]);
        let p = seen.lock().unwrap().clone();
        assert!(p.contains("from English to Spanish"), "{p}");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn ollama_prompt_names_source_and_disables_reasoning() {
        let seen: Arc<Mutex<serde_json::Value>> = Arc::default();
        let s = seen.clone();
        let app = Router::new().route(
            "/v1/chat/completions",
            post(move |Json(b): Json<serde_json::Value>| {
                let s = s.clone();
                async move {
                    *s.lock().unwrap() = b;
                    Json(serde_json::json!({"choices":[{"message":{"content":"[\"Hola\"]"}}]}))
                }
            }),
        );
        let base = serve(app).await;
        let out = translate_ollama(&base, "m", &lines(), "en", "es")
            .await
            .unwrap();
        assert_eq!(out, vec!["Hola"]);
        let b = seen.lock().unwrap().clone();
        assert_eq!(b["reasoning_effort"], "none");
        assert!(b["messages"][0]["content"]
            .as_str()
            .unwrap()
            .contains("from English to Spanish"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn libretranslate_falls_back_to_api_languages() {
        let app = Router::new().route(
            "/api/languages",
            get(|| async { Json(serde_json::json!([{"code":"en","name":"English"}])) }),
        );
        let base = serve(app).await;
        let langs = detect_libretranslate(&base).await.unwrap();
        assert_eq!(langs.len(), 1);
        assert_eq!(langs[0].code, "en");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn libretranslate_batches_with_numbered_markers() {
        let app = Router::new().route(
            "/translate",
            post(|Json(b): Json<serde_json::Value>| async move {
                let q = b["q"].as_str().unwrap();
                assert_eq!(q, "[0]Hello\n[1]World");
                Json(serde_json::json!({"translatedText": "[0]Hola\n[1]Mundo"}))
            }),
        );
        let base = serve(app).await;
        let two = vec![
            SubtitleLine {
                id: 0,
                start: 0,
                end: 1,
                text: "Hello".into(),
            },
            SubtitleLine {
                id: 1,
                start: 1,
                end: 2,
                text: "World".into(),
            },
        ];
        let out = translate_libretranslate(&base, &two, "en", "es")
            .await
            .unwrap();
        assert_eq!(out, vec!["Hola", "Mundo"]);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn deepl_error_includes_http_status() {
        let app = Router::new().route("/v2/translate", post(|| async { StatusCode::FORBIDDEN }));
        let base = serve(app).await;
        let err = translate_deepl_with_base(&base, "key", &lines(), "en", "es")
            .await
            .unwrap_err();
        let m = err.to_string();
        assert!(m.contains("403") && m.contains("DeepL"), "{m}");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn deepl_rejects_unsupported_language_without_network() {
        let err = translate_deepl_with_base("http://127.0.0.1:1", "key", &lines(), "en", "fa")
            .await
            .unwrap_err();
        assert!(err.to_string().contains("deepl_unsupported_lang:fa"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn google_translates_and_key_test_works() {
        let app = Router::new()
            .route(
                "/language/translate/v2",
                post(|| async {
                    Json(serde_json::json!({"data":{"translations":[{"translatedText":"Hola"}]}}))
                }),
            )
            .route(
                "/language/translate/v2/languages",
                get(|| async { Json(serde_json::json!({})) }),
            );
        let base = serve(app).await;
        let out = translate_google_with_base(&base, "k", &lines(), "en", "es")
            .await
            .unwrap();
        assert_eq!(out, vec!["Hola"]);
        test_google_key_with_base(&base, "k").await.unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn key_tests_report_failure_status() {
        let app = Router::new()
            .route("/v2/usage", get(|| async { StatusCode::FORBIDDEN }))
            .route("/v1/models", get(|| async { StatusCode::UNAUTHORIZED }));
        let base = serve(app).await;
        assert!(test_deepl_key_with_base(&base, "k")
            .await
            .unwrap_err()
            .to_string()
            .contains("403"));
        assert!(test_openai_key_with_base(&base, "k")
            .await
            .unwrap_err()
            .to_string()
            .contains("401"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn refused_connection_maps_to_connect_failed() {
        // Port 1 on loopback: nothing listens, so the connect is refused.
        let err = detect_ollama("http://127.0.0.1:1").await.unwrap_err();
        assert!(matches!(err, TranslateError::ConnectFailed), "{err:?}");
    }
}
