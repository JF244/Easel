//! The Crafting Apps: free, open-source editors from the ArtCraft team that
//! stand in for the Adobe suite. Easel finds the ones you have installed,
//! opens files in the right one, and hands their command-line tools to the
//! Claude Code brain so it can do real edits.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub struct CraftApp {
  pub id: &'static str,
  pub name: &'static str,
  pub replaces: &'static str,
  pub purpose: &'static str,
  /// File types this app is the first choice for, lowercase without dots.
  pub extensions: &'static [&'static str],
  pub mcp: McpArgs,
}

/// How each app's `<id>-cli mcp` server is told which folders it may use.
#[derive(Clone, Copy)]
pub enum McpArgs {
  /// `mcp`
  Plain,
  /// PhotoCraft: `mcp --automation-read-root DIR --automation-write-root DIR`
  ReadWriteRoots,
  /// LightCraft: `mcp DIR` (the library folder)
  FolderArgument,
}

pub const APPS: &[CraftApp] = &[
  CraftApp {
    id: "photocraft",
    name: "PhotoCraft",
    replaces: "Photoshop",
    purpose: "Image editing: layers, masks, type and real PSD files",
    extensions: &["png", "jpg", "jpeg", "webp", "gif", "psd", "psb", "tif", "tiff", "bmp", "kra", "ora"],
    mcp: McpArgs::ReadWriteRoots,
  },
  CraftApp {
    id: "vectorcraft",
    name: "VectorCraft",
    replaces: "Illustrator",
    purpose: "Vector illustration: logos, icons, badges, SVG and PDF",
    extensions: &["svg", "svgz", "ai", "eps", "vectorcraft"],
    mcp: McpArgs::Plain,
  },
  CraftApp {
    id: "filmcraft",
    name: "FilmCraft",
    replaces: "Premiere Pro",
    purpose: "Video editing, color and sound",
    extensions: &["mp4", "mov", "mkv", "webm", "m4v"],
    mcp: McpArgs::Plain,
  },
  CraftApp {
    id: "lightcraft",
    name: "LightCraft",
    replaces: "Lightroom",
    purpose: "Photo library and raw development",
    extensions: &["dng", "cr2", "cr3", "nef", "arw", "raf", "orf", "rw2"],
    mcp: McpArgs::FolderArgument,
  },
  CraftApp {
    id: "pdfcraft",
    name: "PdfCraft",
    replaces: "Acrobat",
    purpose: "Reading, organizing and protecting PDFs",
    extensions: &["pdf"],
    mcp: McpArgs::Plain,
  },
  CraftApp {
    id: "effectcraft",
    name: "EffectCraft",
    replaces: "After Effects",
    purpose: "Motion graphics and visual effects",
    extensions: &[],
    mcp: McpArgs::Plain,
  },
  CraftApp {
    id: "designcraft",
    name: "DesignCraft",
    replaces: "InDesign",
    purpose: "Page layout and publishing",
    extensions: &["idml"],
    mcp: McpArgs::Plain,
  },
];

/// Paths you set by hand in Settings, which win over detection.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct AppPaths {
  pub gui: String,
  pub cli: String,
}

/// How to start a program: usually just its path, but Flatpak apps go
/// through `flatpak run`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Launcher {
  pub program: PathBuf,
  pub prefix_args: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppStatus {
  pub id: String,
  pub name: String,
  pub replaces: String,
  pub purpose: String,
  pub extensions: Vec<String>,
  pub releases_url: String,
  /// Where the app was found, for display; None when it isn't installed.
  pub gui: Option<String>,
  /// Where its command-line tool was found; the brain needs this to edit.
  pub cli: Option<String>,
}

impl Launcher {
  fn direct(path: PathBuf) -> Self {
    Self { program: path, prefix_args: Vec::new() }
  }

  pub fn describe(&self) -> String {
    let mut parts = vec![self.program.to_string_lossy().into_owned()];
    parts.extend(self.prefix_args.iter().cloned());
    parts.join(" ")
  }

  pub fn command(&self) -> std::process::Command {
    let mut command = std::process::Command::new(&self.program);
    command.args(&self.prefix_args);
    command
  }
}

pub fn get(id: &str) -> Option<&'static CraftApp> {
  APPS.iter().find(|a| a.id == id)
}

pub fn releases_url(app: &CraftApp) -> String {
  format!("https://github.com/storytold/{}/releases/latest", app.id)
}

pub fn status(app: &CraftApp, overrides: &HashMap<String, AppPaths>) -> AppStatus {
  let paths = overrides.get(app.id).cloned().unwrap_or_default();
  AppStatus {
    id: app.id.into(),
    name: app.name.into(),
    replaces: app.replaces.into(),
    purpose: app.purpose.into(),
    extensions: app.extensions.iter().map(|e| e.to_string()).collect(),
    releases_url: releases_url(app),
    gui: find_gui(app, &paths).map(|l| l.describe()),
    cli: find_cli(app, &paths).map(|l| l.describe()),
  }
}

/// Opens the app, optionally with a file, as its own window.
pub fn open(app: &CraftApp, overrides: &HashMap<String, AppPaths>, file: Option<&Path>) -> Result<(), String> {
  let paths = overrides.get(app.id).cloned().unwrap_or_default();
  let launcher = find_gui(app, &paths)
    .ok_or_else(|| format!("{} isn't installed. Get it from {} or set its path on the Apps page.", app.name, releases_url(app)))?;
  let mut command = launcher.command();
  if let Some(file) = file {
    command.arg(file);
  }
  command.spawn().map(|_| ()).map_err(|e| format!("Couldn't start {}: {e}", app.name))
}

/// The MCP server command for an app, limited to `work_dir`.
pub fn mcp_server(app: &CraftApp, overrides: &HashMap<String, AppPaths>, work_dir: &Path) -> Option<Launcher> {
  let paths = overrides.get(app.id).cloned().unwrap_or_default();
  let mut launcher = find_cli(app, &paths)?;
  let dir = work_dir.to_string_lossy().into_owned();
  launcher.prefix_args.push("mcp".into());
  match app.mcp {
    McpArgs::Plain => {}
    McpArgs::ReadWriteRoots => {
      launcher.prefix_args.extend(["--automation-read-root".into(), dir.clone(), "--automation-write-root".into(), dir]);
    }
    McpArgs::FolderArgument => launcher.prefix_args.push(dir),
  }
  Some(launcher)
}

/// Which app opens this file by default.
pub fn app_for(path: &Path) -> Option<&'static CraftApp> {
  let ext = path.extension()?.to_str()?.to_ascii_lowercase();
  APPS.iter().find(|a| a.extensions.contains(&ext.as_str()))
}

fn find_gui(app: &CraftApp, paths: &AppPaths) -> Option<Launcher> {
  if let Some(path) = existing(&paths.gui) {
    return Some(Launcher::direct(path));
  }
  find_program(app.id)
    .or_else(|| installed_copy(app))
    .map(Launcher::direct)
    .or_else(|| flatpak(app, None))
}

fn find_cli(app: &CraftApp, paths: &AppPaths) -> Option<Launcher> {
  let cli_name = format!("{}-cli", app.id);
  if let Some(path) = existing(&paths.cli) {
    return Some(Launcher::direct(path));
  }
  // The CLI ships next to the app in every installer, so look there first.
  let beside_gui = existing(&paths.gui)
    .or_else(|| installed_copy(app))
    .and_then(|gui| gui.parent().map(|dir| dir.join(exe(&cli_name))))
    .filter(|p| p.is_file());
  beside_gui
    .or_else(|| find_program(&cli_name))
    .map(Launcher::direct)
    .or_else(|| flatpak(app, Some(&cli_name)))
}

fn existing(path: &str) -> Option<PathBuf> {
  let path = path.trim();
  if path.is_empty() {
    return None;
  }
  let p = PathBuf::from(path);
  p.is_file().then_some(p)
}

/// Looks for `name` on PATH, the way a terminal would.
fn find_program(name: &str) -> Option<PathBuf> {
  let path_var = std::env::var_os("PATH")?;
  std::env::split_paths(&path_var).map(|dir| dir.join(exe(name))).find(|p| p.is_file())
}

/// Install locations that usually aren't on PATH: the Windows installer's
/// folder, ~/.local/bin, /opt, and AppImages kept in ~/Applications or Downloads.
fn installed_copy(app: &CraftApp) -> Option<PathBuf> {
  let mut candidates: Vec<PathBuf> = Vec::new();
  if cfg!(windows) {
    for var in ["ProgramFiles", "ProgramW6432", "LOCALAPPDATA"] {
      if let Some(base) = std::env::var_os(var) {
        let base = PathBuf::from(base);
        candidates.push(base.join(app.name).join(exe(app.id)));
        candidates.push(base.join("Programs").join(app.name).join(exe(app.id)));
      }
    }
  } else {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    if let Some(home) = &home {
      candidates.push(home.join(".local/bin").join(app.id));
    }
    candidates.push(PathBuf::from("/opt").join(app.id).join(app.id));
    candidates.push(PathBuf::from("/usr/local/bin").join(app.id));
    candidates.push(PathBuf::from("/usr/bin").join(app.id));
    if let Some(found) = candidates.iter().find(|p| p.is_file()) {
      return Some(found.clone());
    }
    if let Some(home) = &home {
      for dir in ["Applications", "Downloads", "AppImages", ".local/bin"] {
        if let Some(image) = newest_appimage(&home.join(dir), app.id) {
          return Some(image);
        }
      }
    }
    return None;
  }
  candidates.into_iter().find(|p| p.is_file())
}

/// e.g. ~/Applications/photocraft-0.9.2-linux-x86_64.AppImage
fn newest_appimage(dir: &Path, id: &str) -> Option<PathBuf> {
  let mut found: Vec<PathBuf> = std::fs::read_dir(dir)
    .ok()?
    .flatten()
    .map(|e| e.path())
    .filter(|p| {
      let name = p.file_name().and_then(|n| n.to_str()).unwrap_or_default().to_ascii_lowercase();
      name.starts_with(id) && name.ends_with(".appimage") && !name.starts_with(&format!("{id}-cli"))
    })
    .collect();
  found.sort();
  found.pop()
}

/// Flatpak installs: `flatpak run ai.storyteller.<id>` (or `--command=<id>-cli`).
fn flatpak(app: &CraftApp, command: Option<&str>) -> Option<Launcher> {
  if cfg!(windows) {
    return None;
  }
  let flatpak_id = format!("ai.storyteller.{}", app.id);
  let home = std::env::var_os("HOME").map(PathBuf::from);
  let installed = [home.map(|h| h.join(".local/share/flatpak/app")), Some(PathBuf::from("/var/lib/flatpak/app"))]
    .into_iter()
    .flatten()
    .any(|dir| dir.join(&flatpak_id).is_dir());
  let flatpak = find_program("flatpak")?;
  if !installed {
    return None;
  }
  let mut prefix_args = vec!["run".to_string()];
  if let Some(command) = command {
    prefix_args.push(format!("--command={command}"));
  }
  prefix_args.push(flatpak_id);
  Some(Launcher { program: flatpak, prefix_args })
}

fn exe(name: &str) -> String {
  if cfg!(windows) { format!("{name}.exe") } else { name.to_string() }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn files_go_to_the_right_app() {
    assert_eq!(app_for(Path::new("/x/cover.PNG")).unwrap().id, "photocraft");
    assert_eq!(app_for(Path::new("/x/logo.svg")).unwrap().id, "vectorcraft");
    assert_eq!(app_for(Path::new("/x/clip.mov")).unwrap().id, "filmcraft");
    assert_eq!(app_for(Path::new("/x/IMG_1.CR3")).unwrap().id, "lightcraft");
    assert_eq!(app_for(Path::new("/x/manual.pdf")).unwrap().id, "pdfcraft");
    assert!(app_for(Path::new("/x/notes.txt")).is_none());
  }

  #[test]
  fn every_app_has_a_unique_id() {
    let mut ids: Vec<&str> = APPS.iter().map(|a| a.id).collect();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), APPS.len());
  }

  #[cfg(unix)]
  #[test]
  fn hand_set_paths_win_and_the_cli_is_found_beside_the_app() {
    let dir = temp_dir();
    let gui = dir.join("photocraft");
    let cli = dir.join("photocraft-cli");
    std::fs::write(&gui, "").unwrap();
    std::fs::write(&cli, "").unwrap();
    let mut overrides = HashMap::new();
    overrides.insert("photocraft".to_string(), AppPaths { gui: gui.to_string_lossy().into(), cli: String::new() });

    let s = status(get("photocraft").unwrap(), &overrides);
    assert_eq!(s.gui.as_deref(), Some(gui.to_str().unwrap()));
    assert_eq!(s.cli.as_deref(), Some(cli.to_str().unwrap()));
    let _ = std::fs::remove_dir_all(dir);
  }

  #[cfg(unix)]
  #[test]
  fn photocraft_mcp_is_limited_to_the_work_folder() {
    let dir = temp_dir();
    let cli = dir.join("photocraft-cli");
    std::fs::write(&cli, "").unwrap();
    let mut overrides = HashMap::new();
    overrides.insert("photocraft".to_string(), AppPaths { gui: String::new(), cli: cli.to_string_lossy().into() });

    let server = mcp_server(get("photocraft").unwrap(), &overrides, Path::new("/work")).unwrap();
    assert_eq!(server.program, cli);
    assert_eq!(server.prefix_args, vec!["mcp", "--automation-read-root", "/work", "--automation-write-root", "/work"]);
    let _ = std::fs::remove_dir_all(dir);
  }

  #[test]
  fn finds_the_newest_appimage() {
    let dir = temp_dir();
    for name in ["photocraft-0.9.1-linux-x86_64.AppImage", "photocraft-0.9.2-linux-x86_64.AppImage", "photocraft-cli-0.9.2.AppImage", "other.AppImage"] {
      std::fs::write(dir.join(name), "").unwrap();
    }
    let found = newest_appimage(&dir, "photocraft").unwrap();
    assert!(found.ends_with("photocraft-0.9.2-linux-x86_64.AppImage"));
    let _ = std::fs::remove_dir_all(dir);
  }

  fn temp_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("easel-apps-{}", rand::random::<u32>()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
  }
}
