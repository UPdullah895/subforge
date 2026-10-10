use subforge_deps::HideWindow;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::process::Command;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum TranslateError {
    #[error("Python not found")]
    PythonNotFound,
    #[error("Subtitle script failed: {0}")]
    ScriptFailed(String),
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
async fn send_checked(request: reqwest::RequestBuilder) -> Result<reqwest::Response, TranslateError> {
    match request.send().await {
        Ok(resp) => Ok(resp),
        Err(e) if e.is_connect() => Err(TranslateError::ConnectFailed),
        Err(e) => Err(TranslateError::Http(e)),
    }
}

fn get_python() -> Result<PathBuf, TranslateError> {
    super::find_python().ok_or(TranslateError::PythonNotFound)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubtitleLine {
    pub id: usize,
    pub start: i32,
    pub end: i32,
    pub text: String,
}

pub fn extract_lines(input_path: &str) -> Result<Vec<SubtitleLine>, TranslateError> {
    let python = get_python()?;
    let script = super::script_path("subtitle_lines.py");

    let output = Command::new(&python).hide_window()
        .args([&script, "extract", input_path])
        .output()?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<SubtitleLine> = serde_json::from_str(&stdout)?;
    Ok(lines)
}

/// Builds the `rebuild` argument list, adding `--rtl-fix` when the saved
/// setting asks for right-to-left punctuation fixing (on by default).
fn rebuild_args(script: &str, original: &str, translations: &str, output: &str) -> Vec<String> {
    let mut args = vec![
        script.to_string(),
        "rebuild".to_string(),
        original.to_string(),
        translations.to_string(),
        output.to_string(),
    ];
    if crate::config::AppConfig::load().fix_rtl_punctuation {
        args.push("--rtl-fix".to_string());
    }
    args
}

pub fn rebuild_lines(
    original_path: &str,
    translations_json: &str,
    output_path: &str,
) -> Result<(), TranslateError> {
    let python = get_python()?;
    let script = super::script_path("subtitle_lines.py");

    let output = Command::new(&python).hide_window()
        .args(rebuild_args(&script, original_path, translations_json, output_path))
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        return Err(TranslateError::ScriptFailed(stderr));
    }

    Ok(())
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
    let ellipsis = if trimmed.chars().count() > 120 { "..." } else { "" };
    Err(format!("unparseable response: {}{}", snippet, ellipsis))
}

pub async fn detect_ollama(host: &str) -> Result<OllamaTagResponse, TranslateError> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .unwrap_or_default();
    let url = format!("{}/api/tags", host.trim_end_matches('/'));
    let resp = send_checked(client.get(&url)).await?;
    if !resp.status().is_success() {
        return Err(TranslateError::Api(format!("Ollama returned {}", resp.status())));
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
    let mut queue: std::collections::VecDeque<&'a [SubtitleLine]> = std::collections::VecDeque::new();
    queue.push_back(lines);
    while let Some(chunk) = queue.pop_front() {
        if chunk.is_empty() {
            continue;
        }
        match call(chunk).await {
            Ok(t) if t.len() == chunk.len() => out.extend(t),
            Ok(t) if chunk.len() == 1 && !t.is_empty() => out.push(t.join(" ")),
            Ok(_) | Err(TranslateError::Api(_)) | Err(TranslateError::Json(_)) if chunk.len() > 1 => {
                let (a, b) = chunk.split_at(chunk.len() / 2);
                queue.push_front(b);
                queue.push_front(a);
            }
            Ok(_) => return Err(TranslateError::Api("model returned no translation for a cue".to_string())),
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
    translate_resilient(lines, |chunk| translate_ollama_batch(host, model, chunk, source, target)).await
}

pub async fn translate_openai(
    api_key: &str,
    model: &str,
    lines: &[SubtitleLine],
    source: &str,
    target: &str,
) -> Result<Vec<String>, TranslateError> {
    translate_resilient(lines, |chunk| translate_openai_batch(api_key, model, chunk, source, target)).await
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
    let client = reqwest::Client::builder()
        // Local model inference on a 25-line batch routinely exceeds 30s;
        // extend timeout to 5 minutes for more reliable inference.
        .timeout(std::time::Duration::from_secs(300))
        .build()
        .unwrap_or_default();
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
        let text = resp.text().await.unwrap_or_default();
        return Err(TranslateError::Api(format!("Ollama API error: {}", text)));
    }

    let body_text = resp.text().await.map_err(|e| TranslateError::Http(e))?;
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
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .unwrap_or_default();
    let base = host.trim_end_matches('/');
    for path in &["/languages", "/api/languages"] {
        if let Ok(resp) = client.get(&format!("{}{}", base, path)).send().await {
            if resp.status().is_success() {
                if let Ok(data) = resp.json::<Vec<LibreLanguage>>().await {
                    return Ok(data);
                }
            }
        }
    }
    Err(TranslateError::Api(
        "LibreTranslate not reachable on /languages or /api/languages".to_string()
    ))
}

pub async fn translate_libretranslate(
    host: &str,
    lines: &[SubtitleLine],
    source: &str,
    target: &str,
) -> Result<Vec<String>, TranslateError> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .unwrap_or_default();
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
        let text = resp.text().await.unwrap_or_default();
        return Err(TranslateError::Api(format!("LibreTranslate error: {}", text)));
    }

    let json_resp: serde_json::Value = resp.json().await?;
    let block = json_resp["translatedText"]
        .as_str()
        .ok_or_else(|| TranslateError::Api("Missing translatedText".to_string()))?;

    let mut results = Vec::with_capacity(lines.len());
    for i in 0..lines.len() {
        let marker = format!("[{}]", i);
        let next_marker = format!("[{}]", i + 1);
        if let Some(start) = block.find(&marker) {
            let from = start + marker.len();
            let to = block[from..].find(&next_marker).map(|e| from + e).unwrap_or(block.len());
            results.push(block[from..to].trim().to_string());
        } else {
            // Fallback: keep original text if marker not found
            results.push(lines[i].text.clone());
        }
    }

    Ok(results)
}

// ─── DeepL ───

/// Languages DeepL's v2 API actually supports, out of the codes SubForge's
/// UI offers. Hindi (hi), Persian (fa), and Urdu (ur) are NOT supported by
/// DeepL and produce a 400 "source_lang not supported" if sent.
const DEEPL_LANGS: &[&str] = &["en", "ar", "es", "fr", "de", "ja", "ko", "zh", "ru", "pt", "it", "tr", "nl", "pl", "sv", "id"];

pub async fn translate_deepl(
    api_key: &str,
    lines: &[SubtitleLine],
    source: &str,
    target: &str,
) -> Result<Vec<String>, TranslateError> {
    // Validate that source and target languages are supported by DeepL
    for code in &[source, target] {
        if !DEEPL_LANGS.contains(&code.to_lowercase().as_str()) {
            return Err(TranslateError::Api(format!("deepl_unsupported_lang:{}", code)));
        }
    }

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .unwrap_or_default();
    let is_free = api_key.ends_with(":fx");
    let url = if is_free {
        "https://api-free.deepl.com/v2/translate"
    } else {
        "https://api.deepl.com/v2/translate"
    };

    let texts: Vec<String> = lines.iter().map(|l| l.text.clone()).collect();

    let body = serde_json::json!({
        "text": texts,
        "source_lang": source.to_uppercase(),
        "target_lang": target.to_uppercase(),
    });

    let resp = client
        .post(url)
        .header("Authorization", format!("DeepL-Auth-Key {}", api_key))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await?;

    if !resp.status().is_success() {
        let text = resp.text().await.unwrap_or_default();
        return Err(TranslateError::Api(format!("DeepL error: {}", text)));
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
    api_key: &str,
    model: &str,
    lines: &[SubtitleLine],
    source: &str,
    target: &str,
) -> Result<Vec<String>, TranslateError> {
    let target = lang_label(target);
    let source = lang_label(source);
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .unwrap_or_default();
    let url = "https://api.openai.com/v1/chat/completions";

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

    let resp = client
        .post(url)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await?;

    if !resp.status().is_success() {
        let text = resp.text().await.unwrap_or_default();
        return Err(TranslateError::Api(format!("OpenAI error: {}", text)));
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
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .unwrap_or_default();
    let url = format!("https://translation.googleapis.com/language/translate/v2?key={api_key}");

    let texts: Vec<String> = lines.iter().map(|l| l.text.clone()).collect();
    let body = serde_json::json!({
        "q": texts,
        "source": source,
        "target": target,
        "format": "text",
    });

    let resp = client.post(&url).json(&body).send().await?;
    if !resp.status().is_success() {
        let text = resp.text().await.unwrap_or_default();
        return Err(TranslateError::Api(format!("Google Translate error: {}", text)));
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
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .unwrap_or_default();
    let is_free = api_key.ends_with(":fx");
    let url = if is_free {
        "https://api-free.deepl.com/v2/usage"
    } else {
        "https://api.deepl.com/v2/usage"
    };
    let resp = send_checked(
        client.get(url).header("Authorization", format!("DeepL-Auth-Key {api_key}")),
    )
    .await?;
    if resp.status().is_success() {
        Ok(())
    } else {
        Err(TranslateError::Api(format!("DeepL returned {}", resp.status())))
    }
}

pub async fn test_openai_key(api_key: &str) -> Result<(), TranslateError> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .unwrap_or_default();
    let resp = send_checked(
        client
            .get("https://api.openai.com/v1/models")
            .header("Authorization", format!("Bearer {api_key}")),
    )
    .await?;
    if resp.status().is_success() {
        Ok(())
    } else {
        Err(TranslateError::Api(format!("OpenAI returned {}", resp.status())))
    }
}

pub async fn test_google_key(api_key: &str) -> Result<(), TranslateError> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .unwrap_or_default();
    let url = format!(
        "https://translation.googleapis.com/language/translate/v2/languages?key={api_key}"
    );
    let resp = send_checked(client.get(&url)).await?;
    if resp.status().is_success() {
        Ok(())
    } else {
        Err(TranslateError::Api(format!("Google Translate returned {}", resp.status())))
    }
}

#[cfg(test)]
mod ollama_parse_tests {
    use super::parse_ollama_translations;

    #[test]
    fn test_plain_json_array() {
        let content = r#"["Hola", "Mundo"]"#;
        let result = parse_ollama_translations(content, 2);
        assert!(result.is_ok());
        let translations = result.unwrap();
        assert_eq!(translations, vec!["Hola", "Mundo"]);
    }

    #[test]
    fn test_fenced_json_array() {
        let content = r#"```json
["Hola", "Mundo"]
```"#;
        let result = parse_ollama_translations(content, 2);
        assert!(result.is_ok());
        let translations = result.unwrap();
        assert_eq!(translations, vec!["Hola", "Mundo"]);
    }

    #[test]
    fn test_array_embedded_in_prose() {
        let content = r#"Here are the translations:
["Hola", "Mundo"]
Hope this helps!"#;
        let result = parse_ollama_translations(content, 2);
        assert!(result.is_ok());
        let translations = result.unwrap();
        assert_eq!(translations, vec!["Hola", "Mundo"]);
    }

    #[test]
    fn test_plain_lines_fallback() {
        let content = "Hola\nMundo";
        let result = parse_ollama_translations(content, 2);
        assert!(result.is_ok());
        let translations = result.unwrap();
        assert_eq!(translations, vec!["Hola", "Mundo"]);
    }

    #[test]
    fn test_empty_content() {
        let content = "";
        let result = parse_ollama_translations(content, 2);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("empty response"));
    }

    #[test]
    fn test_long_multibyte_content_does_not_panic() {
        // Arabic model output longer than the 120-char snippet cap — the
        // error snippet must truncate by characters, not bytes.
        let content = "ترجمة غير صالحة ".repeat(20);
        let result = parse_ollama_translations(&content, 2);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("unparseable response"));
    }
}

#[cfg(test)]
mod resilient_tests {
    use super::{translate_resilient, SubtitleLine, TranslateError};

    fn lines(n: usize) -> Vec<SubtitleLine> {
        (0..n)
            .map(|i| SubtitleLine { id: i, start: 0, end: 0, text: format!("line {i}") })
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
        let res = translate_resilient(&input, |_| async { Err(TranslateError::ConnectFailed) }).await;
        assert!(matches!(res, Err(TranslateError::ConnectFailed)));
    }
}

#[cfg(test)]
mod ollama_body_tests {
    use super::ollama_chat_body;

    #[test]
    fn disables_reasoning_so_batches_do_not_stall() {
        let b = ollama_chat_body("gemma4:12b", "hi");
        assert_eq!(b["reasoning_effort"], "none");
        assert_eq!(b["model"], "gemma4:12b");
        assert_eq!(b["messages"][0]["content"], "hi");
    }
}
