//! ComfyUI running on your own PC (or another machine on your network).
//! Built-in graphs cover text-to-image, image-to-image and painted-mask fills
//! for any checkpoint ComfyUI can load in one file (SD 1.5, SDXL, Flux
//! all-in-one). Anything else, including local video, runs as your own
//! workflow exported with "Save (API Format)".

use super::{decode_image, error_text, random_seed, GenerateRequest, Output};
use crate::settings::{ComfySettings, CustomWorkflow};
use serde_json::{json, Map, Value};
use std::time::{Duration, Instant};

const POLL_EVERY: Duration = Duration::from_millis(800);
/// Local video can take a long time on one GPU.
const GIVE_UP_AFTER: Duration = Duration::from_secs(60 * 60);

pub async fn generate(client: &reqwest::Client, settings: &ComfySettings, req: &GenerateRequest) -> Result<Vec<Output>, String> {
  let base = base_url(settings)?;
  let checkpoint = req.model.clone().filter(|m| !m.trim().is_empty()).unwrap_or_else(|| settings.checkpoint.clone());
  if checkpoint.trim().is_empty() {
    return Err("Pick a ComfyUI checkpoint in Settings first.".into());
  }
  if req.prompt.trim().is_empty() {
    return Err("Type what you want to see first.".into());
  }

  let image_name = match req.images.first() {
    Some(image) => Some(upload(client, &base, image, "easel-input.png").await?),
    None => None,
  };
  let mask_name = match &req.mask {
    Some(mask) => Some(upload(client, &base, mask, "easel-mask.png").await?),
    None => None,
  };
  let (width, height) = size_for(&req.aspect_ratio, settings.base_size);
  let negative = req.negative.clone().unwrap_or_else(|| settings.negative.clone());

  let mut outputs = Vec::new();
  let first_seed = req.seed.unwrap_or_else(random_seed);
  for i in 0..req.count() as u64 {
    let seed = first_seed.wrapping_add(i);
    let graph = build_graph(&GraphInput {
      checkpoint: &checkpoint,
      prompt: req.prompt.trim(),
      negative: &negative,
      seed,
      settings,
      width,
      height,
      image: image_name.as_deref(),
      mask: mask_name.as_deref(),
      strength: req.strength,
    });
    for mut output in run_graph(client, &base, graph).await? {
      output.seed = Some(seed);
      outputs.push(output);
    }
  }
  Ok(outputs)
}

/// Runs one of your saved workflows, filling in its {{placeholders}}.
pub async fn run_workflow(client: &reqwest::Client, settings: &ComfySettings, workflow: &CustomWorkflow, req: &GenerateRequest) -> Result<Vec<Output>, String> {
  let base = base_url(settings)?;
  let template: Value = serde_json::from_str(&workflow.json)
    .map_err(|e| format!("The workflow \"{}\" isn't valid JSON: {e}", workflow.name))?;
  if template.get("nodes").is_some() && template.get("links").is_some() {
    return Err(format!(
      "\"{}\" was saved in ComfyUI's normal format. Re-export it with Workflow > Export (API) and paste that instead.",
      workflow.name
    ));
  }

  let image_name = match req.images.first() {
    Some(image) => Some(upload(client, &base, image, "easel-input.png").await?),
    None => None,
  };
  let mask_name = match &req.mask {
    Some(mask) => Some(upload(client, &base, mask, "easel-mask.png").await?),
    None => None,
  };
  let (width, height) = size_for(&req.aspect_ratio, settings.base_size);
  let negative = req.negative.clone().unwrap_or_else(|| settings.negative.clone());

  let mut outputs = Vec::new();
  let first_seed = req.seed.unwrap_or_else(random_seed);
  for i in 0..req.count() as u64 {
    let seed = first_seed.wrapping_add(i);
    let values = Placeholders {
      prompt: req.prompt.trim(),
      negative: &negative,
      seed,
      width,
      height,
      image: image_name.as_deref().unwrap_or_default(),
      mask: mask_name.as_deref().unwrap_or_default(),
      strength: req.strength,
    };
    let graph = fill_placeholders(&template, &values);
    for mut output in run_graph(client, &base, graph).await? {
      output.seed = Some(seed);
      outputs.push(output);
    }
  }
  Ok(outputs)
}

pub async fn list_checkpoints(client: &reqwest::Client, settings: &ComfySettings) -> Result<Vec<String>, String> {
  let base = base_url(settings)?;
  let resp = client
    .get(format!("{base}/object_info/CheckpointLoaderSimple"))
    .send()
    .await
    .map_err(|e| unreachable_message(&base, e))?;
  let status = resp.status();
  let text = resp.text().await.map_err(|e| e.to_string())?;
  if !status.is_success() {
    return Err(format!("ComfyUI said: {}", error_text(status, &text)));
  }
  let value: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
  Ok(checkpoint_names(&value))
}

struct GraphInput<'a> {
  checkpoint: &'a str,
  prompt: &'a str,
  negative: &'a str,
  seed: u64,
  settings: &'a ComfySettings,
  width: u32,
  height: u32,
  image: Option<&'a str>,
  mask: Option<&'a str>,
  strength: f32,
}

/// Builds a ComfyUI API-format graph. Node ids are arbitrary strings.
fn build_graph(input: &GraphInput) -> Value {
  let s = input.settings;
  let mut graph = json!({
    "ckpt": { "class_type": "CheckpointLoaderSimple", "inputs": { "ckpt_name": input.checkpoint } },
    "pos": { "class_type": "CLIPTextEncode", "inputs": { "text": input.prompt, "clip": ["ckpt", 1] } },
    "neg": { "class_type": "CLIPTextEncode", "inputs": { "text": input.negative, "clip": ["ckpt", 1] } },
    "decode": { "class_type": "VAEDecode", "inputs": { "samples": ["sampler", 0], "vae": ["ckpt", 2] } },
    "save": { "class_type": "SaveImage", "inputs": { "filename_prefix": "easel", "images": ["decode", 0] } },
  });

  let (latent, denoise) = match (input.image, input.mask) {
    (Some(image), Some(mask)) => {
      graph["load"] = json!({ "class_type": "LoadImage", "inputs": { "image": image } });
      graph["loadMask"] = json!({ "class_type": "LoadImageMask", "inputs": { "image": mask, "channel": "red" } });
      graph["latent"] = json!({ "class_type": "VAEEncodeForInpaint", "inputs": {
        "pixels": ["load", 0], "vae": ["ckpt", 2], "mask": ["loadMask", 0], "grow_mask_by": 8
      }});
      // A masked fill replaces the area completely.
      ("latent", 1.0)
    }
    (Some(image), None) => {
      graph["load"] = json!({ "class_type": "LoadImage", "inputs": { "image": image } });
      graph["latent"] = json!({ "class_type": "VAEEncode", "inputs": { "pixels": ["load", 0], "vae": ["ckpt", 2] } });
      ("latent", input.strength.clamp(0.05, 1.0))
    }
    _ => {
      graph["latent"] = json!({ "class_type": "EmptyLatentImage", "inputs": {
        "width": input.width, "height": input.height, "batch_size": 1
      }});
      ("latent", 1.0)
    }
  };

  graph["sampler"] = json!({ "class_type": "KSampler", "inputs": {
    "seed": input.seed,
    "steps": s.steps.clamp(1, 150),
    "cfg": s.cfg,
    "sampler_name": s.sampler,
    "scheduler": s.scheduler,
    "denoise": denoise,
    "model": ["ckpt", 0],
    "positive": ["pos", 0],
    "negative": ["neg", 0],
    "latent_image": [latent, 0],
  }});
  graph
}

struct Placeholders<'a> {
  prompt: &'a str,
  negative: &'a str,
  seed: u64,
  width: u32,
  height: u32,
  image: &'a str,
  mask: &'a str,
  strength: f32,
}

/// A text value that is exactly "{{seed}}" (or width, height, strength)
/// becomes a number; {{prompt}} and friends are replaced inside any text.
fn fill_placeholders(value: &Value, p: &Placeholders) -> Value {
  match value {
    Value::String(text) => match text.trim() {
      "{{seed}}" => json!(p.seed),
      "{{width}}" => json!(p.width),
      "{{height}}" => json!(p.height),
      "{{strength}}" => json!(p.strength),
      _ => Value::String(
        text
          .replace("{{prompt}}", p.prompt)
          .replace("{{negative}}", p.negative)
          .replace("{{image}}", p.image)
          .replace("{{mask}}", p.mask),
      ),
    },
    Value::Array(items) => Value::Array(items.iter().map(|v| fill_placeholders(v, p)).collect()),
    Value::Object(map) => Value::Object(map.iter().map(|(k, v)| (k.clone(), fill_placeholders(v, p))).collect::<Map<_, _>>()),
    other => other.clone(),
  }
}

async fn run_graph(client: &reqwest::Client, base: &str, graph: Value) -> Result<Vec<Output>, String> {
  let client_id = format!("easel-{:x}", rand::random::<u64>());
  let resp = client
    .post(format!("{base}/prompt"))
    .json(&json!({ "prompt": graph, "client_id": client_id }))
    .send()
    .await
    .map_err(|e| unreachable_message(base, e))?;
  let status = resp.status();
  let text = resp.text().await.map_err(|e| e.to_string())?;
  let value: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
  if !status.is_success() {
    return Err(format!("ComfyUI rejected the job: {}", describe_queue_error(&value).unwrap_or_else(|| error_text(status, &text))));
  }
  let prompt_id = value
    .get("prompt_id")
    .and_then(Value::as_str)
    .ok_or("ComfyUI accepted the job but didn't say which one it was.")?
    .to_string();

  let started = Instant::now();
  let entry = loop {
    if started.elapsed() > GIVE_UP_AFTER {
      return Err("ComfyUI was still working after an hour, so Easel stopped waiting. The result may still appear in ComfyUI's output folder.".into());
    }
    tokio::time::sleep(POLL_EVERY).await;
    let resp = client
      .get(format!("{base}/history/{prompt_id}"))
      .send()
      .await
      .map_err(|e| unreachable_message(base, e))?;
    let history: Value = resp.json().await.unwrap_or(Value::Null);
    if let Some(entry) = history.get(&prompt_id) {
      if entry.pointer("/status/completed").and_then(Value::as_bool) == Some(true)
        || entry.pointer("/status/status_str").and_then(Value::as_str) == Some("error")
      {
        break entry.clone();
      }
    }
  };

  if entry.pointer("/status/status_str").and_then(Value::as_str) == Some("error") {
    return Err(format!("ComfyUI failed while running the job: {}", describe_run_error(&entry)));
  }

  let files = output_files(&entry);
  if files.is_empty() {
    return Err("ComfyUI finished but saved nothing. Make sure the workflow ends in a Save Image or Save Video node.".into());
  }
  let mut outputs = Vec::new();
  for file in files {
    let resp = client
      .get(format!("{base}/view"))
      .query(&[("filename", &file.filename), ("subfolder", &file.subfolder), ("type", &file.kind)])
      .send()
      .await
      .map_err(|e| unreachable_message(base, e))?;
    if !resp.status().is_success() {
      return Err(format!("Couldn't download {} from ComfyUI ({}).", file.filename, resp.status().as_u16()));
    }
    let bytes = resp.bytes().await.map_err(|e| e.to_string())?.to_vec();
    let extension = file.filename.rsplit('.').next().unwrap_or("png").to_ascii_lowercase();
    outputs.push(Output { bytes, extension, seed: None, note: String::new() });
  }
  Ok(outputs)
}

async fn upload(client: &reqwest::Client, base: &str, data_url: &str, file_name: &str) -> Result<String, String> {
  let image = decode_image(data_url)?;
  let part = reqwest::multipart::Part::bytes(image.bytes)
    .file_name(file_name.to_string())
    .mime_str(&image.mime)
    .map_err(|e| e.to_string())?;
  // Without overwrite, ComfyUI renames repeat uploads; the reply has the real name.
  let form = reqwest::multipart::Form::new().part("image", part).text("type", "input").text("overwrite", "false");
  let resp = client
    .post(format!("{base}/upload/image"))
    .multipart(form)
    .send()
    .await
    .map_err(|e| unreachable_message(base, e))?;
  let status = resp.status();
  let text = resp.text().await.map_err(|e| e.to_string())?;
  if !status.is_success() {
    return Err(format!("ComfyUI didn't accept the image: {}", error_text(status, &text)));
  }
  let value: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
  let name = value.get("name").and_then(Value::as_str).ok_or("ComfyUI didn't return a name for the uploaded image.")?;
  Ok(match value.get("subfolder").and_then(Value::as_str) {
    Some(sub) if !sub.is_empty() => format!("{sub}/{name}"),
    _ => name.to_string(),
  })
}

struct OutputFile {
  filename: String,
  subfolder: String,
  kind: String,
}

/// Saved files from every output node: images, plus videos and animations
/// from Save Video, Save Animated WEBP and Video Helper Suite nodes.
fn output_files(entry: &Value) -> Vec<OutputFile> {
  let mut files = Vec::new();
  let Some(outputs) = entry.get("outputs").and_then(Value::as_object) else { return files };
  let mut node_ids: Vec<&String> = outputs.keys().collect();
  node_ids.sort();
  for node_id in node_ids {
    for key in ["images", "gifs", "videos", "animated"] {
      for item in outputs[node_id].get(key).and_then(Value::as_array).into_iter().flatten() {
        let kind = item.get("type").and_then(Value::as_str).unwrap_or("output");
        // Preview nodes write throwaway temp files; only keep saved results.
        if kind != "output" {
          continue;
        }
        if let Some(filename) = item.get("filename").and_then(Value::as_str) {
          files.push(OutputFile {
            filename: filename.to_string(),
            subfolder: item.get("subfolder").and_then(Value::as_str).unwrap_or_default().to_string(),
            kind: kind.to_string(),
          });
        }
      }
    }
  }
  files
}

fn checkpoint_names(value: &Value) -> Vec<String> {
  let field = value.pointer("/CheckpointLoaderSimple/input/required/ckpt_name/0");
  let list = match field {
    // Newer ComfyUI: ["COMBO", { "options": [...] }]
    Some(Value::String(kind)) if kind == "COMBO" => value.pointer("/CheckpointLoaderSimple/input/required/ckpt_name/1/options"),
    other => other,
  };
  list
    .and_then(Value::as_array)
    .into_iter()
    .flatten()
    .filter_map(|v| v.as_str().map(str::to_string))
    .collect()
}

fn describe_queue_error(value: &Value) -> Option<String> {
  let mut parts = Vec::new();
  if let Some(message) = value.pointer("/error/message").and_then(Value::as_str) {
    parts.push(message.to_string());
  }
  if let Some(nodes) = value.get("node_errors").and_then(Value::as_object) {
    for node in nodes.values() {
      let class = node.get("class_type").and_then(Value::as_str).unwrap_or("node");
      for err in node.get("errors").and_then(Value::as_array).into_iter().flatten() {
        let msg = err.get("message").and_then(Value::as_str).unwrap_or_default();
        let details = err.get("details").and_then(Value::as_str).unwrap_or_default();
        parts.push(format!("{class}: {msg} {details}").trim().to_string());
      }
    }
  }
  if parts.is_empty() { None } else { Some(parts.join("; ")) }
}

fn describe_run_error(entry: &Value) -> String {
  entry
    .pointer("/status/messages")
    .and_then(Value::as_array)
    .into_iter()
    .flatten()
    .filter(|m| m.get(0).and_then(Value::as_str) == Some("execution_error"))
    .filter_map(|m| {
      let detail = m.get(1)?;
      let node = detail.get("node_type").and_then(Value::as_str).unwrap_or("a node");
      let message = detail.get("exception_message").and_then(Value::as_str).unwrap_or("unknown error");
      Some(format!("{node}: {}", message.trim()))
    })
    .next()
    .unwrap_or_else(|| "no details given; check the ComfyUI window".into())
}

/// Pixel size for an aspect ratio, keeping about `base`² pixels, in steps of 64.
fn size_for(aspect: &str, base: u32) -> (u32, u32) {
  let base = base.clamp(256, 2048) as f64;
  let (w, h) = aspect
    .split_once(':')
    .and_then(|(w, h)| Some((w.trim().parse::<f64>().ok()?, h.trim().parse::<f64>().ok()?)))
    .filter(|(w, h)| *w > 0.0 && *h > 0.0)
    .unwrap_or((1.0, 1.0));
  let ratio = w / h;
  let width = base * ratio.sqrt();
  let height = base / ratio.sqrt();
  let snap = |v: f64| ((v / 64.0).round() as u32).max(4) * 64;
  (snap(width), snap(height))
}

fn base_url(settings: &ComfySettings) -> Result<String, String> {
  let url = settings.url.trim().trim_end_matches('/');
  if url.is_empty() {
    return Err("Set the ComfyUI address in Settings first.".into());
  }
  Ok(url.to_string())
}

fn unreachable_message(base: &str, err: reqwest::Error) -> String {
  format!("Couldn't reach ComfyUI at {base}. Is it running? ({err})")
}

#[cfg(test)]
mod tests {
  use super::*;

  fn settings() -> ComfySettings {
    ComfySettings::default()
  }

  fn input<'a>(s: &'a ComfySettings, image: Option<&'a str>, mask: Option<&'a str>) -> GraphInput<'a> {
    GraphInput {
      checkpoint: "sdxl.safetensors",
      prompt: "a fox",
      negative: "blurry",
      seed: 7,
      settings: s,
      width: 1024,
      height: 1024,
      image,
      mask,
      strength: 0.4,
    }
  }

  #[test]
  fn text_to_image_uses_an_empty_latent() {
    let s = settings();
    let g = build_graph(&input(&s, None, None));
    assert_eq!(g["latent"]["class_type"], "EmptyLatentImage");
    assert_eq!(g["sampler"]["inputs"]["denoise"], 1.0);
    assert_eq!(g["sampler"]["inputs"]["seed"], 7);
  }

  #[test]
  fn image_to_image_uses_strength_as_denoise() {
    let s = settings();
    let g = build_graph(&input(&s, Some("in.png"), None));
    assert_eq!(g["latent"]["class_type"], "VAEEncode");
    assert!((g["sampler"]["inputs"]["denoise"].as_f64().unwrap() - 0.4).abs() < 1e-6);
  }

  #[test]
  fn masked_fill_encodes_for_inpainting() {
    let s = settings();
    let g = build_graph(&input(&s, Some("in.png"), Some("mask.png")));
    assert_eq!(g["latent"]["class_type"], "VAEEncodeForInpaint");
    assert_eq!(g["loadMask"]["inputs"]["image"], "mask.png");
  }

  #[test]
  fn placeholders_become_numbers_or_text() {
    let template = json!({ "3": { "inputs": { "seed": "{{seed}}", "text": "photo of {{prompt}}", "width": "{{width}}" } } });
    let p = Placeholders { prompt: "a fox", negative: "", seed: 42, width: 832, height: 1216, image: "", mask: "", strength: 0.5 };
    let filled = fill_placeholders(&template, &p);
    assert_eq!(filled["3"]["inputs"]["seed"], 42);
    assert_eq!(filled["3"]["inputs"]["width"], 832);
    assert_eq!(filled["3"]["inputs"]["text"], "photo of a fox");
  }

  #[test]
  fn sizes_keep_about_the_same_pixel_count() {
    assert_eq!(size_for("1:1", 1024), (1024, 1024));
    assert_eq!(size_for("16:9", 1024), (1344, 768));
    assert_eq!(size_for("9:16", 1024), (768, 1344));
    assert_eq!(size_for("nonsense", 512), (512, 512));
  }

  #[test]
  fn reads_both_checkpoint_list_formats() {
    let old = json!({ "CheckpointLoaderSimple": { "input": { "required": { "ckpt_name": [["a.safetensors", "b.ckpt"]] } } } });
    let new = json!({ "CheckpointLoaderSimple": { "input": { "required": { "ckpt_name": ["COMBO", { "options": ["c.safetensors"] }] } } } });
    assert_eq!(checkpoint_names(&old), vec!["a.safetensors", "b.ckpt"]);
    assert_eq!(checkpoint_names(&new), vec!["c.safetensors"]);
  }

  #[test]
  fn collects_saved_images_and_videos_but_not_previews() {
    let entry = json!({ "outputs": {
      "9": { "images": [{ "filename": "easel_0001.png", "subfolder": "", "type": "output" }] },
      "12": { "gifs": [{ "filename": "clip.mp4", "subfolder": "video", "type": "output" }] },
      "20": { "images": [{ "filename": "preview.png", "subfolder": "", "type": "temp" }] }
    }});
    let names: Vec<String> = output_files(&entry).into_iter().map(|f| f.filename).collect();
    assert_eq!(names, vec!["clip.mp4", "easel_0001.png"]);
  }

  #[test]
  fn explains_node_errors() {
    let reply = json!({ "error": { "message": "Prompt outputs failed validation" }, "node_errors": {
      "ckpt": { "class_type": "CheckpointLoaderSimple", "errors": [{ "message": "Value not in list", "details": "ckpt_name: 'x' not in []" }] }
    }});
    let text = describe_queue_error(&reply).unwrap();
    assert!(text.contains("CheckpointLoaderSimple: Value not in list"));
  }
}
