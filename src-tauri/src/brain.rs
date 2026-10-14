//! The brain plans jobs and writes prompts. It tries each brain in your chosen
//! order: when one is missing, out of usage, or answers with something Easel
//! can't use, the next one gets the same request, and the reply says which
//! brain answered and why the earlier ones were skipped.

use crate::providers::cli::{self, CliCall, PLANNING_TIME_LIMIT};
use crate::providers::{claude, gemini, local_llm};
use crate::settings::{AdvisorMode, BrainKind, BrainSettings};
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

/// Added to the planner's instructions when the advisor can be asked automatically.
const ASK_ADVISOR_RULE: &str = r#"
- If the request is complex or you're unsure how to plan it well, you may instead reply with
  {"reply": "", "jobs": [], "askAdvisor": "one precise question for a stronger model"}.
  You'll get its advice and then plan again. Don't ask for simple requests."#;

const ADVISOR_SYSTEM: &str = "You advise a smaller model that plans image jobs inside Easel, a desktop image app. \
It can generate pictures with Gemini or ComfyUI and make exact edits with editor apps. Answer its question with \
concrete guidance: which engine or app to use, how to word the prompts, and the steps in order. \
Plain text, under 200 words.";

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
  /// The frontier brain that advised, when one did.
  pub advisor: Option<String>,
  /// What the advisor said, shown under the reply.
  pub advice: Option<String>,
}

struct ParsedPlan {
  reply: String,
  jobs: Vec<PlannedJob>,
  ask_advisor: Option<String>,
}

/// Messages starting with this go straight to the advisor.
const ADVISOR_COMMAND: &str = "/advisor";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Improved {
  pub prompt: String,
  pub brain: String,
  pub skipped: Vec<String>,
}

pub async fn plan(client: &reqwest::Client, settings: &BrainSettings, work_dir: &Path, turns: &[ChatTurn], context: &PlannerContext) -> Result<Plan, String> {
  // "/advisor make me a…" sends this turn to the frontier advisor directly.
  if let Some(request) = turns.last().filter(|t| t.role == "user").and_then(|t| strip_advisor_command(&t.text)) {
    if settings.advisor_mode == AdvisorMode::Off || settings.advisor_order.is_empty() {
      return Err("The advisor is turned off. Choose advisor brains in Settings > Brain.".into());
    }
    let mut turns = turns.to_vec();
    if let Some(last) = turns.last_mut() {
      last.text = request;
    }
    let user = planner_message(&turns, context);
    let (mut plan, _) = plan_with(client, settings, work_dir, &settings.advisor_order, PLANNER_SYSTEM, &user).await?;
    plan.advisor = Some(plan.brain.clone());
    return Ok(plan);
  }

  let auto = settings.advisor_mode == AdvisorMode::Auto && !settings.advisor_order.is_empty();
  let system = if auto { format!("{PLANNER_SYSTEM}{ASK_ADVISOR_RULE}") } else { PLANNER_SYSTEM.to_string() };
  let user = planner_message(turns, context);
  let (plan, parsed) = plan_with(client, settings, work_dir, &settings.order, &system, &user).await?;
  let question = match parsed.ask_advisor {
    Some(q) if auto => q,
    _ => return Ok(plan),
  };

  // The primary brain asked for help: get advice, then let the same brain plan again.
  let primary = plan.brain.clone();
  let mut skipped = plan.skipped.clone();
  let advisor_question = format!("{user}\n\nThe planner's question: {question}");
  let (advice, advisor) = match first_answer(client, settings, work_dir, &settings.advisor_order, ADVISOR_SYSTEM, &advisor_question).await {
    Ok((text, name, mut also_skipped)) => {
      skipped.append(&mut also_skipped);
      (Some(text), Some(name))
    }
    Err(err) => {
      skipped.push(format!("advisor: {err}"));
      (None, None)
    }
  };
  let follow_up = match (&advice, &advisor) {
    (Some(text), Some(name)) => format!("{user}\n\nAdvice from {name}: {text}\n\nNow reply with the JSON plan; don't ask again."),
    _ => format!("{user}\n\nNo advisor was available. Decide yourself and reply with the JSON plan."),
  };
  let kind = settings.order.iter().find(|k| label(settings, k) == primary).cloned();
  let order: Vec<BrainKind> = kind.into_iter().chain(settings.order.iter().cloned()).collect();
  let (mut final_plan, _) = plan_with(client, settings, work_dir, &order, PLANNER_SYSTEM, &follow_up).await?;
  skipped.append(&mut final_plan.skipped);
  final_plan.skipped = skipped;
  final_plan.advisor = advisor;
  final_plan.advice = advice;
  Ok(final_plan)
}

/// Tries brains in order until one returns a usable plan.
async fn plan_with(
  client: &reqwest::Client,
  settings: &BrainSettings,
  work_dir: &Path,
  order: &[BrainKind],
  system: &str,
  user: &str,
) -> Result<(Plan, ParsedPlan), String> {
  let mut skipped = Vec::new();
  for kind in dedup(order) {
    let name = label(settings, &kind);
    match ask(client, settings, work_dir, &kind, system, user).await.and_then(|text| parse_plan(&text)) {
      Ok(parsed) => {
        let plan = Plan {
          reply: parsed.reply.clone(),
          jobs: parsed.jobs.clone(),
          brain: name,
          skipped,
          advisor: None,
          advice: None,
        };
        return Ok((plan, parsed));
      }
      Err(err) => skipped.push(format!("{name}: {err}")),
    }
  }
  Err(no_brain_message(&skipped))
}

/// Tries brains in order until one gives any non-empty text answer.
async fn first_answer(
  client: &reqwest::Client,
  settings: &BrainSettings,
  work_dir: &Path,
  order: &[BrainKind],
  system: &str,
  user: &str,
) -> Result<(String, String, Vec<String>), String> {
  let mut skipped = Vec::new();
  for kind in dedup(order) {
    let name = label(settings, &kind);
    match ask(client, settings, work_dir, &kind, system, user).await {
      Ok(text) if !clean_single_answer(&text).is_empty() => return Ok((clean_single_answer(&text), name, skipped)),
      Ok(_) => skipped.push(format!("{name}: empty answer")),
      Err(err) => skipped.push(format!("{name}: {err}")),
    }
  }
  Err(no_brain_message(&skipped))
}

fn strip_advisor_command(text: &str) -> Option<String> {
  let trimmed = text.trim_start();
  let rest = trimmed.strip_prefix(ADVISOR_COMMAND)?;
  // "/advisors" or "/advisorX" isn't the command.
  if rest.chars().next().is_some_and(|c| !c.is_whitespace()) {
    return None;
  }
  Some(rest.trim().to_string())
}

pub async fn improve(client: &reqwest::Client, settings: &BrainSettings, work_dir: &Path, prompt: &str, editing: bool) -> Result<Improved, String> {
  if prompt.trim().is_empty() {
    return Err("Type a prompt first, then the brain can improve it.".into());
  }
  let system = if editing { IMPROVE_EDIT_SYSTEM } else { IMPROVE_SYSTEM };
  let (prompt, brain, skipped) = first_answer(client, settings, work_dir, &settings.order, system, prompt.trim()).await?;
  Ok(Improved { prompt, brain, skipped })
}

/// A quick round trip through one brain, for the Test button in Settings.
pub async fn test(client: &reqwest::Client, settings: &BrainSettings, work_dir: &Path, kind: &BrainKind) -> Result<String, String> {
  let started = std::time::Instant::now();
  let answer = ask(client, settings, work_dir, kind, "Reply with exactly one word.", "Say ready.").await?;
  let first: String = clean_single_answer(&answer).chars().take(60).collect();
  Ok(format!("{} answered \"{first}\" in {:.1}s", label(settings, kind), started.elapsed().as_secs_f32()))
}

/// Sends one question to one brain.
async fn ask(client: &reqwest::Client, s: &BrainSettings, work_dir: &Path, kind: &BrainKind, system: &str, user: &str) -> Result<String, String> {
  match kind {
    BrainKind::ClaudeApi => claude::complete(client, &s.claude_api_key, &s.claude_api_model, system, user).await,
    BrainKind::Gemini => gemini::complete(client, &s.gemini_api_key, &s.gemini_model, system, user).await,
    BrainKind::Local => local_llm::complete_local(client, &s.local_url, &s.local_model, system, user).await,
    BrainKind::Remote(id) => {
      let remote = s.remotes.iter().find(|r| &r.id == id).ok_or("this remote server was removed in Settings")?;
      local_llm::complete(client, &remote.url, &remote.api_key, &remote.model, system, user).await
    }
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

fn dedup(order: &[BrainKind]) -> Vec<BrainKind> {
  let mut out: Vec<BrainKind> = Vec::new();
  for kind in order {
    if !out.contains(kind) {
      out.push(kind.clone());
    }
  }
  out
}

pub fn label(settings: &BrainSettings, kind: &BrainKind) -> String {
  match kind {
    BrainKind::ClaudeCli => "Claude Code".into(),
    BrainKind::AgyCli => "Antigravity".into(),
    BrainKind::ClaudeApi => "Claude API".into(),
    BrainKind::Gemini => "Gemini".into(),
    BrainKind::Local => "Local model".into(),
    BrainKind::Remote(id) => settings
      .remotes
      .iter()
      .find(|r| &r.id == id)
      .map(|r| if r.name.trim().is_empty() { "Remote server".to_string() } else { r.name.trim().to_string() })
      .unwrap_or_else(|| "Remote server".into()),
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
fn parse_plan(text: &str) -> Result<ParsedPlan, String> {
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
  let ask_advisor = value.get("askAdvisor").and_then(Value::as_str).map(str::trim).filter(|q| !q.is_empty()).map(str::to_string);
  if reply.is_empty() && jobs.is_empty() && ask_advisor.is_none() {
    return Err("gave an empty plan".into());
  }
  Ok(ParsedPlan { reply, jobs, ask_advisor })
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
    let ParsedPlan { reply, jobs, .. } = parse_plan(text).unwrap();
    assert_eq!(reply, "Making two covers.");
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].engine, "gemini");
    assert_eq!(jobs[0].count, Some(4));
  }

  #[test]
  fn a_question_needs_no_jobs() {
    let ParsedPlan { reply, jobs, .. } = parse_plan(r#"{"reply":"Which style?","jobs":[]}"#).unwrap();
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
    let order = vec![BrainKind::Local, BrainKind::ClaudeCli, BrainKind::Local];
    assert_eq!(dedup(&order), vec![BrainKind::Local, BrainKind::ClaudeCli]);
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

  #[test]
  fn advisor_command_is_recognised_only_on_its_own() {
    assert_eq!(strip_advisor_command("/advisor plan a 3-part series").as_deref(), Some("plan a 3-part series"));
    assert_eq!(strip_advisor_command("  /advisor").as_deref(), Some(""));
    assert!(strip_advisor_command("/advisors are great").is_none());
    assert!(strip_advisor_command("ask the /advisor").is_none());
  }

  #[test]
  fn a_plan_may_be_just_a_question_for_the_advisor() {
    let parsed = parse_plan(r#"{"reply":"","jobs":[],"askAdvisor":"Which engine keeps exact text?"}"#).unwrap();
    assert_eq!(parsed.ask_advisor.as_deref(), Some("Which engine keeps exact text?"));
  }

  #[tokio::test]
  async fn auto_mode_gets_advice_then_the_primary_plans_with_it() {
    let base = stand_in_server();
    let s = BrainSettings {
      order: vec![BrainKind::Remote("p".into())],
      advisor_order: vec![BrainKind::Remote("a".into())],
      advisor_mode: AdvisorMode::Auto,
      remotes: vec![remote("p", "LAN box", &base, "primary"), remote("a", "Gemini 4", &base, "advisor")],
      ..Default::default()
    };
    let turns = vec![ChatTurn { role: "user".into(), text: "a poster with exact text".into() }];
    let plan = plan(&reqwest::Client::new(), &s, Path::new("."), &turns, &PlannerContext::default()).await.unwrap();
    assert_eq!(plan.brain, "LAN box");
    assert_eq!(plan.advisor.as_deref(), Some("Gemini 4"));
    assert_eq!(plan.advice.as_deref(), Some("Use the editor for exact text."));
    assert_eq!(plan.jobs.len(), 1);
    assert_eq!(plan.jobs[0].prompt, "poster, planned with advice");
  }

  #[tokio::test]
  async fn advisor_command_goes_straight_to_the_advisor() {
    let base = stand_in_server();
    let s = BrainSettings {
      order: vec![BrainKind::Remote("p".into())],
      advisor_order: vec![BrainKind::Remote("a".into())],
      advisor_mode: AdvisorMode::OnRequest,
      remotes: vec![remote("p", "LAN box", &base, "primary"), remote("a", "Gemini 4", &base, "advisor")],
      ..Default::default()
    };
    let turns = vec![ChatTurn { role: "user".into(), text: "/advisor plan a course cover".into() }];
    let plan = plan(&reqwest::Client::new(), &s, Path::new("."), &turns, &PlannerContext::default()).await.unwrap();
    assert_eq!(plan.brain, "Gemini 4");
    assert_eq!(plan.advisor.as_deref(), Some("Gemini 4"));
    assert_eq!(plan.reply, "Advisor planned it.");
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

  fn remote(id: &str, name: &str, url: &str, model: &str) -> crate::settings::RemoteBrain {
    crate::settings::RemoteBrain { id: id.into(), name: name.into(), url: url.into(), api_key: "test-key".into(), model: model.into() }
  }

  /// A tiny OpenAI-compatible server. The "primary" model first asks for advice,
  /// then plans once the advice is in its prompt; the "advisor" model answers
  /// questions, or plans when asked directly.
  fn stand_in_server() -> String {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
      for stream in listener.incoming().flatten() {
        let mut stream = stream;
        let mut buf = Vec::new();
        let mut chunk = [0u8; 8192];
        // Read headers, then the body by Content-Length.
        let body = loop {
          let n = stream.read(&mut chunk).unwrap_or(0);
          if n == 0 { break String::new(); }
          buf.extend_from_slice(&chunk[..n]);
          let text = String::from_utf8_lossy(&buf).to_string();
          if let Some(end) = text.find("\r\n\r\n") {
            let len = text.lines().find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse::<usize>().unwrap_or(0))).unwrap_or(0);
            if buf.len() >= end + 4 + len { break text[end + 4..].to_string(); }
          }
        };
        let request: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
        let model = request["model"].as_str().unwrap_or_default();
        let system = request["messages"][0]["content"].as_str().unwrap_or_default();
        let user = request["messages"][1]["content"].as_str().unwrap_or_default();
        let answer = match model {
          "advisor" if system.starts_with("You advise") => "Use the editor for exact text.".to_string(),
          "advisor" => r#"{"reply":"Advisor planned it.","jobs":[]}"#.to_string(),
          _ if user.contains("Advice from Gemini 4: Use the editor for exact text.") => {
            r#"{"reply":"Done.","jobs":[{"engine":"gemini","prompt":"poster, planned with advice"}]}"#.to_string()
          }
          _ => r#"{"reply":"","jobs":[],"askAdvisor":"How do I keep text exact?"}"#.to_string(),
        };
        let payload = serde_json::json!({ "choices": [{ "message": { "content": answer } }] }).to_string();
        let _ = write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", payload.len(), payload);
      }
    });
    format!("http://{addr}")
  }
}
