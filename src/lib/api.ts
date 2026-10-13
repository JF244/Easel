import { convertFileSrc, invoke } from "@tauri-apps/api/core";

export type BrainKind = "claudeCli" | "agyCli" | "claudeApi" | "local";
export type ProviderId = "gemini" | "comfy" | "comfyWorkflow";

export interface CustomWorkflow {
  id: string;
  name: string;
  json: string;
}

export interface AppPaths {
  gui: string;
  cli: string;
}

export interface Settings {
  outputDir: string;
  editorPath: string;
  apps: Record<string, AppPaths>;
  gemini: { apiKey: string; imageModel: string };
  comfy: {
    url: string;
    checkpoint: string;
    steps: number;
    cfg: number;
    sampler: string;
    scheduler: string;
    negative: string;
    baseSize: number;
    workflows: CustomWorkflow[];
  };
  brain: {
    order: BrainKind[];
    claudeCliPath: string;
    claudeCliModel: string;
    agyCliPath: string;
    claudeApiKey: string;
    claudeApiModel: string;
    localUrl: string;
    localModel: string;
  };
}

export interface MediaMeta {
  prompt: string;
  provider: string;
  model: string;
  mode: string;
  seed: number | null;
  createdAt: string;
  note: string;
}

export interface GalleryItem {
  path: string;
  fileName: string;
  isVideo: boolean;
  kind: "image" | "video" | "document";
  modifiedMs: number;
  meta: MediaMeta | null;
}

export interface GenerateRequest {
  provider: ProviderId;
  prompt: string;
  negative?: string;
  aspectRatio?: string;
  count?: number;
  seed?: number;
  images?: string[];
  imagePaths?: string[];
  mask?: string;
  strength?: number;
  workflowId?: string;
  model?: string;
}

export interface ChatTurn {
  role: "user" | "assistant";
  text: string;
}

export interface PlannerContext {
  geminiReady: boolean;
  comfyReady: boolean;
  workflows: string[];
  editors: { id: string; purpose: string }[];
  images: { path: string; prompt: string; attached: boolean }[];
}

export interface PlannedJob {
  engine: string;
  workflow?: string | null;
  app?: string | null;
  prompt: string;
  negative?: string | null;
  aspectRatio?: string | null;
  count?: number | null;
  inputImage?: string | null;
}

export interface Plan {
  reply: string;
  jobs: PlannedJob[];
  brain: string;
  skipped: string[];
}

export interface Improved {
  prompt: string;
  brain: string;
  skipped: string[];
}

export interface AppStatus {
  id: string;
  name: string;
  replaces: string;
  purpose: string;
  extensions: string[];
  releasesUrl: string;
  /** Where it was found; null when not installed. */
  gui: string | null;
  /** Its command-line tool; the brain needs this to make edits. */
  cli: string | null;
}

export interface EditorResult {
  item: GalleryItem;
  summary: string;
}

export const BRAIN_LABELS: Record<BrainKind, string> = {
  claudeCli: "Claude Code (your Claude subscription)",
  agyCli: "Antigravity (your Google subscription)",
  claudeApi: "Claude API (your API key, pay per use)",
  local: "Local model (LM Studio / Ollama)",
};

export const ASPECT_RATIOS = ["1:1", "16:9", "9:16", "4:3", "3:4", "3:2", "2:3"];

export const api = {
  getSettings: () => invoke<Settings>("get_settings"),
  saveSettings: (settings: Settings) => invoke<Settings>("save_settings", { settings }),
  outputDir: () => invoke<string>("output_dir"),
  galleryList: () => invoke<GalleryItem[]>("gallery_list"),
  galleryDelete: (path: string) => invoke<void>("gallery_delete", { path }),
  readImage: (path: string) => invoke<string>("read_image", { path }),
  generate: (request: GenerateRequest) => invoke<GalleryItem[]>("generate", { request }),
  brainPlan: (turns: ChatTurn[], context: PlannerContext) => invoke<Plan>("brain_plan", { turns, context }),
  brainImprove: (prompt: string, editing: boolean) => invoke<Improved>("brain_improve", { prompt, editing }),
  geminiModels: (apiKey: string) => invoke<string[]>("gemini_models", { apiKey }),
  comfyCheckpoints: (url: string) => invoke<string[]>("comfy_checkpoints", { url }),
  localModels: (url: string) => invoke<string[]>("local_models", { url }),
  openInEditor: (path: string) => invoke<void>("open_in_editor", { path }),
  appsStatus: () => invoke<AppStatus[]>("apps_status"),
  openWithApp: (appId: string, path?: string) => invoke<void>("open_with_app", { appId, path: path ?? null }),
  runEditorJob: (job: { app: string; instructions: string; inputPath?: string }) => invoke<EditorResult>("run_editor_job", { job }),
};

/** A URL the window can show for a file on disk. */
export function fileUrl(path: string): string {
  return convertFileSrc(path);
}

/** Tauri rejects with a plain string; anything else gets a readable fallback. */
export function errorMessage(err: unknown): string {
  if (typeof err === "string") return err;
  if (err instanceof Error) return err.message;
  return "Something went wrong.";
}

export function engineReady(settings: Settings | null, provider: ProviderId): string | null {
  if (!settings) return "Settings are still loading.";
  if (provider === "gemini" && !settings.gemini.apiKey.trim()) return "Add your Gemini API key in Settings first.";
  if (provider === "comfy" && !settings.comfy.checkpoint.trim()) return "Pick a ComfyUI checkpoint in Settings first.";
  if (provider === "comfyWorkflow" && settings.comfy.workflows.length === 0) return "Add a ComfyUI workflow in Settings first.";
  return null;
}

/** The Craft app that opens this file by default, if any. */
export function appForPath(apps: AppStatus[], path: string): AppStatus | undefined {
  const ext = path.split(".").pop()?.toLowerCase() ?? "";
  return apps.find((a) => a.extensions.includes(ext));
}

export function extensionOf(path: string): string {
  return (path.split(".").pop() ?? "").toUpperCase();
}
