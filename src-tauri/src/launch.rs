//! Finds the real program behind a name like "claude" or "agy".
//!
//! On Windows, Rust only finds `name.exe` on PATH, so npm installs (which put
//! a `claude.cmd` wrapper on PATH) look missing. Pointing at the `.cmd` is no
//! better: Windows then runs it through cmd.exe, which can't pass a prompt
//! with quotes or line breaks ("batch file arguments are invalid"). So Easel
//! reads the wrapper, finds what it really runs (an .exe, or node plus a .js
//! file) and starts that directly. Nothing ever goes through cmd.exe.

use std::path::{Path, PathBuf};

/// A program to start, plus arguments that go before the caller's own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
  pub program: PathBuf,
  pub prefix_args: Vec<String>,
}

pub fn resolve(program: &str) -> Result<Resolved, String> {
  let path_var = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect::<Vec<_>>()).unwrap_or_default();
  let extra = extra_locations(program);
  resolve_with(program, &path_var, &extra, cfg!(windows))
}

/// The testable core: `path_dirs` stands in for PATH, `extra` for install
/// folders that often aren't on PATH, and `windows` for the platform rules.
pub fn resolve_with(program: &str, path_dirs: &[PathBuf], extra: &[PathBuf], windows: bool) -> Result<Resolved, String> {
  let program = program.trim().trim_matches('"');
  if program.is_empty() {
    return Err("no program is set".into());
  }
  let as_path = Path::new(program);
  let has_dir = as_path.components().count() > 1 || program.contains('\\') || program.contains('/');

  if has_dir {
    if is_wrapper(as_path) {
      return from_wrapper(as_path, path_dirs, windows);
    }
    return Ok(direct(as_path.to_path_buf()));
  }

  if !windows {
    // Unix finds bare names on PATH itself; ~/.local/bin often isn't on it for GUI apps.
    if let Some(found) = path_dirs.iter().chain(extra.iter()).map(|d| d.join(program)).find(|p| p.is_file()) {
      return Ok(direct(found));
    }
    return Ok(direct(PathBuf::from(program)));
  }

  let base = program.trim_end_matches(".exe").trim_end_matches(".cmd").trim_end_matches(".bat");
  let dirs: Vec<&PathBuf> = path_dirs.iter().chain(extra.iter()).collect();
  if let Some(exe) = dirs.iter().map(|d| d.join(format!("{base}.exe"))).find(|p| p.is_file()) {
    return Ok(direct(exe));
  }
  for ext in ["cmd", "bat"] {
    if let Some(wrapper) = dirs.iter().map(|d| d.join(format!("{base}.{ext}"))).find(|p| p.is_file()) {
      return from_wrapper(&wrapper, path_dirs, windows);
    }
  }
  Err(format!("couldn't find {base}.exe or {base}.cmd on PATH"))
}

fn direct(program: PathBuf) -> Resolved {
  Resolved { program, prefix_args: Vec::new() }
}

fn is_wrapper(path: &Path) -> bool {
  matches!(path.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()).as_deref(), Some("cmd" | "bat"))
}

fn from_wrapper(wrapper: &Path, path_dirs: &[PathBuf], windows: bool) -> Result<Resolved, String> {
  let text = std::fs::read_to_string(wrapper).map_err(|e| format!("couldn't read {}: {e}", wrapper.display()))?;
  let dir = wrapper.parent().unwrap_or(Path::new("."));
  let target = wrapper_target(&text).ok_or_else(|| {
    format!("{} isn't an npm wrapper Easel understands; set the path to the real .exe instead", wrapper.display())
  })?;
  let target_path = dir.join(target.replace('\\', std::path::MAIN_SEPARATOR_STR));
  let ext = target_path.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()).unwrap_or_default();
  match ext.as_str() {
    "exe" => Ok(direct(target_path)),
    "js" | "cjs" | "mjs" => {
      let node_name = if windows { "node.exe" } else { "node" };
      let node = Some(dir.join(node_name))
        .filter(|p| p.is_file())
        .or_else(|| path_dirs.iter().map(|d| d.join(node_name)).find(|p| p.is_file()))
        .ok_or("this npm install needs Node.js, which wasn't found on PATH")?;
      Ok(Resolved { program: node, prefix_args: vec![target_path.to_string_lossy().into_owned()] })
    }
    _ => Err(format!("{} runs {}, which Easel can't start directly", wrapper.display(), target_path.display())),
  }
}

/// The file an npm (cmd-shim) wrapper runs: the last quoted path that starts
/// at the wrapper's own folder (`%dp0%\…` or `%~dp0\…`) and isn't node.exe.
pub fn wrapper_target(text: &str) -> Option<String> {
  let mut found = None;
  for marker in ["\"%dp0%\\", "\"%~dp0\\", "\"%~dp0"] {
    let mut rest = text;
    while let Some(start) = rest.find(marker) {
      let after = &rest[start + marker.len()..];
      let Some(end) = after.find('"') else { break };
      let candidate = after[..end].trim_start_matches('\\');
      if !candidate.to_ascii_lowercase().ends_with("node.exe") && !candidate.is_empty() {
        found = Some(candidate.to_string());
      }
      rest = &after[end..];
    }
    if found.is_some() {
      break;
    }
  }
  found
}

/// Where Claude Code and Antigravity often land without being on PATH.
fn extra_locations(_program: &str) -> Vec<PathBuf> {
  let mut dirs = Vec::new();
  if let Some(home) = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")) {
    dirs.push(PathBuf::from(&home).join(".local").join("bin"));
  }
  if let Some(appdata) = std::env::var_os("APPDATA") {
    dirs.push(PathBuf::from(appdata).join("npm"));
  }
  if let Some(local) = std::env::var_os("LOCALAPPDATA") {
    let local = PathBuf::from(local);
    dirs.push(local.join("Programs").join("Antigravity").join("bin"));
    dirs.push(local.join("Programs").join("agy"));
  }
  dirs
}

#[cfg(test)]
mod tests {
  use super::*;

  // What npm's cmd-shim writes for a JavaScript package and for a native binary.
  const JS_SHIM: &str = r#"@ECHO off
GOTO start
:find_dp0
SET dp0=%~dp0
EXIT /b
:start
SETLOCAL
CALL :find_dp0

IF EXIST "%dp0%\node.exe" (
  SET "_prog=%dp0%\node.exe"
) ELSE (
  SET "_prog=node"
  SET PATHEXT=%PATHEXT:;.JS;=;%
)

endLocal & goto #_undefined_# 2>NUL || title %COMSPEC% & "%_prog%"  "%dp0%\node_modules\@anthropic-ai\claude-code\cli.js" %*
"#;
  const EXE_SHIM: &str = "@ECHO off\r\nGOTO start\r\n:start\r\n\"%dp0%\\node_modules\\@anthropic-ai\\claude-code\\bin\\claude.exe\"   %*\r\n";

  #[test]
  fn reads_the_target_out_of_npm_wrappers() {
    assert_eq!(wrapper_target(JS_SHIM).as_deref(), Some(r"node_modules\@anthropic-ai\claude-code\cli.js"));
    assert_eq!(wrapper_target(EXE_SHIM).as_deref(), Some(r"node_modules\@anthropic-ai\claude-code\bin\claude.exe"));
    assert_eq!(wrapper_target("@echo off\r\necho hi"), None);
  }

  #[test]
  fn a_native_exe_wrapper_runs_the_exe_directly() {
    let dir = temp_dir();
    let exe = dir.join("node_modules/@anthropic-ai/claude-code/bin/claude.exe");
    std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
    std::fs::write(&exe, "").unwrap();
    std::fs::write(dir.join("claude.cmd"), EXE_SHIM).unwrap();

    let r = resolve_with("claude", &[dir.clone()], &[], true).unwrap();
    assert_eq!(r.program, exe);
    assert!(r.prefix_args.is_empty());
    let _ = std::fs::remove_dir_all(dir);
  }

  #[test]
  fn a_javascript_wrapper_runs_node_with_the_script() {
    let dir = temp_dir();
    std::fs::write(dir.join("claude.cmd"), JS_SHIM).unwrap();
    std::fs::write(dir.join("node.exe"), "").unwrap();

    // Pointing at the .cmd itself works the same as the bare name.
    let wrapper = dir.join("claude.cmd");
    let r = resolve_with(wrapper.to_str().unwrap(), &[], &[], true).unwrap();
    assert_eq!(r.program, dir.join("node.exe"));
    assert!(r.prefix_args[0].ends_with("cli.js"), "{:?}", r.prefix_args);
    let _ = std::fs::remove_dir_all(dir);
  }

  #[test]
  fn an_exe_on_path_wins_over_a_wrapper() {
    let dir = temp_dir();
    std::fs::write(dir.join("agy.exe"), "").unwrap();
    std::fs::write(dir.join("agy.cmd"), EXE_SHIM).unwrap();
    assert_eq!(resolve_with("agy", &[dir.clone()], &[], true).unwrap().program, dir.join("agy.exe"));
    let _ = std::fs::remove_dir_all(dir);
  }

  #[test]
  fn install_folders_off_path_are_searched() {
    let dir = temp_dir();
    std::fs::write(dir.join("claude.exe"), "").unwrap();
    assert_eq!(resolve_with("claude", &[], &[dir.clone()], true).unwrap().program, dir.join("claude.exe"));
    assert!(resolve_with("claude", &[], &[], true).unwrap_err().contains("claude.exe or claude.cmd"));
    let _ = std::fs::remove_dir_all(dir);
  }

  #[test]
  fn full_paths_to_real_programs_are_used_as_they_are() {
    let r = resolve_with("/usr/local/bin/claude", &[], &[], false).unwrap();
    assert_eq!(r, direct(PathBuf::from("/usr/local/bin/claude")));
  }

  fn temp_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("easel-launch-{}", rand::random::<u32>()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
  }
}
