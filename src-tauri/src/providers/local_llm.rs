//! Any OpenAI-compatible chat server: LM Studio (http://127.0.0.1:1234) or
//! Ollama (http://127.0.0.1:11434) on this PC, or a remote one such as
//! llama.cpp's llama-server (with or without --rpc), vLLM, or a hosted API.

use super::error_text;
use serde_json::{json, Value};

pub async fn complete(client: &reqwest::Client, base_url: &str, api_key: &str, model: &str, system: &str, user: &str) -> Result<String, String> {
  let base = base_url.trim().trim_end_matches('/');
  if base.is_empty() {
    return Err("no server address is set".into());
  }
  let model = if model.trim().is_empty() { first_model(client, base, api_key).await? } else { model.trim().to_string() };
  let body = json!({
    "model": model,
    "temperature": 0.7,
    "messages": [
      { "role": "system", "content": system },
      { "role": "user", "content": user },
    ],
  });
  let resp = with_key(client.post(endpoint(base, "chat/completions")), api_key)
    .json(&body)
    .send()
    .await
    .map_err(|e| format!("couldn't reach the model server at {base} (is it running?): {e}"))?;
  let status = resp.status();
  let text = resp.text().await.map_err(|e| e.to_string())?;
  if !status.is_success() {
    return Err(error_text(status, &text));
  }
  let value: Value = serde_json::from_str(&text).map_err(|_| "the local model sent a reply Easel couldn't read".to_string())?;
  let answer = value
    .pointer("/choices/0/message/content")
    .and_then(Value::as_str)
    .unwrap_or_default()
    .to_string();
  if answer.trim().is_empty() {
    return Err("the local model returned an empty answer".into());
  }
  Ok(strip_think_tags(&answer))
}

pub async fn list_models(client: &reqwest::Client, base_url: &str, api_key: &str) -> Result<Vec<String>, String> {
  let base = base_url.trim().trim_end_matches('/');
  let resp = with_key(client.get(endpoint(base, "models")), api_key)
    .send()
    .await
    .map_err(|e| format!("Couldn't reach {base}. Is the server running? ({e})"))?;
  let status = resp.status();
  let text = resp.text().await.map_err(|e| e.to_string())?;
  if !status.is_success() {
    return Err(error_text(status, &text));
  }
  let value: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
  Ok(model_ids(&value))
}

/// Servers are usually given as http://host:port, but some hosted APIs give a
/// base that already ends in /v1 (or /openai), so only add /v1 when it's missing.
fn endpoint(base: &str, path: &str) -> String {
  let has_version = base.ends_with("/v1") || base.ends_with("/openai") || base.ends_with("/v1beta/openai");
  if has_version { format!("{base}/{path}") } else { format!("{base}/v1/{path}") }
}

fn with_key(request: reqwest::RequestBuilder, api_key: &str) -> reqwest::RequestBuilder {
  if api_key.trim().is_empty() { request } else { request.bearer_auth(api_key.trim()) }
}

async fn first_model(client: &reqwest::Client, base: &str, api_key: &str) -> Result<String, String> {
  list_models(client, base, api_key)
    .await?
    .into_iter()
    .next()
    .ok_or_else(|| "the local server has no model loaded".to_string())
}

fn model_ids(value: &Value) -> Vec<String> {
  value
    .get("data")
    .and_then(Value::as_array)
    .into_iter()
    .flatten()
    .filter_map(|m| m.get("id").and_then(Value::as_str).map(str::to_string))
    .collect()
}

/// Reasoning models (Qwen3, DeepSeek-R1) wrap their thinking in <think> tags.
fn strip_think_tags(text: &str) -> String {
  match (text.find("<think>"), text.rfind("</think>")) {
    (Some(start), Some(end)) if end > start => format!("{}{}", &text[..start], &text[end + "</think>".len()..]).trim().to_string(),
    _ => text.trim().to_string(),
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn removes_reasoning_blocks() {
    assert_eq!(strip_think_tags("<think>hmm</think>\n{\"reply\":\"hi\"}"), "{\"reply\":\"hi\"}");
    assert_eq!(strip_think_tags("plain"), "plain");
  }

  #[test]
  fn adds_v1_only_when_the_address_lacks_it() {
    assert_eq!(endpoint("http://10.0.0.5:8080", "models"), "http://10.0.0.5:8080/v1/models");
    assert_eq!(endpoint("https://api.example.com/v1", "chat/completions"), "https://api.example.com/v1/chat/completions");
    assert_eq!(endpoint("https://generativelanguage.googleapis.com/v1beta/openai", "models"), "https://generativelanguage.googleapis.com/v1beta/openai/models");
  }

  #[test]
  fn reads_model_ids() {
    let v = json!({ "data": [{ "id": "qwen3-30b-a3b" }, { "id": "gpt-oss-20b" }] });
    assert_eq!(model_ids(&v), vec!["qwen3-30b-a3b", "gpt-oss-20b"]);
  }
}
