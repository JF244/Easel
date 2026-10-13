//! A text model running on your own PC through an OpenAI-compatible server:
//! LM Studio (http://127.0.0.1:1234) or Ollama (http://127.0.0.1:11434).

use super::error_text;
use serde_json::{json, Value};

pub async fn complete(client: &reqwest::Client, base_url: &str, model: &str, system: &str, user: &str) -> Result<String, String> {
  let base = base_url.trim().trim_end_matches('/');
  if base.is_empty() {
    return Err("no local model server address is set".into());
  }
  let model = if model.trim().is_empty() { first_model(client, base).await? } else { model.trim().to_string() };
  let body = json!({
    "model": model,
    "temperature": 0.7,
    "messages": [
      { "role": "system", "content": system },
      { "role": "user", "content": user },
    ],
  });
  let resp = client
    .post(format!("{base}/v1/chat/completions"))
    .json(&body)
    .send()
    .await
    .map_err(|e| format!("couldn't reach the local model at {base} (is LM Studio or Ollama running?): {e}"))?;
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

pub async fn list_models(client: &reqwest::Client, base_url: &str) -> Result<Vec<String>, String> {
  let base = base_url.trim().trim_end_matches('/');
  let resp = client
    .get(format!("{base}/v1/models"))
    .send()
    .await
    .map_err(|e| format!("Couldn't reach {base}. Is LM Studio or Ollama running? ({e})"))?;
  let status = resp.status();
  let text = resp.text().await.map_err(|e| e.to_string())?;
  if !status.is_success() {
    return Err(error_text(status, &text));
  }
  let value: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
  Ok(model_ids(&value))
}

async fn first_model(client: &reqwest::Client, base: &str) -> Result<String, String> {
  list_models(client, base)
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
  fn reads_model_ids() {
    let v = json!({ "data": [{ "id": "qwen3-30b-a3b" }, { "id": "gpt-oss-20b" }] });
    assert_eq!(model_ids(&v), vec!["qwen3-30b-a3b", "gpt-oss-20b"]);
  }
}
