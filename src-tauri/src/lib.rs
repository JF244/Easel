mod apps;
mod brain;
mod editor;
mod gallery;
mod launch;
mod preflight;
mod providers;
mod settings;

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use apps::AppStatus;
use brain::{ChatTurn, Improved, Plan, PlannerContext};
use editor::{EditorJob, EditorResult};
use gallery::{GalleryItem, MediaMeta};
use providers::{comfy, gemini, local_llm, GenerateRequest, Provider};
use settings::Settings;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;
use tauri::{Manager, State};

/// Bigger than any image a person would edit, small enough to keep memory sane.
const MAX_INPUT_BYTES: u64 = 40 * 1024 * 1024;

struct AppState {
  settings: Mutex<Settings>,
  settings_path: PathBuf,
  pictures_dir: Option<PathBuf>,
  /// Empty folder the agent CLIs run in, so they can't browse your files.
  brain_dir: PathBuf,
  /// Private folders where editor jobs work on copies.
  jobs_dir: PathBuf,
  client: reqwest::Client,
}

impl AppState {
  fn settings(&self) -> Settings {
    self.settings.lock().map(|s| s.clone()).unwrap_or_default()
  }

  /// Brain settings with the Gemini key copied in for the Gemini brain.
  fn brain_settings(&self) -> settings::BrainSettings {
    let settings = self.settings();
    let mut brain = settings.brain;
    brain.gemini_api_key = settings.gemini.api_key;
    brain
  }

  fn output_dir(&self) -> PathBuf {
    self.settings().output_dir(self.pictures_dir.clone())
  }
}

#[tauri::command]
fn get_settings(state: State<AppState>) -> Settings {
  state.settings()
}

#[tauri::command]
fn save_settings(app: tauri::AppHandle, state: State<AppState>, settings: Settings) -> Result<Settings, String> {
  settings.save(&state.settings_path)?;
  *state.settings.lock().map_err(|_| "Settings are busy, try again.")? = settings.clone();
  allow_output_dir(&app, &settings.output_dir(state.pictures_dir.clone()));
  Ok(settings)
}

#[tauri::command]
fn output_dir(state: State<AppState>) -> String {
  state.output_dir().to_string_lossy().into_owned()
}

#[tauri::command]
fn gallery_list(state: State<AppState>) -> Vec<GalleryItem> {
  gallery::list(&state.output_dir())
}

#[tauri::command]
fn gallery_delete(state: State<AppState>, path: String) -> Result<(), String> {
  gallery::delete(&state.output_dir(), Path::new(&path))
}

/// Reads an image from disk as a data URL so it can be edited or masked.
#[tauri::command]
fn read_image(path: String) -> Result<String, String> {
  image_data_url(Path::new(&path))
}

#[tauri::command]
async fn generate(state: State<'_, AppState>, mut request: GenerateRequest) -> Result<Vec<GalleryItem>, String> {
  let settings = state.settings();
  for path in std::mem::take(&mut request.image_paths) {
    request.images.push(image_data_url(Path::new(&path))?);
  }

  let (outputs, model) = match request.provider {
    Provider::Gemini => {
      let model = request.model.clone().filter(|m| !m.trim().is_empty()).unwrap_or(settings.gemini.image_model.clone());
      (gemini::generate(&state.client, &settings.gemini.api_key, &model, &request).await?, model)
    }
    Provider::Comfy => {
      let model = request.model.clone().filter(|m| !m.trim().is_empty()).unwrap_or(settings.comfy.checkpoint.clone());
      (comfy::generate(&state.client, &settings.comfy, &request).await?, model)
    }
    Provider::ComfyWorkflow => {
      let id = request.workflow_id.clone().unwrap_or_default();
      let workflow = settings
        .comfy
        .workflows
        .iter()
        .find(|w| w.id == id || w.name == id)
        .ok_or("That ComfyUI workflow isn't saved anymore. Check Settings > ComfyUI.")?;
      (comfy::run_workflow(&state.client, &settings.comfy, workflow, &request).await?, workflow.name.clone())
    }
  };

  let output_dir = state.output_dir();
  let mut saved = Vec::new();
  for output in outputs {
    let meta = MediaMeta {
      prompt: request.prompt.trim().to_string(),
      provider: provider_name(request.provider).into(),
      model: model.clone(),
      mode: request.mode().into(),
      seed: output.seed,
      note: output.note.clone(),
      ..Default::default()
    };
    saved.push(gallery::save_result(&output_dir, &output.bytes, &output.extension, &meta)?);
  }
  Ok(saved)
}

#[tauri::command]
async fn brain_plan(state: State<'_, AppState>, turns: Vec<ChatTurn>, context: PlannerContext) -> Result<Plan, String> {
  brain::plan(&state.client, &state.brain_settings(), &state.brain_dir, &turns, &context).await
}

#[tauri::command]
async fn brain_improve(state: State<'_, AppState>, prompt: String, editing: bool) -> Result<Improved, String> {
  brain::improve(&state.client, &state.brain_settings(), &state.brain_dir, &prompt, editing).await
}

#[tauri::command]
async fn gemini_models(state: State<'_, AppState>, api_key: String) -> Result<Vec<String>, String> {
  gemini::list_image_models(&state.client, &api_key).await
}

#[tauri::command]
async fn comfy_checkpoints(state: State<'_, AppState>, url: String) -> Result<Vec<String>, String> {
  let mut comfy = state.settings().comfy;
  comfy.url = url;
  comfy::list_checkpoints(&state.client, &comfy).await
}

#[tauri::command]
async fn local_models(state: State<'_, AppState>, url: String, api_key: Option<String>) -> Result<Vec<String>, String> {
  local_llm::list_models(&state.client, &url, api_key.as_deref().unwrap_or_default()).await
}

/// Warnings about this GPU and ComfyUI's PyTorch, checked before a ComfyUI job.
#[tauri::command]
async fn comfy_preflight(state: State<'_, AppState>) -> Result<preflight::Preflight, String> {
  let comfy_url = state.settings().comfy.url;
  let profile = hardware_profile_path();
  Ok(preflight::check(&state.client, &comfy_url, profile.as_deref()).await)
}

/// Runs one quick question through a single brain and reports what happened.
#[tauri::command]
async fn brain_test(state: State<'_, AppState>, kind: settings::BrainKind) -> Result<String, String> {
  brain::test(&state.client, &state.brain_settings(), &state.brain_dir, &kind).await
}

/// Where the setup's hardware detector writes its profile.
fn hardware_profile_path() -> Option<PathBuf> {
  let base = if cfg!(windows) {
    std::env::var_os("LOCALAPPDATA").map(|d| PathBuf::from(d).join("EaselSuite"))
  } else {
    std::env::var_os("XDG_DATA_HOME")
      .map(PathBuf::from)
      .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
      .map(|d| d.join("easel-suite"))
  };
  base.map(|b| b.join("hardware-profile.json")).filter(|p| p.is_file())
}

/// Which local model servers (Ollama, LM Studio, llama-server) are running now.
#[tauri::command]
async fn detect_local_servers(state: State<'_, AppState>) -> Result<Vec<local_llm::LocalServer>, String> {
  Ok(local_llm::detect_local_servers(&state.client).await)
}

#[tauri::command]
async fn gemini_text_models(state: State<'_, AppState>, api_key: String) -> Result<Vec<String>, String> {
  gemini::list_text_models(&state.client, &api_key).await
}

#[tauri::command]
fn apps_status(state: State<AppState>) -> Vec<AppStatus> {
  let overrides = state.settings().apps;
  apps::APPS.iter().map(|app| apps::status(app, &overrides)).collect()
}

/// Opens a Craft app, with a file if one is given.
#[tauri::command]
fn open_with_app(state: State<AppState>, app_id: String, path: Option<String>) -> Result<(), String> {
  let app = apps::get(&app_id).ok_or("Easel doesn't know that app.")?;
  if let Some(path) = &path {
    if !gallery::is_media(Path::new(path)) {
      return Err("That isn't a file the Craft apps open.".into());
    }
  }
  apps::open(app, &state.settings().apps, path.as_deref().map(Path::new))
}

/// Opens a file in the Craft app made for it, or in your other editor.
#[tauri::command]
fn open_in_editor(state: State<AppState>, path: String) -> Result<(), String> {
  let file = Path::new(&path);
  if !gallery::is_media(file) {
    return Err("That isn't an image, video or document Easel knows.".into());
  }
  let settings = state.settings();
  if let Some(app) = apps::app_for(file) {
    if apps::status(app, &settings.apps).gui.is_some() {
      return apps::open(app, &settings.apps, Some(file));
    }
  }
  let editor = settings.editor_path.trim().to_string();
  if editor.is_empty() {
    let hint = apps::app_for(file).map(|a| format!("Install {} (see the Apps page)", a.name)).unwrap_or_else(|| "Set an editor in Settings".into());
    return Err(format!("No editor for this file yet. {hint}."));
  }
  std::process::Command::new(&editor)
    .arg(&path)
    .spawn()
    .map(|_| ())
    .map_err(|e| format!("Couldn't start {editor}: {e}"))
}

/// The brain (Claude Code) does a real edit in one of the Craft apps.
#[tauri::command]
async fn run_editor_job(state: State<'_, AppState>, job: EditorJob) -> Result<EditorResult, String> {
  let settings = state.settings();
  if let Some(input) = job.input_path.as_deref().filter(|p| !p.trim().is_empty()) {
    if !Path::new(input).is_file() {
      return Err("The input file doesn't exist anymore.".into());
    }
  }
  editor::run(&settings.brain, &settings.apps, &state.jobs_dir, &state.output_dir(), &job).await
}

fn image_data_url(path: &Path) -> Result<String, String> {
  let ext = path.extension().and_then(|e| e.to_str()).unwrap_or_default().to_ascii_lowercase();
  let mime = match ext.as_str() {
    "png" => "image/png",
    "jpg" | "jpeg" => "image/jpeg",
    "webp" => "image/webp",
    _ => return Err("Easel can edit PNG, JPEG and WebP images.".into()),
  };
  let size = std::fs::metadata(path).map_err(|e| format!("Couldn't open {}: {e}", path.display()))?.len();
  if size > MAX_INPUT_BYTES {
    return Err("That image is over 40 MB. Resize it first.".into());
  }
  let bytes = std::fs::read(path).map_err(|e| format!("Couldn't read {}: {e}", path.display()))?;
  Ok(format!("data:{mime};base64,{}", BASE64.encode(bytes)))
}

fn provider_name(provider: Provider) -> &'static str {
  match provider {
    Provider::Gemini => "gemini",
    Provider::Comfy => "comfy",
    Provider::ComfyWorkflow => "workflow",
  }
}

/// Lets the window show images from the output folder, wherever you put it.
fn allow_output_dir(app: &tauri::AppHandle, dir: &Path) {
  let _ = std::fs::create_dir_all(dir);
  if let Err(err) = app.asset_protocol_scope().allow_directory(dir, true) {
    log::warn!("couldn't allow {} for previews: {err}", dir.display());
  }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
  tauri::Builder::default()
    .plugin(tauri_plugin_dialog::init())
    .plugin(tauri_plugin_opener::init())
    .setup(|app| {
      let paths = app.path();
      let config_dir = paths.app_config_dir()?;
      let settings_path = config_dir.join("settings.json");
      let brain_dir = paths.app_cache_dir()?.join("brain-workdir");
      let jobs_dir = paths.app_cache_dir()?.join("editor-jobs");
      let pictures_dir = paths.picture_dir().ok();
      let settings = Settings::load(&settings_path);
      allow_output_dir(app.handle(), &settings.output_dir(pictures_dir.clone()));

      let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(15))
        // Image models can take a couple of minutes on a busy day.
        .timeout(Duration::from_secs(600))
        .build()?;

      app.manage(AppState { settings: Mutex::new(settings), settings_path, pictures_dir, brain_dir, jobs_dir, client });
      Ok(())
    })
    .invoke_handler(tauri::generate_handler![
      get_settings,
      save_settings,
      output_dir,
      gallery_list,
      gallery_delete,
      read_image,
      generate,
      brain_plan,
      brain_improve,
      gemini_models,
      comfy_checkpoints,
      local_models,
      gemini_text_models,
      detect_local_servers,
      comfy_preflight,
      brain_test,
      open_in_editor,
      apps_status,
      open_with_app,
      run_editor_job,
    ])
    .run(tauri::generate_context!())
    .expect("Easel couldn't start");
}
