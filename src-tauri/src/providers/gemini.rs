//! Google Gemini image models ("Nano Banana") through the Gemini API, billed to
//! your own Google AI Studio key. Gemini has no mask input, so for a painted
//! area we send the mask as a second image and say which part to change.

use super::{decode_image, error_text, extension_for_mime, GenerateRequest, Output};
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use serde_json::{json, Value};

const API_BASE: &str = "https://generativelanguage.googleapis.com/v1beta";
const ASPECT_RATIOS: &[&str] = &["1:1", "2:3", "3:2", "3:4", "4:3", "4:5", "5:4", "9:16", "16:9", "21:9"];

const MASK_INSTRUCTION: &str = "Edit the first image. The second image is a mask: \
change only the area that is white in the mask, blend the edges naturally, and keep \
everything in the black area exactly as it is. The change to make: ";

pub async fn generate(client: &reqwest::Client, api_key: &str, model: &str, req: &GenerateRequest) -> Result<Vec<Output>, String> {
  if api_key.trim().is_empty() {
    return Err("Add your Gemini API key in Settings first (free from Google AI Studio).".into());
  }
  let body = build_body(req)?;
  let url = format!("{API_BASE}/models/{}:generateContent", model.trim());

  let mut outputs = Vec::new();
  // Gemini returns one image per call, so ask once per image wanted.
  for _ in 0..req.count() {
    let resp = client
      .post(&url)
      .header("x-goog-api-key", api_key.trim())
      .json(&body)
      .send()
      .await
      .map_err(|e| format!("Couldn't reach Google's Gemini API: {e}"))?;
    let status = resp.status();
    let text = resp.text().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
      return Err(format!("Gemini said: {}", error_text(status, &text)));
    }
    let value: Value = serde_json::from_str(&text).map_err(|_| "Gemini sent a reply Easel couldn't read.".to_string())?;
    outputs.extend(parse_response(&value)?);
  }
  Ok(outputs)
}

/// Image-capable Gemini models your key can use, for the model picker.
pub async fn list_image_models(client: &reqwest::Client, api_key: &str) -> Result<Vec<String>, String> {
  Ok(image_model_names(&fetch_models(client, api_key).await?))
}

/// Text models your key can use, for the Gemini brain.
pub async fn list_text_models(client: &reqwest::Client, api_key: &str) -> Result<Vec<String>, String> {
  Ok(text_model_names(&fetch_models(client, api_key).await?))
}

/// One answer from a Gemini text model: the Gemini brain.
pub async fn complete(client: &reqwest::Client, api_key: &str, model: &str, system: &str, user: &str) -> Result<String, String> {
  if api_key.trim().is_empty() {
    return Err("no Gemini API key is set".into());
  }
  let model = match model.trim() {
    "" => newest_pro(&list_text_models(client, api_key).await?).ok_or("your Gemini key lists no text models")?,
    m => m.to_string(),
  };
  let body = json!({
    "systemInstruction": { "parts": [{ "text": system }] },
    "contents": [{ "role": "user", "parts": [{ "text": user }] }],
  });
  let resp = client
    .post(format!("{API_BASE}/models/{model}:generateContent"))
    .header("x-goog-api-key", api_key.trim())
    .json(&body)
    .send()
    .await
    .map_err(|e| format!("couldn't reach the Gemini API: {e}"))?;
  let status = resp.status();
  let text = resp.text().await.map_err(|e| e.to_string())?;
  if !status.is_success() {
    return Err(error_text(status, &text));
  }
  let value: Value = serde_json::from_str(&text).map_err(|_| "Gemini sent a reply Easel couldn't read".to_string())?;
  let answer = answer_text(&value);
  if answer.trim().is_empty() {
    let blocked = value.pointer("/promptFeedback/blockReason").and_then(Value::as_str).unwrap_or("no text returned");
    return Err(format!("Gemini gave no answer ({blocked})"));
  }
  Ok(answer)
}

async fn fetch_models(client: &reqwest::Client, api_key: &str) -> Result<Value, String> {
  if api_key.trim().is_empty() {
    return Err("Add your Gemini API key first.".into());
  }
  let resp = client
    .get(format!("{API_BASE}/models?pageSize=1000"))
    .header("x-goog-api-key", api_key.trim())
    .send()
    .await
    .map_err(|e| format!("Couldn't reach Google's Gemini API: {e}"))?;
  let status = resp.status();
  let text = resp.text().await.map_err(|e| e.to_string())?;
  if !status.is_success() {
    return Err(format!("Gemini said: {}", error_text(status, &text)));
  }
  serde_json::from_str(&text).map_err(|e| e.to_string())
}

fn build_body(req: &GenerateRequest) -> Result<Value, String> {
  let mut parts = Vec::new();
  for image in &req.images {
    parts.push(inline_part(image)?);
  }
  let prompt = req.prompt.trim();
  if prompt.is_empty() {
    return Err("Type what you want to see first.".into());
  }
  match &req.mask {
    Some(mask) => {
      if req.images.is_empty() {
        return Err("Pick an image to edit before painting a mask.".into());
      }
      parts.push(inline_part(mask)?);
      parts.push(json!({ "text": format!("{MASK_INSTRUCTION}{prompt}") }));
    }
    None => parts.push(json!({ "text": prompt })),
  }

  let mut generation_config = json!({ "responseModalities": ["TEXT", "IMAGE"] });
  // Edits keep the input's shape; only new images take an aspect ratio.
  if req.images.is_empty() && ASPECT_RATIOS.contains(&req.aspect_ratio.as_str()) {
    generation_config["imageConfig"] = json!({ "aspectRatio": req.aspect_ratio });
  }
  Ok(json!({
    "contents": [{ "role": "user", "parts": parts }],
    "generationConfig": generation_config,
  }))
}

fn inline_part(data_url: &str) -> Result<Value, String> {
  let image = decode_image(data_url)?;
  Ok(json!({ "inline_data": { "mime_type": image.mime, "data": BASE64.encode(&image.bytes) } }))
}

fn parse_response(value: &Value) -> Result<Vec<Output>, String> {
  let mut outputs = Vec::new();
  let mut notes = Vec::new();
  let mut finish_reason = None;

  for candidate in value.get("candidates").and_then(Value::as_array).into_iter().flatten() {
    finish_reason = candidate.get("finishReason").and_then(Value::as_str).or(finish_reason);
    let parts = candidate.pointer("/content/parts").and_then(Value::as_array);
    for part in parts.into_iter().flatten() {
      if let Some(text) = part.get("text").and_then(Value::as_str) {
        if part.get("thought").and_then(Value::as_bool) != Some(true) {
          notes.push(text.trim().to_string());
        }
      }
      let inline = part.get("inlineData").or_else(|| part.get("inline_data"));
      if let Some(inline) = inline {
        let mime = inline
          .get("mimeType")
          .or_else(|| inline.get("mime_type"))
          .and_then(Value::as_str)
          .unwrap_or("image/png");
        let data = inline.get("data").and_then(Value::as_str).unwrap_or_default();
        let bytes = BASE64.decode(data).map_err(|_| "Gemini sent an image Easel couldn't decode.".to_string())?;
        outputs.push(Output { bytes, extension: extension_for_mime(mime).into(), seed: None, note: String::new() });
      }
    }
  }

  let note = notes.join("\n").trim().to_string();
  if outputs.is_empty() {
    let blocked = value.pointer("/promptFeedback/blockReason").and_then(Value::as_str);
    return Err(match (blocked, finish_reason, note.is_empty()) {
      (Some(reason), _, _) => format!("Gemini blocked this prompt ({reason}). Try rewording it."),
      (_, _, false) => format!("Gemini answered with text instead of an image: {note}"),
      (_, Some(reason), _) => format!("Gemini didn't return an image (stopped with {reason}). Try rewording the prompt."),
      _ => "Gemini didn't return an image. Try rewording the prompt.".into(),
    });
  }
  for output in &mut outputs {
    output.note = note.clone();
  }
  Ok(outputs)
}

/// The visible answer, without "thought" parts.
fn answer_text(value: &Value) -> String {
  value
    .pointer("/candidates/0/content/parts")
    .and_then(Value::as_array)
    .into_iter()
    .flatten()
    .filter(|p| p.get("thought").and_then(Value::as_bool) != Some(true))
    .filter_map(|p| p.get("text").and_then(Value::as_str))
    .collect::<Vec<_>>()
    .join("")
}

/// Gemini text models: chat-capable, not image, embedding, TTS or audio models.
fn text_model_names(value: &Value) -> Vec<String> {
  let mut names: Vec<String> = generate_content_models(value)
    .into_iter()
    .filter(|n| n.starts_with("gemini") && !["image", "embedding", "tts", "audio", "live"].iter().any(|x| n.contains(x)))
    .collect();
  names.sort();
  names.dedup();
  names
}

/// The highest-numbered stable Pro model, falling back to any newest model.
fn newest_pro(names: &[String]) -> Option<String> {
  let version = |n: &str| -> f64 {
    n.trim_start_matches("gemini-").split('-').next().and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0)
  };
  let rank = |n: &String| (n.contains("pro") as u8, !n.contains("preview") && !n.contains("exp") as bool, version(n));
  names.iter().max_by(|a, b| rank(a).partial_cmp(&rank(b)).unwrap_or(std::cmp::Ordering::Equal)).cloned()
}

fn generate_content_models(value: &Value) -> Vec<String> {
  value
    .get("models")
    .and_then(Value::as_array)
    .into_iter()
    .flatten()
    .filter(|m| {
      m.get("supportedGenerationMethods")
        .and_then(Value::as_array)
        .map(|methods| methods.iter().any(|x| x.as_str() == Some("generateContent")))
        .unwrap_or(false)
    })
    .filter_map(|m| m.get("name").and_then(Value::as_str))
    .map(|name| name.trim_start_matches("models/").to_string())
    .collect()
}

fn image_model_names(value: &Value) -> Vec<String> {
  let mut names: Vec<String> = value
    .get("models")
    .and_then(Value::as_array)
    .into_iter()
    .flatten()
    .filter(|m| {
      m.get("supportedGenerationMethods")
        .and_then(Value::as_array)
        .map(|methods| methods.iter().any(|x| x.as_str() == Some("generateContent")))
        .unwrap_or(false)
    })
    .filter_map(|m| m.get("name").and_then(Value::as_str))
    .map(|name| name.trim_start_matches("models/").to_string())
    .filter(|name| name.contains("image"))
    .collect();
  names.sort();
  names.dedup();
  names
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::providers::Provider;

  fn request(prompt: &str) -> GenerateRequest {
    GenerateRequest {
      provider: Provider::Gemini,
      prompt: prompt.into(),
      negative: None,
      aspect_ratio: "16:9".into(),
      count: 1,
      seed: None,
      images: vec![],
      image_paths: vec![],
      mask: None,
      strength: 0.6,
      workflow_id: None,
      model: None,
    }
  }

  const PNG_B64: &str = "iVBORw0KGgo=";

  #[test]
  fn text_to_image_body_sets_aspect_ratio() {
    let body = build_body(&request("a lighthouse")).unwrap();
    assert_eq!(body.pointer("/generationConfig/imageConfig/aspectRatio").unwrap(), "16:9");
    assert_eq!(body.pointer("/contents/0/parts/0/text").unwrap(), "a lighthouse");
  }

  #[test]
  fn masked_edit_sends_image_then_mask_then_instruction() {
    let mut req = request("make the sky purple");
    req.images = vec![format!("data:image/png;base64,{PNG_B64}")];
    req.mask = Some(format!("data:image/png;base64,{PNG_B64}"));
    let body = build_body(&req).unwrap();
    let parts = body.pointer("/contents/0/parts").unwrap().as_array().unwrap();
    assert_eq!(parts.len(), 3);
    assert!(parts[2]["text"].as_str().unwrap().ends_with("make the sky purple"));
    assert!(body.pointer("/generationConfig/imageConfig").is_none());
  }

  #[test]
  fn empty_prompt_is_refused() {
    assert!(build_body(&request("  ")).is_err());
  }

  #[test]
  fn parses_images_and_text() {
    let reply = json!({ "candidates": [{ "content": { "parts": [
      { "text": "Here you go" },
      { "inlineData": { "mimeType": "image/png", "data": PNG_B64 } }
    ]}}]});
    let out = parse_response(&reply).unwrap();
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].extension, "png");
    assert_eq!(out[0].note, "Here you go");
  }

  #[test]
  fn explains_a_blocked_prompt() {
    let reply = json!({ "promptFeedback": { "blockReason": "SAFETY" } });
    assert!(parse_response(&reply).unwrap_err().contains("SAFETY"));
  }

  #[test]
  fn picks_the_newest_stable_pro_text_model() {
    let listing = json!({ "models": [
      { "name": "models/gemini-2.5-pro", "supportedGenerationMethods": ["generateContent"] },
      { "name": "models/gemini-4-pro", "supportedGenerationMethods": ["generateContent"] },
      { "name": "models/gemini-4-pro-preview-09", "supportedGenerationMethods": ["generateContent"] },
      { "name": "models/gemini-4-flash", "supportedGenerationMethods": ["generateContent"] },
      { "name": "models/gemini-4-flash-image", "supportedGenerationMethods": ["generateContent"] },
      { "name": "models/text-embedding-005", "supportedGenerationMethods": ["embedContent"] }
    ]});
    let names = text_model_names(&listing);
    assert!(!names.iter().any(|n| n.contains("image")));
    assert_eq!(newest_pro(&names).as_deref(), Some("gemini-4-pro"));
  }

  #[test]
  fn answer_skips_thought_parts() {
    let reply = json!({ "candidates": [{ "content": { "parts": [
      { "text": "thinking…", "thought": true }, { "text": "{\"reply\":\"hi\"}" }
    ]}}]});
    assert_eq!(answer_text(&reply), "{\"reply\":\"hi\"}");
  }

  #[test]
  fn lists_only_image_models() {
    let listing = json!({ "models": [
      { "name": "models/gemini-2.5-flash-image", "supportedGenerationMethods": ["generateContent"] },
      { "name": "models/gemini-2.5-flash", "supportedGenerationMethods": ["generateContent"] },
      { "name": "models/imagen-4.0-generate-001", "supportedGenerationMethods": ["predict"] }
    ]});
    assert_eq!(image_model_names(&listing), vec!["gemini-2.5-flash-image"]);
  }
}
