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

    let output = Command::new(&python)
        .args([&script, "extract", input_path])
        .output()?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<SubtitleLine> = serde_json::from_str(&stdout)?;
    Ok(lines)
}

pub fn rebuild_lines(
    original_path: &str,
    translations_json: &str,
    output_path: &str,
) -> Result<(), TranslateError> {
    let python = get_python()?;
    let script = super::script_path("subtitle_lines.py");

    let output = Command::new(&python)
        .args([&script, "rebuild", original_path, translations_json, output_path])
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

pub async fn detect_ollama(host: &str) -> Result<OllamaTagResponse, TranslateError> {
    let client = reqwest::Client::new();
    let url = format!("{}/api/tags", host.trim_end_matches('/'));
    let resp = client.get(&url).send().await?;
    if !resp.status().is_success() {
        return Err(TranslateError::Api(format!("Ollama returned {}", resp.status())));
    }
    let data: OllamaTagResponse = resp.json().await?;
    Ok(data)
}

pub async fn translate_ollama(
    host: &str,
    model: &str,
    lines: &[SubtitleLine],
    _source: &str,
    target: &str,
) -> Result<Vec<String>, TranslateError> {
    let client = reqwest::Client::new();
    let url = format!("{}/v1/chat/completions", host.trim_end_matches('/'));

    let texts: Vec<String> = lines.iter().map(|l| l.text.clone()).collect();
    let prompt = format!(
        "Translate the following subtitle lines to {}. \
Preserve the meaning, tone, and approximate length. \
Return ONLY a JSON array of translated strings, one per line, in the same order. \
Do not add explanations or markdown formatting.\n\nLines:\n{}\n\nReturn format: [\"translation1\", \"translation2\", ...]",
        target,
        texts.join("\n")
    );

    let body = serde_json::json!({
        "model": model,
        "messages": [{"role": "user", "content": prompt}],
        "temperature": 0.3,
    });

    let resp = client.post(&url).json(&body).send().await?;
    if !resp.status().is_success() {
        let text = resp.text().await.unwrap_or_default();
        return Err(TranslateError::Api(format!("Ollama API error: {}", text)));
    }

    let json_resp: serde_json::Value = resp.json().await?;
    let content = json_resp["choices"][0]["message"]["content"]
        .as_str()
        .ok_or_else(|| TranslateError::Api("Missing response content".to_string()))?;

    // Try to extract JSON array from the response (models sometimes wrap it in markdown)
    let cleaned = content
        .trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();

    let translations: Vec<String> = serde_json::from_str(cleaned)?;
    Ok(translations)
}

// ─── LibreTranslate ───

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibreLanguage {
    pub code: String,
    pub name: String,
}

pub async fn detect_libretranslate(host: &str) -> Result<Vec<LibreLanguage>, TranslateError> {
    let client = reqwest::Client::new();
    let url = format!("{}/languages", host.trim_end_matches('/'));
    let resp = client.get(&url).send().await?;
    if !resp.status().is_success() {
        return Err(TranslateError::Api(format!("LibreTranslate returned {}", resp.status())));
    }
    let data: Vec<LibreLanguage> = resp.json().await?;
    Ok(data)
}

pub async fn translate_libretranslate(
    host: &str,
    lines: &[SubtitleLine],
    source: &str,
    target: &str,
) -> Result<Vec<String>, TranslateError> {
    let client = reqwest::Client::new();
    let url = format!("{}/translate", host.trim_end_matches('/'));

    let mut results = Vec::new();
    for line in lines {
        let body = serde_json::json!({
            "q": line.text,
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
        let translated = json_resp["translatedText"]
            .as_str()
            .ok_or_else(|| TranslateError::Api("Missing translatedText".to_string()))?;
        results.push(translated.to_string());
    }

    Ok(results)
}

// ─── DeepL ───

pub async fn translate_deepl(
    api_key: &str,
    lines: &[SubtitleLine],
    source: &str,
    target: &str,
) -> Result<Vec<String>, TranslateError> {
    let client = reqwest::Client::new();
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

pub async fn translate_openai(
    api_key: &str,
    model: &str,
    lines: &[SubtitleLine],
    _source: &str,
    target: &str,
) -> Result<Vec<String>, TranslateError> {
    let client = reqwest::Client::new();
    let url = "https://api.openai.com/v1/chat/completions";

    let texts: Vec<String> = lines.iter().map(|l| l.text.clone()).collect();
    let prompt = format!(
        "Translate the following subtitle lines to {}. \
Preserve meaning, tone, and approximate length. \
Return ONLY a JSON array of translated strings in the same order. \
No markdown, no explanations.\n\n{}\n\nReturn: [\"...\", \"...\"]",
        target,
        texts.join("\n")
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
