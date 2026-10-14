//! Any OpenAI-compatible chat server: LM Studio (http://127.0.0.1:1234) or
//! Ollama (http://127.0.0.1:11434) on this PC, or a remote one such as
//! llama.cpp's llama-server (with or without --rpc), vLLM, or a hosted API.

use super::error_text;
use serde_json::{json, Value};
use std::time::Duration;

/// Servers that usually run on this PC: Ollama, LM Studio, llama.cpp's llama-server.
pub const KNOWN_LOCAL_SERVERS: &[&str] = &["http://127.0.0.1:11434", "http://127.0.0.1:1234", "http://127.0.0.1:8080"];
const PROBE_TIMEOUT: Duration = Duration::from_millis(1500);

/// A model server found running on this PC.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalServer {
  pub url: String,
  pub models: Vec<String>,
}

/// The local brain: tries the saved address, and if nothing answers there and
/// it's one of the usual local ports, tries the others. So "LM Studio's port"
/// still works when it's Ollama that's running, and the other way round.
pub async fn complete_local(client: &reqwest::Client, base_url: &str, model: &str, system: &str, user: &str) -> Result<String, String> {
  match complete(client, base_url, "", model, system, user).await {
    Err(err) if err.starts_with("couldn't reach") && is_known_local(base_url) => {
      for other in KNOWN_LOCAL_SERVERS.iter().filter(|u| !same_server(u, base_url)) {
        if let Ok(models) = probe(client, other).await {
          // A model name from the saved server may not exist here; use this server's first model then.
          let pick = if models.iter().any(|m| m == model.trim()) { model.to_string() } else { String::new() };
          return complete(client, other, "", &pick, system, user).await;
        }
      }
      Err(format!("{err}. Also checked {}, and none of them answered.", KNOWN_LOCAL_SERVERS.join(", ")))
    }
    other => other,
  }
}

/// Which of the usual local servers are running right now, and their models.
pub async fn detect_local_servers(client: &reqwest::Client) -> Vec<LocalServer> {
  let mut found = Vec::new();
  for url in KNOWN_LOCAL_SERVERS {
    if let Ok(models) = probe(client, url).await {
      found.push(LocalServer { url: url.to_string(), models });
    }
  }
  found
}

async fn probe(client: &reqwest::Client, base: &str) -> Result<Vec<String>, String> {
  let resp = client.get(endpoint(base, "models")).timeout(PROBE_TIMEOUT).send().await.map_err(|e| e.to_string())?;
  if !resp.status().is_success() {
    return Err(resp.status().to_string());
  }
  let value: Value = resp.json().await.map_err(|e| e.to_string())?;
  Ok(model_ids(&value))
}

fn is_known_local(url: &str) -> bool {
  KNOWN_LOCAL_SERVERS.iter().any(|u| same_server(u, url))
}

/// "localhost:1234", "http://127.0.0.1:1234/" and "http://localhost:1234/v1" are the same server.
fn same_server(a: &str, b: &str) -> bool {
  normalize(a) == normalize(b)
}

fn normalize(url: &str) -> String {
  let u = url.trim().trim_end_matches('/').trim_end_matches("/v1").to_ascii_lowercase();
  let u = u.strip_prefix("http://").or_else(|| u.strip_prefix("https://")).unwrap_or(&u).to_string();
  u.replace("localhost", "127.0.0.1")
}

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
  fn recognises_the_usual_local_ports_however_they_are_written() {
    assert!(is_known_local("http://localhost:1234/"));
    assert!(is_known_local("127.0.0.1:11434"));
    assert!(is_known_local("http://127.0.0.1:8080/v1"));
    assert!(!is_known_local("http://192.168.1.20:11434"));
  }

  #[tokio::test]
  async fn falls_back_to_whichever_local_server_is_running() {
    // Nothing listens on the saved address; the "other" server answers.
    let live = stand_in_server();
    let client = reqwest::Client::new();
    let models = probe(&client, &live).await.unwrap();
    assert_eq!(models, vec!["qwen3:8b"]);
    let answer = complete(&client, &live, "", "", "sys", "hi").await.unwrap();
    assert_eq!(answer, "hello from the stand-in");
  }

  #[test]
  fn reads_model_ids() {
    let v = json!({ "data": [{ "id": "qwen3-30b-a3b" }, { "id": "gpt-oss-20b" }] });
    assert_eq!(model_ids(&v), vec!["qwen3-30b-a3b", "gpt-oss-20b"]);
  }

  /// Serves /v1/models and /v1/chat/completions, like Ollama does.
  fn stand_in_server() -> String {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
      for mut stream in listener.incoming().flatten() {
        let mut buf = [0u8; 16384];
        let n = stream.read(&mut buf).unwrap_or(0);
        let head = String::from_utf8_lossy(&buf[..n]).to_string();
        let body = if head.starts_with("GET") {
          json!({ "data": [{ "id": "qwen3:8b" }] })
        } else {
          json!({ "choices": [{ "message": { "content": "hello from the stand-in" } }] })
        }
        .to_string();
        let _ = write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body);
      }
    });
    format!("http://{addr}")
  }
}
