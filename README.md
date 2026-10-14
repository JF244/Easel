# Easel

A local-first AI image studio for one person: you. No account, no credits, no
tracking. Everything you make is saved as normal files in `Pictures/Easel`.

- **Brain:** describe what you want; the brain writes the prompts, picks an
  engine and runs the jobs. Brains are tried in your order, and the next one
  takes over when one is missing or out of usage:
  - **Claude Code** (`claude -p`), on your Claude subscription
  - **Antigravity** (`agy -p`), on your Google subscription
  - **Gemini** (for example Gemini 4 / Argon), with your Gemini API key
  - **Claude API**, with your own API key (pay per use)
  - **Local model**: LM Studio or Ollama on this PC
  - **Remote servers**: any OpenAI-compatible endpoint, such as llama.cpp's
    `llama-server` (including one pooling GPUs across machines with `--rpc`),
    vLLM, LM Studio or Ollama on another PC, or a hosted API with a key
- **Advisor:** a second list of frontier brains (Claude, Gemini 4…) that a
  cheaper primary brain can lean on. Start a chat message with `/advisor` to
  send it straight to the advisor, or set the advisor to "auto" so the
  primary brain asks it when it's unsure and then plans with the advice.
- **History:** the brain chat is saved and survives switching tabs and
  restarting Easel; Create, Edit and Gallery keep their state across tabs.
- **Create:** prompt to image with Gemini (Nano Banana) or ComfyUI.
- **Edit:** change an image by prompt, or paint over an area to redo just that part.
- **Gallery:** browse, search prompts, edit, open in PhotoCraft, delete.
- **Your ComfyUI workflows:** run any workflow exported in API format,
  including local video (Wan and similar), with `{{prompt}}`-style placeholders.
- **Apps:** the Crafting Apps, free open-source editors that replace the
  Adobe suite. Easel finds the ones you have installed, opens each file in the
  right one, and picks up what you save there. The Claude Code brain can also
  use their tools to make exact edits for you ("add the title in white, crop to 16:9").

| Instead of    | App                                                         | Opens                         |
|---------------|-------------------------------------------------------------|-------------------------------|
| Photoshop     | [PhotoCraft](https://github.com/storytold/photocraft)       | PNG, JPEG, WebP, PSD, TIFF    |
| Illustrator   | [VectorCraft](https://github.com/storytold/vectorcraft)     | SVG, AI, EPS                  |
| Premiere Pro  | [FilmCraft](https://github.com/storytold/filmcraft)         | MP4, MOV, MKV, WebM           |
| Lightroom     | [LightCraft](https://github.com/storytold/lightcraft)       | DNG, CR2/CR3, NEF, ARW, RAF   |
| Acrobat       | [PdfCraft](https://github.com/storytold/pdfcraft)           | PDF                           |
| After Effects | [EffectCraft](https://github.com/storytold/effectcraft)     | (opened from the Apps page)   |
| InDesign      | [DesignCraft](https://github.com/storytold/designcraft)     | IDML                          |

Install them from each app's GitHub releases page (the Apps page links to
them). Easel looks on your PATH, in the usual install folders, for AppImages
in `~/Applications` or `~/Downloads`, and for Flatpak installs; you can also
set a path by hand. For the brain to edit with an app, its command-line tool
(`photocraft-cli` and so on) must be found too; every installer includes it.

Editor jobs run Claude Code with only that app's tools allowed, on a copy of
your file in a private work folder, so originals are never changed.

## Run it on CachyOS (or any Arch-based distro)

```bash
sudo pacman -S --needed rustup nodejs npm webkit2gtk-4.1 base-devel librsvg
rustup default stable
git clone https://github.com/JF244/easel.git
cd easel
npm ci
npm run tauri dev        # run from source with live reload
npm run tauri build      # build an installable AppImage / .rpm in src-tauri/target/release/bundle
```

If the window is blank on Wayland with an AMD card, start it with
`WEBKIT_DISABLE_DMABUF_RENDERER=1 npm run tauri dev`.

## Run it on Windows

Install [Rust](https://rustup.rs), [Node.js 22 LTS](https://nodejs.org) and
the "Desktop development with C++" workload from the Visual Studio Build Tools.
WebView2 is already part of Windows 10 and 11. Then:

```powershell
git clone https://github.com/JF244/easel.git
cd easel
npm ci
npm run tauri dev
npm run tauri build      # installer in src-tauri\target\release\bundle\nsis
```

Or skip building: every push to `main` builds Linux and Windows installers in
GitHub Actions. Open the latest run under **Actions** and download them from
**Artifacts**.

## Set up the AI

Open **Settings** in the app.

| What                | You need                                                                         |
|---------------------|----------------------------------------------------------------------------------|
| Gemini images       | An API key from aistudio.google.com (free tier, then pay per image)              |
| ComfyUI             | ComfyUI running (default `http://127.0.0.1:8188`) and a checkpoint picked        |
| Claude Code brain   | `claude` installed and signed in once (run `claude` in a terminal)               |
| Antigravity brain   | `agy` installed and signed in once (run `agy` in a terminal)                     |
| Claude API brain    | An API key from console.anthropic.com (billed separately from Claude Pro/Max)    |
| Local brain         | LM Studio (`http://127.0.0.1:1234`) or Ollama (`http://127.0.0.1:11434`) running |

For the 9070 XT on CachyOS, ComfyUI on ROCm runs Flux and SDXL locally. A 14B
model, or a 20–30B mixture-of-experts model, makes a good local brain.

Notes:
- The agent CLIs run in an empty scratch folder and get the prompt as a plain
  argument (never through a shell), so they can't read your files or run
  anything from a prompt.
- Antigravity's `-p` mode has open bug reports about hanging or printing nothing
  when it isn't attached to a terminal, mostly on Windows. If that happens, Easel
  waits up to 5 minutes, then moves on to the next brain.
- Gemini has no real mask input. For painted areas, Easel sends the mask as a
  second image and tells the model to change only the white part. ComfyUI does
  true masked inpainting.

## Develop

```bash
npm run typecheck                     # TypeScript
cd src-tauri && cargo test            # backend tests
```

Layout: `src/` is the React interface (pages in `src/pages`), `src-tauri/src`
is the Rust side (`brain.rs` is the fallback chain, `providers/` holds Gemini,
ComfyUI, Claude, the CLIs and the local model, `gallery.rs` is the folder
gallery).
