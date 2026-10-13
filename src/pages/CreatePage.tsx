import { useState } from "react";
import { api, ASPECT_RATIOS, engineReady, errorMessage, type GalleryItem, type Improved } from "../lib/api";
import { useApp } from "../lib/store";
import { EngineSelect, type EngineChoice } from "../components/EngineSelect";
import { BrainNote, ErrorNote } from "../components/ErrorNote";
import { ImagePicker } from "../components/ImagePicker";
import { ResultGrid } from "../components/ResultGrid";

export default function CreatePage() {
  const { settings, addToGallery } = useApp();
  const [engine, setEngine] = useState<EngineChoice>({ provider: "gemini" });
  const [prompt, setPrompt] = useState("");
  const [negative, setNegative] = useState("");
  const [aspect, setAspect] = useState("1:1");
  const [count, setCount] = useState(1);
  const [references, setReferences] = useState<string[]>([]);
  const [busy, setBusy] = useState<"generate" | "improve" | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [improved, setImproved] = useState<Improved | null>(null);
  const [results, setResults] = useState<GalleryItem[]>([]);

  const isComfy = engine.provider !== "gemini";
  const notReady = engineReady(settings, engine.provider);

  const improve = async () => {
    setBusy("improve");
    setError(null);
    try {
      const result = await api.brainImprove(prompt, false);
      setImproved(result);
      setPrompt(result.prompt);
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(null);
    }
  };

  const generate = async () => {
    setBusy("generate");
    setError(null);
    try {
      const items = await api.generate({
        provider: engine.provider,
        workflowId: engine.workflowId,
        prompt,
        negative: isComfy && negative.trim() ? negative : undefined,
        aspectRatio: aspect,
        count,
        imagePaths: references,
      });
      setResults(items);
      addToGallery(items);
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(null);
    }
  };

  return (
    <div className="mx-auto max-w-5xl space-y-5 p-6">
      <header>
        <h1 className="text-xl font-semibold">Create</h1>
        <p className="text-sm text-ink-2">Write a prompt, pick an engine, and generate. Results are saved to your gallery folder.</p>
      </header>

      <section className="card space-y-4 p-4">
        <div>
          <label className="label" htmlFor="create-prompt">
            Prompt
          </label>
          <textarea
            id="create-prompt"
            rows={4}
            className="field h-auto py-2"
            placeholder="A safety officer in a hard hat checking a valve at sunrise, photo, warm light"
            value={prompt}
            onChange={(e) => setPrompt(e.target.value)}
          />
          <div className="mt-2 flex items-center gap-3">
            <button type="button" className="btn h-8 text-xs" disabled={!prompt.trim() || busy !== null} onClick={improve}>
              {busy === "improve" ? "Improving…" : "Improve prompt with the brain"}
            </button>
            {improved && <BrainNote brain={improved.brain} skipped={improved.skipped} />}
          </div>
        </div>

        <div className="grid gap-4 sm:grid-cols-3">
          <EngineSelect id="create-engine" settings={settings} value={engine} onChange={setEngine} />
          <div>
            <label className="label" htmlFor="create-aspect">
              Shape
            </label>
            <select id="create-aspect" className="field" value={aspect} onChange={(e) => setAspect(e.target.value)}>
              {ASPECT_RATIOS.map((r) => (
                <option key={r} value={r}>
                  {r}
                </option>
              ))}
            </select>
          </div>
          <div>
            <label className="label" htmlFor="create-count">
              How many
            </label>
            <select id="create-count" className="field" value={count} onChange={(e) => setCount(Number(e.target.value))}>
              {[1, 2, 3, 4].map((n) => (
                <option key={n} value={n}>
                  {n}
                </option>
              ))}
            </select>
          </div>
        </div>

        {isComfy ? (
          <div>
            <label className="label" htmlFor="create-negative">
              Avoid (optional, uses the default from Settings when empty)
            </label>
            <input id="create-negative" className="field" value={negative} onChange={(e) => setNegative(e.target.value)} />
          </div>
        ) : (
          <ImagePicker label="Reference images (optional, up to 3)" paths={references} onChange={setReferences} max={3} />
        )}

        <div className="flex items-center gap-3">
          <button type="button" className="btn btn-primary" disabled={!prompt.trim() || busy !== null || notReady !== null} onClick={generate}>
            {busy === "generate" ? "Generating…" : `Generate ${count > 1 ? `${count} images` : "image"}`}
          </button>
          {notReady && <span className="text-sm text-ink-2">{notReady}</span>}
        </div>
        <ErrorNote message={error} />
      </section>

      <ResultGrid items={results} />
    </div>
  );
}
