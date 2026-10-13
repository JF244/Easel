# Easel

A local-first AI image studio for one person: you. No account, no credits, no
tracking. Everything you make is saved as normal files in `Pictures/Easel`.

- **Brain:** describe what you want; the brain writes the prompts, picks an
  engine and runs the jobs. Brains are tried in your order, and the next one
  takes over when one is missing or out of usage:
  1. **Claude Code** (`claude -p`), on your Claude subscription
  2. **Antigravity** (`agy -p`), on your Google subscription
  3. **Claude API**, with your own API key (pay per use)
  4. **Local model**: LM Studio or Ollama on your own GPU
- **Create:** prompt to image with Gemini (Nano Banana) or ComfyUI.
- **Edit:** change an image by prompt, or paint over an area to redo just that part.
- **Gallery:** browse, search prompts, edit, open in PhotoCraft, delete.
- **Your ComfyUI workflows:** run any workflow exported in API format,
  including local video (Wan and similar), with `{{prompt}}`-style placeholders.

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
