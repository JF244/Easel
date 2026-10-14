import { useCallback, useEffect, useRef, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { api, engineReady, errorMessage, type GalleryItem, type Improved } from "../lib/api";
import { useApp } from "../lib/store";
import { EngineSelect, type EngineChoice } from "../components/EngineSelect";
import { BrainNote, ErrorNote } from "../components/ErrorNote";
import { ResultGrid } from "../components/ResultGrid";
import { ComfyPreflight } from "../components/ComfyPreflight";

const PAINT_COLOR = "rgba(240, 90, 90, 1)";

export default function EditPage() {
  const { settings, editPath, editImage, addToGallery } = useApp();
  const [source, setSource] = useState<string | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [engine, setEngine] = useState<EngineChoice>({ provider: "gemini" });
  const [prompt, setPrompt] = useState("");
  const [strength, setStrength] = useState(0.6);
  const [brush, setBrush] = useState(48);
  const [erasing, setErasing] = useState(false);
  const [hasMask, setHasMask] = useState(false);
  const [busy, setBusy] = useState<"generate" | "improve" | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [improved, setImproved] = useState<Improved | null>(null);
  const [results, setResults] = useState<GalleryItem[]>([]);

  const imgRef = useRef<HTMLImageElement>(null);
  const paintRef = useRef<HTMLCanvasElement>(null);
  const drawing = useRef(false);
  const last = useRef<{ x: number; y: number } | null>(null);

  // Load the chosen image as a data URL so the mask canvas lines up with it exactly.
  useEffect(() => {
    if (!editPath) return;
    setLoadError(null);
    setSource(null);
    setResults([]);
    api
      .readImage(editPath)
      .then(setSource)
      .catch((err) => setLoadError(errorMessage(err)));
  }, [editPath]);

  const resetCanvas = useCallback(() => {
    const img = imgRef.current;
    const canvas = paintRef.current;
    if (!img || !canvas) return;
    canvas.width = img.naturalWidth;
    canvas.height = img.naturalHeight;
    setHasMask(false);
  }, []);

  const pickFile = async () => {
    const chosen = await open({ multiple: false, filters: [{ name: "Images", extensions: ["png", "jpg", "jpeg", "webp"] }] });
    if (typeof chosen === "string") editImage(chosen);
  };

  const pointAt = (e: React.PointerEvent<HTMLCanvasElement>) => {
    const canvas = e.currentTarget;
    const rect = canvas.getBoundingClientRect();
    return {
      x: ((e.clientX - rect.left) / rect.width) * canvas.width,
      y: ((e.clientY - rect.top) / rect.height) * canvas.height,
      scale: canvas.width / rect.width,
    };
  };

  const paint = (e: React.PointerEvent<HTMLCanvasElement>) => {
    const ctx = e.currentTarget.getContext("2d");
    if (!ctx) return;
    const { x, y, scale } = pointAt(e);
    ctx.globalCompositeOperation = erasing ? "destination-out" : "source-over";
    ctx.strokeStyle = PAINT_COLOR;
    ctx.fillStyle = PAINT_COLOR;
    ctx.lineCap = "round";
    ctx.lineJoin = "round";
    // The brush size is in screen pixels, so it feels the same at any zoom.
    ctx.lineWidth = brush * scale;
    const from = last.current ?? { x, y };
    ctx.beginPath();
    ctx.moveTo(from.x, from.y);
    ctx.lineTo(x, y);
    ctx.stroke();
    last.current = { x, y };
    if (!erasing) setHasMask(true);
  };

  /** White where you painted, black everywhere else, at the image's full size. */
  const exportMask = (): string | undefined => {
    const paintCanvas = paintRef.current;
    if (!paintCanvas || !hasMask) return undefined;
    const white = document.createElement("canvas");
    white.width = paintCanvas.width;
    white.height = paintCanvas.height;
    const wctx = white.getContext("2d")!;
    wctx.drawImage(paintCanvas, 0, 0);
    wctx.globalCompositeOperation = "source-in";
    wctx.fillStyle = "#fff";
    wctx.fillRect(0, 0, white.width, white.height);

    const mask = document.createElement("canvas");
    mask.width = paintCanvas.width;
    mask.height = paintCanvas.height;
    const mctx = mask.getContext("2d")!;
    mctx.fillStyle = "#000";
    mctx.fillRect(0, 0, mask.width, mask.height);
    mctx.drawImage(white, 0, 0);
    return mask.toDataURL("image/png");
  };

  const improve = async () => {
    setBusy("improve");
    setError(null);
    try {
      const result = await api.brainImprove(prompt, true);
      setImproved(result);
      setPrompt(result.prompt);
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(null);
    }
  };

  const generate = async () => {
    if (!source) return;
    setBusy("generate");
    setError(null);
    try {
      const items = await api.generate({
        provider: engine.provider,
        workflowId: engine.workflowId,
        prompt,
        images: [source],
        mask: exportMask(),
        strength,
        count: 1,
      });
      setResults(items);
      addToGallery(items);
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(null);
    }
  };

  const notReady = engineReady(settings, engine.provider);

  return (
    <div className="mx-auto max-w-6xl space-y-5 p-6">
      <header className="flex flex-wrap items-end justify-between gap-3">
        <div>
          <h1 className="text-xl font-semibold">Edit</h1>
          <p className="text-sm text-ink-2">Describe a change. Paint over an area to change only that part; leave it unpainted to edit the whole image.</p>
        </div>
        <button type="button" className="btn" onClick={pickFile}>
          Open an image…
        </button>
      </header>

      <ErrorNote message={loadError} />

      {!editPath && !loadError && (
        <div className="card p-10 text-center text-ink-2">
          Open an image from your PC, or choose <strong>Edit this</strong> on any result or gallery image.
        </div>
      )}

      {source && (
        <div className="grid gap-5 lg:grid-cols-[1fr_320px]">
          <div className="card flex items-center justify-center overflow-hidden bg-black p-2">
            <div className="relative inline-block max-w-full">
              <img ref={imgRef} src={source} alt="Image being edited" className="block max-h-[70vh] max-w-full select-none" onLoad={resetCanvas} draggable={false} />
              <canvas
                ref={paintRef}
                aria-label="Paint over the area to change"
                className="absolute inset-0 h-full w-full cursor-crosshair touch-none opacity-50"
                onPointerDown={(e) => {
                  e.currentTarget.setPointerCapture(e.pointerId);
                  drawing.current = true;
                  last.current = null;
                  paint(e);
                }}
                onPointerMove={(e) => drawing.current && paint(e)}
                onPointerUp={() => {
                  drawing.current = false;
                  last.current = null;
                }}
                onPointerLeave={() => {
                  drawing.current = false;
                  last.current = null;
                }}
              />
            </div>
          </div>

          <aside className="card h-fit space-y-4 p-4">
            <div>
              <label className="label" htmlFor="brush-size">
                Brush size: {brush}px
              </label>
              <input id="brush-size" type="range" min={8} max={200} value={brush} onChange={(e) => setBrush(Number(e.target.value))} className="w-full accent-[var(--color-accent)]" />
              <div className="mt-2 flex gap-2">
                <button type="button" className="btn h-8 flex-1 text-xs" aria-pressed={!erasing} onClick={() => setErasing(false)}>
                  {erasing ? "Paint" : "Painting"}
                </button>
                <button type="button" className="btn h-8 flex-1 text-xs" aria-pressed={erasing} onClick={() => setErasing(true)}>
                  {erasing ? "Erasing" : "Erase"}
                </button>
                <button type="button" className="btn h-8 flex-1 text-xs" onClick={resetCanvas}>
                  Clear
                </button>
              </div>
              <p className="mt-2 text-xs text-ink-3">{hasMask ? "Only the painted area will change." : "Nothing painted: the whole image can change."}</p>
            </div>

            <EngineSelect id="edit-engine" settings={settings} value={engine} onChange={setEngine} />

            {engine.provider !== "gemini" && !hasMask && (
              <div>
                <label className="label" htmlFor="edit-strength">
                  How much to change: {Math.round(strength * 100)}%
                </label>
                <input id="edit-strength" type="range" min={0.1} max={1} step={0.05} value={strength} onChange={(e) => setStrength(Number(e.target.value))} className="w-full accent-[var(--color-accent)]" />
              </div>
            )}

            <div>
              <label className="label" htmlFor="edit-prompt">
                What to change
              </label>
              <textarea id="edit-prompt" rows={4} className="field h-auto py-2" placeholder="Replace the sky with a stormy sunset" value={prompt} onChange={(e) => setPrompt(e.target.value)} />
              <div className="mt-2 flex flex-wrap items-center gap-2">
                <button type="button" className="btn h-8 text-xs" disabled={!prompt.trim() || busy !== null} onClick={improve}>
                  {busy === "improve" ? "Improving…" : "Improve with the brain"}
                </button>
                {improved && <BrainNote brain={improved.brain} skipped={improved.skipped} />}
              </div>
            </div>

            <ComfyPreflight provider={engine.provider} />
            <button type="button" className="btn btn-primary w-full" disabled={!prompt.trim() || busy !== null || notReady !== null} onClick={generate}>
              {busy === "generate" ? "Working…" : hasMask ? "Redo the painted area" : "Apply the change"}
            </button>
            {notReady && <p className="text-sm text-ink-2">{notReady}</p>}
            <ErrorNote message={error} />
          </aside>
        </div>
      )}

      <ResultGrid items={results} />
    </div>
  );
}
