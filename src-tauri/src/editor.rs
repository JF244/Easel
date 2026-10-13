//! Real edits by the brain: Claude Code gets one Craft app's MCP tools for one
//! job, works on a copy of the file in a private job folder, and saves the
//! result there. Easel then moves the result into the gallery.
//!
//! Only Claude Code can do this today: it's the brain that can use MCP tools
//! from the command line. The other brains still plan and write prompts.

use crate::apps::{self, AppPaths, CraftApp};
use crate::gallery::{self, GalleryItem, MediaMeta};
use crate::providers::cli::{self, CliCall};
use crate::settings::BrainSettings;
use serde::Deserialize;
use serde_json::json;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Edits can take many tool calls; long videos even more.
const EDIT_TIME_LIMIT: Duration = Duration::from_secs(20 * 60);
const MAX_TURNS: &str = "60";

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorJob {
  pub app: String,
  pub instructions: String,
  #[serde(default)]
  pub input_path: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorResult {
  pub item: GalleryItem,
  /// What Claude said it did.
  pub summary: String,
}

pub async fn run(
  brain: &BrainSettings,
  overrides: &HashMap<String, AppPaths>,
  jobs_dir: &Path,
  output_dir: &Path,
  job: &EditorJob,
) -> Result<EditorResult, String> {
  let app = apps::get(&job.app).ok_or_else(|| format!("Easel doesn't know an app called \"{}\".", job.app))?;
  if job.instructions.trim().is_empty() {
    return Err("Say what the edit should do.".into());
  }

  let job_dir = jobs_dir.join(format!("job-{:08x}", rand::random::<u32>()));
  std::fs::create_dir_all(&job_dir).map_err(|e| format!("Couldn't create a work folder: {e}"))?;
  let result = run_in(brain, overrides, app, &job_dir, output_dir, job).await;
  // The job folder only ever holds copies, so it's always safe to remove.
  let _ = std::fs::remove_dir_all(&job_dir);
  result
}

async fn run_in(
  brain: &BrainSettings,
  overrides: &HashMap<String, AppPaths>,
  app: &CraftApp,
  job_dir: &Path,
  output_dir: &Path,
  job: &EditorJob,
) -> Result<EditorResult, String> {
  let server = apps::mcp_server(app, overrides, job_dir).ok_or_else(|| {
    format!("{}'s command-line tool ({}-cli) wasn't found. Install {} or set the tool's path on the Apps page.", app.name, app.id, app.name)
  })?;

  // Work on a copy, so the original is never touched.
  let input = match job.input_path.as_deref().map(str::trim).filter(|p| !p.is_empty()) {
    Some(path) => Some(copy_input(Path::new(path), job_dir)?),
    None => None,
  };
  let extension = output_extension(app);
  let output = job_dir.join(format!("result.{extension}"));

  let config_path = job_dir.join("mcp.json");
  let config = json!({ "mcpServers": { app.id: { "command": server.program, "args": server.prefix_args } } });
  std::fs::write(&config_path, config.to_string()).map_err(|e| e.to_string())?;

  let mut options = vec![
    "--output-format".to_string(),
    "text".into(),
    "--mcp-config".into(),
    config_path.to_string_lossy().into_owned(),
    "--strict-mcp-config".into(),
    "--allowedTools".into(),
    format!("mcp__{}", app.id),
    "--max-turns".into(),
    MAX_TURNS.into(),
  ];
  if !brain.claude_cli_model.trim().is_empty() {
    options.push("--model".into());
    options.push(brain.claude_cli_model.trim().into());
  }

  let prompt = edit_prompt(app, input.as_deref(), &output, job_dir, &job.instructions);
  let summary = cli::run(CliCall {
    program: &brain.claude_cli_path,
    options,
    prompt,
    work_dir: job_dir,
    label: "Claude Code",
    time_limit: EDIT_TIME_LIMIT,
  })
  .await?;

  let bytes = std::fs::read(&output).map_err(|_| {
    format!("{} finished without saving a result. Claude said: {}", app.name, first_lines(&summary))
  })?;
  let meta = MediaMeta {
    prompt: job.instructions.trim().to_string(),
    provider: app.id.into(),
    model: format!("Claude Code + {}", app.name),
    mode: "editor".into(),
    note: summary.trim().to_string(),
    ..Default::default()
  };
  let item = gallery::save_result(output_dir, &bytes, extension, &meta)?;
  Ok(EditorResult { item, summary: summary.trim().to_string() })
}

fn edit_prompt(app: &CraftApp, input: Option<&Path>, output: &Path, job_dir: &Path, instructions: &str) -> String {
  let input_line = match input {
    Some(path) => format!("Start from this file: {}", path.display()),
    None => "Start from a new, empty document.".to_string(),
  };
  format!(
    "Use the {name} tools (MCP server \"{id}\") to do this job.\n\n\
     {input_line}\n\
     The job: {instructions}\n\n\
     Save the finished result to exactly this path, in the format its extension names: {output}\n\
     Work only inside {dir}. Don't ask questions; make sensible choices and finish the job.\n\
     When you're done, reply with one short sentence about what you did, or what stopped you.",
    name = app.name,
    id = app.id,
    instructions = instructions.trim(),
    output = output.display(),
    dir = job_dir.display(),
  )
}

fn copy_input(path: &Path, job_dir: &Path) -> Result<PathBuf, String> {
  if !gallery::is_media(path) {
    return Err("The input isn't a file type the Craft apps open.".into());
  }
  let name = path.file_name().ok_or("The input path has no file name.")?;
  let copy = job_dir.join(name);
  std::fs::copy(path, &copy).map_err(|e| format!("Couldn't read {}: {e}", path.display()))?;
  Ok(copy)
}

/// What each app's results are saved as, so the gallery can show them.
fn output_extension(app: &CraftApp) -> &'static str {
  match app.id {
    "vectorcraft" => "svg",
    "pdfcraft" | "designcraft" => "pdf",
    "filmcraft" | "effectcraft" => "mp4",
    "lightcraft" => "jpg",
    _ => "png",
  }
}

fn first_lines(text: &str) -> String {
  let t: String = text.lines().take(4).collect::<Vec<_>>().join(" ");
  if t.trim().is_empty() { "(nothing)".into() } else { t.chars().take(400).collect() }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn prompt_names_the_tools_input_and_exact_output() {
    let app = apps::get("photocraft").unwrap();
    let p = edit_prompt(app, Some(Path::new("/job/in.png")), Path::new("/job/result.png"), Path::new("/job"), " crop to 16:9 ");
    assert!(p.contains("PhotoCraft tools (MCP server \"photocraft\")"));
    assert!(p.contains("Start from this file: /job/in.png"));
    assert!(p.contains("The job: crop to 16:9\n"));
    assert!(p.contains("exactly this path, in the format its extension names: /job/result.png"));
  }

  #[test]
  fn results_use_a_format_the_gallery_shows() {
    for app in apps::APPS {
      let probe = PathBuf::from(format!("x.{}", output_extension(app)));
      assert!(gallery::is_media(&probe), "{}", app.id);
    }
  }

  #[tokio::test]
  async fn a_missing_cli_is_explained_and_the_job_folder_is_removed() {
    let jobs = std::env::temp_dir().join(format!("easel-jobs-{}", rand::random::<u32>()));
    let out = jobs.join("out");
    let mut overrides = HashMap::new();
    overrides.insert("pdfcraft".to_string(), AppPaths { gui: String::new(), cli: "/nowhere/pdfcraft-cli".into() });
    let job = EditorJob { app: "pdfcraft".into(), instructions: "rotate page 2".into(), input_path: None };
    // Only meaningful when PdfCraft isn't installed on the test machine.
    if apps::status(apps::get("pdfcraft").unwrap(), &overrides).cli.is_none() {
      let err = run(&BrainSettings::default(), &overrides, &jobs, &out, &job).await.unwrap_err();
      assert!(err.contains("pdfcraft-cli"), "{err}");
      assert_eq!(std::fs::read_dir(&jobs).map(|d| d.count()).unwrap_or(0), 0);
    }
    let _ = std::fs::remove_dir_all(jobs);
  }
}
