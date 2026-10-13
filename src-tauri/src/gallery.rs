//! The gallery is just a folder on disk. Each result is a normal image or video
//! file, plus a small `.json` file next to it with the prompt and settings, so
//! you can browse, copy or back up results with any file manager.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const MEDIA_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "webp", "gif", "mp4", "webm"];
const VIDEO_EXTENSIONS: &[&str] = &["mp4", "webm"];

/// What made an image: saved next to it as `<name>.json`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct MediaMeta {
  pub prompt: String,
  pub provider: String,
  pub model: String,
  pub mode: String,
  pub seed: Option<u64>,
  pub created_at: String,
  /// Any text the model returned alongside the image.
  pub note: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GalleryItem {
  pub path: String,
  pub file_name: String,
  pub is_video: bool,
  pub modified_ms: u64,
  pub meta: Option<MediaMeta>,
}

/// Saves one result into today's folder and returns its gallery entry.
pub fn save_result(output_dir: &Path, bytes: &[u8], extension: &str, meta: &MediaMeta) -> Result<GalleryItem, String> {
  let now = chrono::Local::now();
  let day_dir = output_dir.join(now.format("%Y-%m-%d").to_string());
  std::fs::create_dir_all(&day_dir)
    .map_err(|e| format!("Couldn't create the output folder {}: {e}", day_dir.display()))?;

  let ext = sanitize_extension(extension);
  let stem = format!("easel-{}-{:04x}", now.format("%H%M%S"), rand::random::<u16>());
  let media_path = day_dir.join(format!("{stem}.{ext}"));
  std::fs::write(&media_path, bytes).map_err(|e| format!("Couldn't save the image: {e}"))?;

  let mut meta = meta.clone();
  meta.created_at = now.to_rfc3339();
  let meta_json = serde_json::to_string_pretty(&meta).map_err(|e| e.to_string())?;
  // The image is already saved; a missing sidecar only loses the prompt text.
  if let Err(err) = std::fs::write(day_dir.join(format!("{stem}.json")), meta_json) {
    log::warn!("couldn't write sidecar for {}: {err}", media_path.display());
  }

  item_for(&media_path).ok_or_else(|| "The image was saved but couldn't be read back.".to_string())
}

/// Every image and video in the output folder (and one level of subfolders), newest first.
pub fn list(output_dir: &Path) -> Vec<GalleryItem> {
  let mut items = Vec::new();
  collect(output_dir, 0, &mut items);
  items.sort_by(|a, b| b.modified_ms.cmp(&a.modified_ms));
  items
}

/// Deletes a result and its sidecar. Refuses anything outside the output folder.
pub fn delete(output_dir: &Path, path: &Path) -> Result<(), String> {
  if !is_inside(output_dir, path) {
    return Err("Only files in the Easel output folder can be deleted from here.".into());
  }
  if !is_media(path) {
    return Err("That isn't an image or video.".into());
  }
  std::fs::remove_file(path).map_err(|e| format!("Couldn't delete the file: {e}"))?;
  let _ = std::fs::remove_file(path.with_extension("json"));
  Ok(())
}

pub fn is_media(path: &Path) -> bool {
  path
    .extension()
    .and_then(|e| e.to_str())
    .map(|e| MEDIA_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
    .unwrap_or(false)
}

/// True when `path` resolves to somewhere under `dir` (after following `..` and links).
pub fn is_inside(dir: &Path, path: &Path) -> bool {
  match (dir.canonicalize(), path.canonicalize()) {
    (Ok(dir), Ok(path)) => path.starts_with(dir),
    _ => false,
  }
}

fn collect(dir: &Path, depth: u32, out: &mut Vec<GalleryItem>) {
  let Ok(entries) = std::fs::read_dir(dir) else { return };
  for entry in entries.flatten() {
    let path = entry.path();
    if path.is_dir() {
      if depth < 1 {
        collect(&path, depth + 1, out);
      }
    } else if is_media(&path) {
      if let Some(item) = item_for(&path) {
        out.push(item);
      }
    }
  }
}

fn item_for(path: &Path) -> Option<GalleryItem> {
  let modified_ms = std::fs::metadata(path)
    .and_then(|m| m.modified())
    .ok()
    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
    .map(|d| d.as_millis() as u64)
    .unwrap_or(0);
  let ext = path.extension()?.to_str()?.to_ascii_lowercase();
  let meta = std::fs::read_to_string(path.with_extension("json"))
    .ok()
    .and_then(|text| serde_json::from_str(&text).ok());
  Some(GalleryItem {
    path: path_string(path),
    file_name: path.file_name()?.to_string_lossy().into_owned(),
    is_video: VIDEO_EXTENSIONS.contains(&ext.as_str()),
    modified_ms,
    meta,
  })
}

fn sanitize_extension(ext: &str) -> String {
  let ext = ext.trim_start_matches('.').to_ascii_lowercase();
  if MEDIA_EXTENSIONS.contains(&ext.as_str()) { ext } else { "png".into() }
}

fn path_string(path: &Path) -> String {
  PathBuf::from(path).to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
  use super::*;

  fn temp_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("easel-gallery-{}", rand::random::<u32>()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
  }

  #[test]
  fn save_list_and_delete() {
    let dir = temp_dir();
    let meta = MediaMeta { prompt: "a red barn".into(), provider: "gemini".into(), ..Default::default() };
    let item = save_result(&dir, b"fake-png-bytes", "png", &meta).unwrap();
    assert!(item.file_name.ends_with(".png"));

    let listed = list(&dir);
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].meta.as_ref().unwrap().prompt, "a red barn");

    delete(&dir, Path::new(&item.path)).unwrap();
    assert!(list(&dir).is_empty());
    let _ = std::fs::remove_dir_all(dir);
  }

  #[test]
  fn delete_refuses_files_outside_the_output_folder() {
    let dir = temp_dir();
    let other = temp_dir();
    let outside = other.join("keep.png");
    std::fs::write(&outside, b"x").unwrap();
    assert!(delete(&dir, &outside).is_err());
    assert!(outside.exists());
    let _ = std::fs::remove_dir_all(dir);
    let _ = std::fs::remove_dir_all(other);
  }

  #[test]
  fn unknown_extensions_are_saved_as_png() {
    assert_eq!(sanitize_extension(".WEBP"), "webp");
    assert_eq!(sanitize_extension("exe"), "png");
  }
}
