//! Agent CLIs in print mode, so they run on your subscriptions instead of API
//! keys: Claude Code (`claude -p`) and Google Antigravity (`agy -p`).
//!
//! The program is started directly (never through a shell), so nothing in a
//! prompt can be read as a command. It runs in an empty scratch folder so the
//! agent can't wander through your files, and it's stopped after a time limit.

use std::path::Path;
use std::process::Stdio;
use std::time::Duration;
use tokio::process::Command;

/// Long enough for a brain to plan; editor jobs pass their own, longer limit.
pub const PLANNING_TIME_LIMIT: Duration = Duration::from_secs(300);
const OUT_OF_USAGE_HINTS: &[&str] = &["usage limit", "rate limit", "quota", "limit reached", "credit balance", "hit your"];

pub struct CliCall<'a> {
  /// Program name or full path, e.g. "claude" or "/usr/bin/agy".
  pub program: &'a str,
  /// Options placed before `-p`. Both CLIs read the argument after `-p` as the prompt.
  pub options: Vec<String>,
  pub prompt: String,
  pub work_dir: &'a Path,
  /// Shown in error messages, e.g. "Claude Code".
  pub label: &'a str,
  pub time_limit: Duration,
}

pub async fn run(call: CliCall<'_>) -> Result<String, String> {
  let program = call.program.trim();
  if program.is_empty() {
    return Err(format!("no {} program is set", call.label));
  }
  std::fs::create_dir_all(call.work_dir).map_err(|e| e.to_string())?;

  let mut command = Command::new(program);
  command
    .args(&call.options)
    .arg("-p")
    .arg(&call.prompt)
    .current_dir(call.work_dir)
    .stdin(Stdio::null())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .kill_on_drop(true);
  hide_console_window(&mut command);

  let child = command.spawn().map_err(|e| {
    if e.kind() == std::io::ErrorKind::NotFound {
      format!("{} isn't installed or isn't on your PATH (looked for \"{program}\"); set its full path in Settings", call.label)
    } else {
      format!("couldn't start {}: {e}", call.label)
    }
  })?;

  let output = match tokio::time::timeout(call.time_limit, child.wait_with_output()).await {
    Ok(result) => result.map_err(|e| format!("{} stopped unexpectedly: {e}", call.label))?,
    Err(_) => return Err(format!("{} didn't finish within {} minutes", call.label, call.time_limit.as_secs() / 60)),
  };
  let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
  let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();

  if !output.status.success() {
    return Err(describe_failure(call.label, &stdout, &stderr));
  }
  if stdout.is_empty() {
    return Err(format!("{} printed nothing{}", call.label, tail(&stderr)));
  }
  if looks_out_of_usage(&stdout) && !stdout.contains('{') {
    return Err(format!("{} is out of usage: {}", call.label, first_line(&stdout)));
  }
  Ok(stdout)
}

fn describe_failure(label: &str, stdout: &str, stderr: &str) -> String {
  let detail = if !stderr.is_empty() { stderr } else { stdout };
  if looks_out_of_usage(detail) {
    format!("{label} is out of usage: {}", first_line(detail))
  } else {
    format!("{label} failed{}", tail(detail))
  }
}

fn looks_out_of_usage(text: &str) -> bool {
  let lower = text.to_lowercase();
  OUT_OF_USAGE_HINTS.iter().any(|hint| lower.contains(hint))
}

fn first_line(text: &str) -> String {
  text.lines().next().unwrap_or_default().chars().take(200).collect()
}

fn tail(text: &str) -> String {
  if text.is_empty() {
    return String::new();
  }
  let last: String = text.lines().rev().take(3).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join(" ");
  format!(": {}", last.chars().take(300).collect::<String>())
}

#[cfg(windows)]
fn hide_console_window(command: &mut Command) {
  // Without this, every brain call flashes a terminal window on Windows.
  const CREATE_NO_WINDOW: u32 = 0x0800_0000;
  command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn hide_console_window(_command: &mut Command) {}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn spots_usage_limits() {
    assert!(looks_out_of_usage("Claude AI usage limit reached|1760000000"));
    assert!(looks_out_of_usage("Error: Quota exceeded for model"));
    assert!(looks_out_of_usage("You've hit your session limit · resets 3:45pm"));
    assert!(!looks_out_of_usage("{\"reply\":\"done\"}"));
  }

  #[tokio::test]
  async fn missing_program_gives_a_clear_message() {
    let dir = std::env::temp_dir().join("easel-cli-test");
    let err = run(CliCall {
      program: "easel-no-such-program",
      options: vec![],
      prompt: "hi".into(),
      work_dir: &dir,
      label: "Test CLI",
      time_limit: PLANNING_TIME_LIMIT,
    })
    .await
    .unwrap_err();
    assert!(err.contains("isn't installed"), "{err}");
  }

  #[cfg(unix)]
  #[tokio::test]
  async fn prompt_is_passed_as_one_argument_without_a_shell() {
    let dir = std::env::temp_dir().join("easel-cli-test");
    // `echo -p <prompt>` prints the prompt back untouched, quotes and all.
    let out = run(CliCall {
      program: "echo",
      options: vec![],
      prompt: "a \"quoted\" prompt; $(not run)".into(),
      work_dir: &dir,
      label: "echo",
      time_limit: PLANNING_TIME_LIMIT,
    })
    .await
    .unwrap();
    assert_eq!(out, "-p a \"quoted\" prompt; $(not run)");
  }
}
