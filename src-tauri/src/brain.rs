//! The brain plans jobs and writes prompts. It tries each brain in your chosen
//! order: when one is missing, out of usage, or answers with something Easel
//! can't use, the next one gets the same request, and the reply says which
//! brain answered and why the earlier ones were skipped.

use crate::providers::cli::{self, CliCall, PLANNING_TIME_LIMIT};
use crate::providers::{claude, local_llm};
use crate::settings::{BrainKind, BrainSettings};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::Path;

const PLANNER_SYSTEM: &str = r#"You are the planner inside Easel, a desktop app that makes images with AI.
You never draw yourself. You turn the person's request into image jobs that Easel runs.

Reply with one JSON object and nothing else (no code fences, no commentary):
{
  "reply": "one or two plain sentences to the person about what you're making, or a question if the request is unclear",
  "jobs": [
    {
      "engine": "gemini" | "comfy" | "workflow" | "editor",
      "workflow": "name of the saved workflow, only when engine is workflow",
      "app": "id of the editor app, only when engine is editor",
      "prompt": "the full prompt for the image model",
      "negative": "things to avoid (comfy and workflow only, optional)",
      "aspectRatio": "1:1" | "16:9" | "9:16" | "4:3" | "3:4" | "3:2" | "2:3",
      "count": 1,
      "inputImage": "path of one image from the list below to edit, optional"
    }
  ]
}

Rules:
- Only use engines listed as available. If none are available, return no jobs and say what to set up.
- Write prompts the way image models like them: subject, setting, composition, lighting, style. Keep any exact text the person wants shown in quotes.
- Gemini is best for AI edits and following detailed instructions. Comfy runs on the person's own GPU for free.
- "editor" jobs do precise, non-AI edits in a real editing app: cropping, resizing, color and exposure, adding exact text or a logo, layouts, vector art, PDF pages, video cuts. For an editor job, "prompt" is the step-by-step instruction for the app, and aspectRatio, count and negative are ignored. Prefer an editor job when the person wants exact text, an exact size, or wants to keep the picture itself unchanged.
- You can chain jobs: generate a picture first, then a later turn can add text to it with an editor.
- To change an existing image, set inputImage to its path, copied exactly from the list. Never invent paths.
- count is 1 to 4. Use more than one job only when the person asks for different images.
- If the request is a question, answer it in "reply" and return an empty jobs list."#;

const IMPROVE_SYSTEM: &str = "You rewrite prompts for an image generation model. Keep the person's intent, \
subject and any quoted text exactly. Add concrete visual detail: setting, composition, lighting, style, lens. \
One paragraph, under 120 words. Reply with the rewritten prompt only, no quotes or commentary.";

const IMPROVE_EDIT_SYSTEM: &str = "You rewrite image edit instructions so an image model follows them precisely. \
Say exactly what to change and what must stay the same. Under 80 words. Reply with the instruction only.";

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatTurn {
  pub role: String,
  pub text: String,
}

/// What Easel tells the planner about this machine.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct PlannerContext {
  pub gemini_ready: bool,
  pub comfy_ready: bool,
  pub workflows: Vec<String>,
  /// Editor apps the brain can drive: (id, what it does).
  pub editors: Vec<ContextEditor>,
  /// Images the person attached or recently made: (path, prompt).
  pub images: Vec<ContextImage>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct ContextEditor {
  pub id: String,
  pub purpose: String,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct ContextImage {
  pub path: String,
  pub prompt: String,
  pub attached: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct PlannedJob {
  pub engine: String,
  pub workflow: Option<String>,
  pub app: Option<String>,
  pub prompt: String,
  pub negative: Option<String>,
  pub aspect_ratio: Option<String>,
  pub count: Option<u32>,
  pub input_image: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Plan {
  pub reply: String,
  pub jobs: Vec<PlannedJob>,
  /// Which brain answered, e.g. "Claude Code".
  pub brain: String,
  /// Brains that were tried first and why they were skipped.
  pub skipped: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Improved {
  pub prompt: String,
  pub brain: String,
  pub skipped: Vec<String>,
}

pub async fn plan(client: &reqwest::Client, settings: &BrainSettings, work_dir: &Path, turns: &[ChatTurn], context: &PlannerContext) -> Result<Plan, String> {
  let user = planner_message(turns, context);
  let mut skipped = Vec::new();
  for kind in brains_in_order(settings) {
    match ask(client, settings, work_dir, kind, PLANNER_SYSTEM, &user).await.and_then(|text| parse_plan(&text)) {
      Ok((reply, jobs)) => return Ok(Plan { reply, jobs, brain: label(kind).into(), skipped }),
      Err(err) => skipped.push(format!("{}: {err}", label(kind))),
    }
  }
  Err(no_brain_message(&skipped))
}

pub async fn improve(client: &reqwest::Client, settings: &BrainSettings, work_dir: &Path, prompt: &str, editing: bool) -> Result<Improved, String> {
  if prompt.trim().is_empty() {
    return Err("Type a prompt first, then the brain can improve it.".into());
  }
  let system = if editing { IMPROVE_EDIT_SYSTEM } else { IMPROVE_SYSTEM };
  let mut skipped = Vec::new();
  for kind in brains_in_order(settings) {
    match ask(client, settings, work_dir, kind, system, prompt.trim()).await {
      Ok(text) => {
        let cleaned = clean_single_answer(&text);
        if !cleaned.is_empty() {
          return Ok(Improved { prompt: cleaned, brain: label(kind).into(), skipped });
        }
        skipped.push(format!("{}: empty answer", label(kind)));
      }
      Err(err) => skipped.push(format!("{}: {err}", label(kind))),
    }
  }
  Err(no_brain_message(&skipped))
}

/// Sends one question to one brain.
async fn ask(client: &reqwest::Client, s: &BrainSettings, work_dir: &Path, kind: BrainKind, system: &str, user: &str) -> Result<String, String> {
  match kind {
    BrainKind::ClaudeApi => claude::complete(client, &s.claude_api_key, &s.claude_api_model, system, user).await,
    BrainKind::Local => local_llm::complete(client, &s.local_url, &s.local_model, system, user).await,
    BrainKind::ClaudeCli => {
      let mut options = vec!["--output-format".into(), "text".into(), "--append-system-prompt".into(), system.into()];
      if !s.claude_cli_model.trim().is_empty() {
        options.push("--model".into());
        options.push(s.claude_cli_model.trim().into());
      }
      cli::run(CliCall { program: &s.claude_cli_path, options, prompt: user.into(), work_dir, label: "Claude Code", time_limit: PLANNING_TIME_LIMIT }).await
    }
    BrainKind::AgyCli => {
      // agy has no separate system prompt flag, so the instructions lead the prompt.
      let options = vec!["--output-format".into(), "text".into()];
      let prompt = format!("{system}\n\n---\n\n{user}");
      cli::run(CliCall { program: &s.agy_cli_path, options, prompt, work_dir, label: "Antigravity", time_limit: PLANNING_TIME_LIMIT }).await
    }
  }
}

fn brains_in_order(settings: &BrainSettings) -> Vec<BrainKind> {
  let mut order = Vec::new();
  for kind in &settings.order {
    if !order.contains(kind) {
      order.push(*kind);
    }
  }
  order
}

pub fn label(kind: BrainKind) -> &'static str {
  match kind {
    BrainKind::ClaudeCli => "Claude Code",
    BrainKind::AgyCli => "Antigravity",
    BrainKind::ClaudeApi => "Claude API",
    BrainKind::Local => "Local model",
  }
}

fn no_brain_message(skipped: &[String]) -> String {
  if skipped.is_empty() {
    "No brain is turned on. Pick at least one in Settings > Brain.".into()
  } else {
    format!("No brain could answer. {}", skipped.join(" | "))
  }
}

fn planner_message(turns: &[ChatTurn], context: &PlannerContext) -> String {
  let mut engines = Vec::new();
  if context.gemini_ready {
    engines.push("gemini".to_string());
  }
  if context.comfy_ready {
    engines.push("comfy".to_string());
  }
  for name in &context.workflows {
    engines.push(format!("workflow \"{name}\""));
  }
  for editor in &context.editors {
    engines.push(format!("editor app \"{}\" ({})", editor.id, editor.purpose));
  }
  let engines = if engines.is_empty() { "none".to_string() } else { engines.join(", ") };

  let mut images = String::new();
  for image in &context.images {
    let tag = if image.attached { " (attached by the person)" } else { "" };
    let prompt: String = image.prompt.chars().take(160).collect();
    images.push_str(&format!("- {}{tag}: {}\n", image.path, if prompt.is_empty() { "(no prompt saved)" } else { &prompt }));
  }
  if images.is_empty() {
    images.push_str("(none yet)\n");
  }

  let mut transcript = String::new();
  // The CLIs are single-shot, so the conversation so far is replayed as text.
  for turn in turns.iter().rev().take(20).collect::<Vec<_>>().into_iter().rev() {
    let who = if turn.role == "assistant" { "Easel" } else { "Person" };
    transcript.push_str(&format!("{who}: {}\n", turn.text.trim()));
  }

  format!("Available engines: {engines}\n\nImages you can edit:\n{images}\nConversation:\n{transcript}\nReply with the JSON object now.")
}

/// Pulls the JSON object out of a reply, tolerating code fences or chatter around it.
fn parse_plan(text: &str) -> Result<(String, Vec<PlannedJob>), String> {
  let json = extract_json_object(text).ok_or("didn't answer in the expected format")?;
  let value: Value = serde_json::from_str(json).map_err(|_| "answered with broken JSON".to_string())?;
  let reply = value.get("reply").and_then(Value::as_str).unwrap_or_default().trim().to_string();
  let jobs: Vec<PlannedJob> = match value.get("jobs") {
    Some(Value::Array(items)) => items.iter().filter_map(|j| serde_json::from_value(j.clone()).ok()).collect(),
    _ => Vec::new(),
  };
  let jobs: Vec<PlannedJob> = jobs
    .into_iter()
    .filter(|j| !j.prompt.trim().is_empty())
    .map(|mut j| {
      j.engine = j.engine.trim().to_lowercase();
      j.count = Some(j.count.unwrap_or(1).clamp(1, 4));
      j
    })
    .take(6)
    .collect();
  if reply.is_empty() && jobs.is_empty() {
    return Err("gave an empty plan".into());
  }
  Ok((reply, jobs))
}

fn extract_json_object(text: &str) -> Option<&str> {
  let start = text.find('{')?;
  let end = text.rfind('}')?;
  if end > start { Some(&text[start..=end]) } else { None }
}

fn clean_single_answer(text: &str) -> String {
  let trimmed = text.trim().trim_start_matches("```").trim_end_matches("```").trim();
  trimmed.trim_matches('"').trim().to_string()
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn parses_a_plan_inside_code_fences() {
    let text = "Sure!\n```json\n{\"reply\":\"Making two covers.\",\"jobs\":[{\"engine\":\"Gemini\",\"prompt\":\"a red hard hat\",\"count\":9}]}\n```";
    let (reply, jobs) = parse_plan(text).unwrap();
    assert_eq!(reply, "Making two covers.");
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].engine, "gemini");
    assert_eq!(jobs[0].count, Some(4));
  }

  #[test]
  fn a_question_needs_no_jobs() {
    let (reply, jobs) = parse_plan(r#"{"reply":"Which style?","jobs":[]}"#).unwrap();
    assert_eq!(reply, "Which style?");
    assert!(jobs.is_empty());
  }

  #[test]
  fn plain_text_is_rejected_so_the_next_brain_can_try() {
    assert!(parse_plan("I can't help with that").is_err());
    assert!(parse_plan(r#"{"reply":"","jobs":[]}"#).is_err());
  }

  #[test]
  fn duplicate_brains_are_tried_once() {
    let s = BrainSettings { order: vec![BrainKind::Local, BrainKind::ClaudeCli, BrainKind::Local], ..Default::default() };
    assert_eq!(brains_in_order(&s), vec![BrainKind::Local, BrainKind::ClaudeCli]);
  }

  #[test]
  fn context_lists_engines_and_images() {
    let ctx = PlannerContext {
      gemini_ready: true,
      comfy_ready: false,
      workflows: vec!["Wan video".into()],
      editors: vec![ContextEditor { id: "photocraft".into(), purpose: "image editing".into() }],
      images: vec![ContextImage { path: "/p/a.png".into(), prompt: "a barn".into(), attached: true }],
    };
    let turns = vec![ChatTurn { role: "user".into(), text: "make it night".into() }];
    let msg = planner_message(&turns, &ctx);
    assert!(msg.contains("Available engines: gemini, workflow \"Wan video\", editor app \"photocraft\" (image editing)"));
    assert!(msg.contains("- /p/a.png (attached by the person): a barn"));
    assert!(msg.contains("Person: make it night"));
  }

  #[tokio::test]
  async fn falls_through_to_the_next_brain_and_reports_why() {
    let s = BrainSettings {
      order: vec![BrainKind::ClaudeCli, BrainKind::ClaudeApi],
      claude_cli_path: "easel-no-such-claude".into(),
      claude_api_key: String::new(),
      ..Default::default()
    };
    let client = reqwest::Client::new();
    let dir = std::env::temp_dir().join("easel-brain-test");
    let err = improve(&client, &s, &dir, "a cat", false).await.unwrap_err();
    assert!(err.contains("Claude Code: Claude Code isn't installed"), "{err}");
    assert!(err.contains("Claude API: no Claude API key is set"), "{err}");
  }
}
