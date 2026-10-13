//! Claude through the Anthropic Messages API with your own key. Rust has no
//! official Anthropic SDK, so this is a plain HTTPS call.

use super::error_text;
use serde_json::{json, Value};

const API_URL: &str = "https://api.anthropic.com/v1/messages";
const API_VERSION: &str = "2023-06-01";
/// Lets the API retry a refused request on another model, routed by refusal type.
const FALLBACK_BETA: &str = "server-side-fallback-2026-07-01";

pub async fn complete(client: &reqwest::Client, api_key: &str, model: &str, system: &str, user: &str) -> Result<String, String> {
  if api_key.trim().is_empty() {
    return Err("no Claude API key is set".into());
  }
  let model = model.trim();
  let mut body = json!({
    "model": model,
    "max_tokens": 16000,
    "system": system,
    "messages": [{ "role": "user", "content": user }],
  });
  if supports_effort(model) {
    // Planning a few image prompts doesn't need deep thinking.
    body["output_config"] = json!({ "effort": "low" });
  }
  let mut request = client
    .post(API_URL)
    .header("x-api-key", api_key.trim())
    .header("anthropic-version", API_VERSION);
  if supports_default_fallbacks(model) {
    body["fallbacks"] = json!("default");
    request = request.header("anthropic-beta", FALLBACK_BETA);
  }

  let resp = request.json(&body).send().await.map_err(|e| format!("couldn't reach the Anthropic API: {e}"))?;
  let status = resp.status();
  let text = resp.text().await.map_err(|e| e.to_string())?;
  if !status.is_success() {
    return Err(error_text(status, &text));
  }
  let value: Value = serde_json::from_str(&text).map_err(|_| "the Anthropic API sent a reply Easel couldn't read".to_string())?;
  parse_reply(&value)
}

fn parse_reply(value: &Value) -> Result<String, String> {
  if value.get("stop_reason").and_then(Value::as_str) == Some("refusal") {
    let why = value.pointer("/stop_details/explanation").and_then(Value::as_str).unwrap_or("no reason given");
    return Err(format!("Claude declined this request ({why})"));
  }
  let text: String = value
    .get("content")
    .and_then(Value::as_array)
    .into_iter()
    .flatten()
    .filter(|block| block.get("type").and_then(Value::as_str) == Some("text"))
    .filter_map(|block| block.get("text").and_then(Value::as_str))
    .collect::<Vec<_>>()
    .join("");
  if text.trim().is_empty() {
    return Err("Claude returned an empty answer".into());
  }
  Ok(text)
}

/// The `effort` setting exists on the current model generation only.
fn supports_effort(model: &str) -> bool {
  ["claude-fable-5", "claude-opus-5", "claude-sonnet-5", "claude-haiku-5", "claude-opus-4-6", "claude-opus-4-7", "claude-opus-4-8", "claude-sonnet-4-6"]
    .iter()
    .any(|prefix| model.starts_with(prefix))
}

/// `fallbacks: "default"` is accepted on these models only; others reject it.
fn supports_default_fallbacks(model: &str) -> bool {
  ["claude-fable-5-1", "claude-opus-5-5", "claude-opus-5", "claude-sonnet-5-5"].contains(&model)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn joins_text_blocks_and_skips_thinking() {
    let reply = json!({ "stop_reason": "end_turn", "content": [
      { "type": "thinking", "thinking": "" },
      { "type": "text", "text": "{\"reply\":" },
      { "type": "text", "text": "\"ok\"}" }
    ]});
    assert_eq!(parse_reply(&reply).unwrap(), "{\"reply\":\"ok\"}");
  }

  #[test]
  fn reports_refusals() {
    let reply = json!({ "stop_reason": "refusal", "stop_details": { "explanation": "policy" }, "content": [] });
    assert!(parse_reply(&reply).unwrap_err().contains("policy"));
  }

  #[test]
  fn model_feature_gates() {
    assert!(supports_effort("claude-opus-5-5"));
    assert!(supports_effort("claude-haiku-5-5"));
    assert!(!supports_effort("claude-haiku-4-5"));
    assert!(supports_default_fallbacks("claude-opus-5-5"));
    assert!(!supports_default_fallbacks("claude-haiku-5-5"));
  }
}
