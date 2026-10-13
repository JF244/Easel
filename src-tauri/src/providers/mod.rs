pub mod claude;
pub mod cli;
pub mod comfy;
pub mod gemini;
pub mod local_llm;

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use serde::Deserialize;

/// What the Create and Edit pages send to `generate`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerateRequest {
  pub provider: Provider,
  pub prompt: String,
  #[serde(default)]
  pub negative: Option<String>,
  #[serde(default = "default_aspect")]
  pub aspect_ratio: String,
  #[serde(default = "default_count")]
  pub count: u32,
  #[serde(default)]
  pub seed: Option<u64>,
  /// Input or reference images as PNG/JPEG data URLs (or bare base64).
  #[serde(default)]
  pub images: Vec<String>,
  /// Input images given as files on disk (the brain picks these from the gallery).
  #[serde(default)]
  pub image_paths: Vec<String>,
  /// A black-and-white PNG data URL: white is the area to change.
  #[serde(default)]
  pub mask: Option<String>,
  /// How much an input image may change, 0 to 1 (ComfyUI only).
  #[serde(default = "default_strength")]
  pub strength: f32,
  #[serde(default)]
  pub workflow_id: Option<String>,
  /// Overrides the Gemini model or the ComfyUI checkpoint for this run.
  #[serde(default)]
  pub model: Option<String>,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Provider {
  Gemini,
  Comfy,
  ComfyWorkflow,
}

/// One finished image or video, before it is saved.
#[derive(Debug, Clone)]
pub struct Output {
  pub bytes: Vec<u8>,
  pub extension: String,
  pub seed: Option<u64>,
  pub note: String,
}

impl GenerateRequest {
  pub fn mode(&self) -> &'static str {
    if self.mask.is_some() {
      "inpaint"
    } else if !self.images.is_empty() || !self.image_paths.is_empty() {
      "edit"
    } else {
      "create"
    }
  }

  pub fn count(&self) -> u32 {
    self.count.clamp(1, 8)
  }
}

/// A decoded image: its bytes and MIME type.
pub struct ImageData {
  pub bytes: Vec<u8>,
  pub mime: String,
}

/// Accepts `data:image/png;base64,...` or bare base64.
pub fn decode_image(input: &str) -> Result<ImageData, String> {
  let (mime, data) = match input.strip_prefix("data:") {
    Some(rest) => {
      let (header, data) = rest.split_once(',').ok_or("The image data is incomplete.")?;
      let mime = header.split(';').next().unwrap_or("image/png").to_string();
      (mime, data)
    }
    None => (String::new(), input),
  };
  let bytes = BASE64.decode(data.trim()).map_err(|_| "The image data couldn't be read.".to_string())?;
  let mime = if mime.is_empty() { sniff_mime(&bytes).to_string() } else { mime };
  Ok(ImageData { bytes, mime })
}

pub fn sniff_mime(bytes: &[u8]) -> &'static str {
  if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
    "image/png"
  } else if bytes.starts_with(&[0xFF, 0xD8]) {
    "image/jpeg"
  } else if bytes.len() > 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
    "image/webp"
  } else {
    "image/png"
  }
}

pub fn extension_for_mime(mime: &str) -> &'static str {
  match mime {
    "image/jpeg" => "jpg",
    "image/webp" => "webp",
    "image/gif" => "gif",
    "video/mp4" => "mp4",
    "video/webm" => "webm",
    _ => "png",
  }
}

pub fn random_seed() -> u64 {
  // ComfyUI accepts seeds up to 2^64-1, but some nodes choke above 2^53.
  rand::random::<u64>() % (1u64 << 50)
}

/// Turns an HTTP error body into one short readable line.
pub fn error_text(status: reqwest::StatusCode, body: &str) -> String {
  let message = serde_json::from_str::<serde_json::Value>(body)
    .ok()
    .and_then(|v| {
      v.pointer("/error/message")
        .or_else(|| v.get("message"))
        .or_else(|| v.get("error"))
        .and_then(|m| m.as_str().map(str::to_string))
    })
    .unwrap_or_else(|| body.chars().take(300).collect());
  format!("{} ({})", message.trim(), status.as_u16())
}

fn default_aspect() -> String {
  "1:1".into()
}

fn default_count() -> u32 {
  1
}

fn default_strength() -> f32 {
  0.6
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn decodes_data_urls_and_bare_base64() {
    let png = [0x89u8, b'P', b'N', b'G', 1, 2, 3];
    let b64 = BASE64.encode(png);
    let a = decode_image(&format!("data:image/png;base64,{b64}")).unwrap();
    assert_eq!(a.mime, "image/png");
    assert_eq!(a.bytes, png);
    let b = decode_image(&b64).unwrap();
    assert_eq!(b.mime, "image/png");
  }

  #[test]
  fn readable_errors_from_json_bodies() {
    let s = reqwest::StatusCode::BAD_REQUEST;
    assert_eq!(error_text(s, r#"{"error":{"message":"API key not valid"}}"#), "API key not valid (400)");
    assert_eq!(error_text(s, "plain text"), "plain text (400)");
  }
}
