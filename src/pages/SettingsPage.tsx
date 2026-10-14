import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { allBrains, api, brainLabel, errorMessage, type AdvisorMode, type BrainKind, type CustomWorkflow, type Settings } from "../lib/api";
import { useApp } from "../lib/store";
import { ErrorNote } from "../components/ErrorNote";


export default function SettingsPage() {
  const { settings, setSettings, refreshGallery } = useApp();
  const [draft, setDraft] = useState<Settings | null>(settings);
  const [saveState, setSaveState] = useState<"idle" | "saving" | "saved">("idle");
  const [saveError, setSaveError] = useState<string | null>(null);

  useEffect(() => {
    if (settings && !draft) setDraft(settings);
  }, [settings, draft]);

  if (!draft) return <p className="p-6 text-ink-2">Loading settings…</p>;

  const update = (change: (s: Settings) => void) => {
    const next = structuredClone(draft);
    change(next);
    setDraft(next);
    setSaveState("idle");
  };

  const save = async () => {
    setSaveState("saving");
    setSaveError(null);
    try {
      const saved = await api.saveSettings(draft);
      setSettings(saved);
      setDraft(saved);
      setSaveState("saved");
      void refreshGallery();
    } catch (err) {
      setSaveError(errorMessage(err));
      setSaveState("idle");
    }
  };

  const dirty = JSON.stringify(draft) !== JSON.stringify(settings);

  return (
    <div className="mx-auto max-w-3xl space-y-6 p-6 pb-28">
      <header>
        <h1 className="text-xl font-semibold">Settings</h1>
        <p className="text-sm text-ink-2">Everything is stored on this PC only. Keys are kept in Easel's settings file, readable only by your account.</p>
      </header>

      <BrainSection draft={draft} update={update} />
      <GeminiSection draft={draft} update={update} />
      <ComfySection draft={draft} update={update} />
      <WorkflowSection draft={draft} update={update} />
      <FoldersSection draft={draft} update={update} />

      <div className="fixed right-0 bottom-0 left-44 border-t border-line bg-panel/95 px-6 py-3 backdrop-blur">
        <div className="mx-auto flex max-w-3xl items-center gap-3">
          <button type="button" className="btn btn-primary" disabled={!dirty || saveState === "saving"} onClick={save}>
            {saveState === "saving" ? "Saving…" : "Save settings"}
          </button>
          {saveState === "saved" && !dirty && <span className="text-sm text-ok">Saved.</span>}
          {dirty && <span className="text-sm text-ink-2">You have unsaved changes.</span>}
          <div className="flex-1">
            <ErrorNote message={saveError} />
          </div>
        </div>
      </div>
    </div>
  );
}

type SectionProps = { draft: Settings; update: (change: (s: Settings) => void) => void };

function Section({ title, intro, children }: { title: string; intro: string; children: React.ReactNode }) {
  return (
    <section className="card space-y-4 p-5">
      <div>
        <h2 className="text-base font-semibold">{title}</h2>
        <p className="text-sm text-ink-2">{intro}</p>
      </div>
      {children}
    </section>
  );
}

function TextField({
  id,
  label,
  value,
  onChange,
  placeholder,
  secret,
  hint,
}: {
  id: string;
  label: string;
  value: string;
  onChange: (v: string) => void;
  placeholder?: string;
  secret?: boolean;
  hint?: string;
}) {
  const [show, setShow] = useState(false);
  return (
    <div>
      <label className="label" htmlFor={id}>
        {label}
      </label>
      <div className="flex gap-2">
        <input
          id={id}
          className="field"
          type={secret && !show ? "password" : "text"}
          autoComplete="off"
          spellCheck={false}
          value={value}
          placeholder={placeholder}
          onChange={(e) => onChange(e.target.value)}
        />
        {secret && (
          <button type="button" className="btn shrink-0 text-xs" onClick={() => setShow((s) => !s)}>
            {show ? "Hide" : "Show"}
          </button>
        )}
      </div>
      {hint && <p className="mt-1 text-xs text-ink-3">{hint}</p>}
    </div>
  );
}

/** A "Find" button that loads a list from a server and offers it as choices. */
function ListLoader({ label, load, onPick, current }: { label: string; load: () => Promise<string[]>; onPick: (v: string) => void; current: string }) {
  const [items, setItems] = useState<string[] | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const run = async () => {
    setBusy(true);
    setError(null);
    try {
      const found = await load();
      setItems(found);
      if (found.length === 0) setError("Nothing found.");
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  };
  return (
    <div className="space-y-2">
      <button type="button" className="btn h-8 text-xs" disabled={busy} onClick={run}>
        {busy ? "Looking…" : label}
      </button>
      {items && items.length > 0 && (
        <div className="flex flex-wrap gap-1">
          {items.map((item) => (
            <button
              key={item}
              type="button"
              aria-pressed={item === current}
              className={`rounded border px-2 py-1 text-xs ${item === current ? "border-accent text-accent" : "border-line text-ink-2 hover:text-ink"}`}
              onClick={() => onPick(item)}
            >
              {item}
            </button>
          ))}
        </div>
      )}
      <ErrorNote message={error} />
    </div>
  );
}

/** An ordered list of brains with move, turn-off and turn-on controls. */
function BrainOrder({ draft, list, onChange, emptyText }: { draft: Settings; list: BrainKind[]; onChange: (next: BrainKind[]) => void; emptyText: string }) {
  const off = allBrains(draft).filter((b) => !list.includes(b));
  const move = (i: number, by: number) => {
    const next = [...list];
    const j = i + by;
    if (j < 0 || j >= next.length) return;
    [next[i], next[j]] = [next[j], next[i]];
    onChange(next);
  };
  return (
    <div className="space-y-2">
      <ol className="space-y-2">
        {list.map((kind, i) => (
          <li key={kind} className="flex flex-wrap items-center gap-2 rounded-md border border-line bg-panel-2 px-3 py-2">
            <span className="w-5 text-ink-3">{i + 1}.</span>
            <span className="flex-1">{brainLabel(draft, kind)}</span>
            <button type="button" className="btn h-8 w-8 px-0" aria-label={`Move ${brainLabel(draft, kind)} up`} disabled={i === 0} onClick={() => move(i, -1)}>
              ↑
            </button>
            <button type="button" className="btn h-8 w-8 px-0" aria-label={`Move ${brainLabel(draft, kind)} down`} disabled={i === list.length - 1} onClick={() => move(i, 1)}>
              ↓
            </button>
            <BrainTestButton kind={kind} />
            <button type="button" className="btn h-8 text-xs" onClick={() => onChange(list.filter((k) => k !== kind))}>
              Turn off
            </button>
          </li>
        ))}
        {list.length === 0 && <li className="text-sm text-ink-2">{emptyText}</li>}
      </ol>
      {off.length > 0 && (
        <div className="flex flex-wrap gap-2">
          {off.map((kind) => (
            <button key={kind} type="button" className="btn h-8 text-xs" onClick={() => onChange([...list, kind])}>
              Add: {brainLabel(draft, kind)}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}

function BrainSection({ draft, update }: SectionProps) {
  return (
    <Section
      title="Brain"
      intro="The brain plans jobs and writes prompts. Easel asks the first one on the list and moves down when one is missing, out of usage, or gives an unusable answer."
    >
      <div>
        <h3 className="label">Primary brains, in order</h3>
        <BrainOrder draft={draft} list={draft.brain.order} onChange={(next) => update((s) => (s.brain.order = next))} emptyText="No brain is on. Add one below." />
      </div>

      <div className="space-y-3 border-t border-line pt-4">
        <div>
          <h3 className="text-sm font-semibold">Advisor</h3>
          <p className="text-sm text-ink-2">
            A stronger, frontier brain your primary brain can lean on. Start a chat message with <code className="rounded bg-panel-2 px-1">/advisor</code> to send it
            straight to the advisor.
          </p>
        </div>
        <div className="max-w-sm">
          <label className="label" htmlFor="advisor-mode">
            When to ask the advisor
          </label>
          <select id="advisor-mode" className="field" value={draft.brain.advisorMode} onChange={(e) => update((s) => (s.brain.advisorMode = e.target.value as AdvisorMode))}>
            <option value="off">Never</option>
            <option value="onRequest">Only when I type /advisor</option>
            <option value="auto">Also when the primary brain is unsure</option>
          </select>
        </div>
        {draft.brain.advisorMode !== "off" && (
          <div>
            <h3 className="label">Advisor brains, in order</h3>
            <BrainOrder draft={draft} list={draft.brain.advisorOrder} onChange={(next) => update((s) => (s.brain.advisorOrder = next))} emptyText="No advisor chosen. Add one below." />
          </div>
        )}
      </div>

      <RemoteServers draft={draft} update={update} />

      <div className="grid gap-4 border-t border-line pt-4 sm:grid-cols-2">
        <TextField
          id="claude-cli-path"
          label="Claude Code program"
          value={draft.brain.claudeCliPath}
          onChange={(v) => update((s) => (s.brain.claudeCliPath = v))}
          hint="“claude” finds it on your PATH, including npm's claude.cmd. If you set a path, prefer the real claude.exe. Sign in once by running claude in a terminal. Exact edits in the Craft apps need Claude Code."
        />
        <TextField
          id="claude-cli-model"
          label="Claude Code model (optional)"
          value={draft.brain.claudeCliModel}
          placeholder="Leave empty for Claude Code's default"
          onChange={(v) => update((s) => (s.brain.claudeCliModel = v))}
          hint="For example opus or sonnet."
        />
        <TextField
          id="agy-cli-path"
          label="Antigravity program"
          value={draft.brain.agyCliPath}
          onChange={(v) => update((s) => (s.brain.agyCliPath = v))}
          hint="“agy” finds it on your PATH (an npm agy.cmd works too), or set the full path to agy.exe. Sign in once by running agy in a terminal."
        />
        <div>
          <TextField
            id="gemini-brain-model"
            label="Gemini brain model"
            value={draft.brain.geminiModel}
            placeholder="Empty picks the newest Pro model on your key"
            onChange={(v) => update((s) => (s.brain.geminiModel = v))}
            hint="Uses the Gemini API key from the Gemini section below."
          />
          <div className="mt-2">
            <ListLoader label="Find Gemini text models" load={() => api.geminiTextModels(draft.gemini.apiKey)} current={draft.brain.geminiModel} onPick={(v) => update((s) => (s.brain.geminiModel = v))} />
          </div>
        </div>
        <TextField
          id="claude-api-model"
          label="Claude API model"
          value={draft.brain.claudeApiModel}
          onChange={(v) => update((s) => (s.brain.claudeApiModel = v))}
          hint="claude-opus-5-5 by default. claude-haiku-5-5 is much cheaper for prompt writing."
        />
        <TextField
          id="claude-api-key"
          label="Claude API key (only for the Claude API brain)"
          secret
          value={draft.brain.claudeApiKey}
          placeholder="sk-ant-…"
          onChange={(v) => update((s) => (s.brain.claudeApiKey = v))}
          hint="From console.anthropic.com. Billed per use, separate from a Claude Pro or Max subscription."
        />
        <TextField
          id="local-url"
          label="Local model server (this PC)"
          value={draft.brain.localUrl}
          onChange={(v) => update((s) => (s.brain.localUrl = v))}
          hint="LM Studio: http://127.0.0.1:1234 · Ollama: http://127.0.0.1:11434"
        />
        <div>
          <TextField id="local-model" label="Local model" value={draft.brain.localModel} placeholder="Empty uses the first loaded model" onChange={(v) => update((s) => (s.brain.localModel = v))} />
          <div className="mt-2">
            <ListLoader label="Find loaded models" load={() => api.localModels(draft.brain.localUrl)} current={draft.brain.localModel} onPick={(v) => update((s) => (s.brain.localModel = v))} />
          </div>
          <div className="mt-2">
            <ServerDetector onPick={(url) => update((s) => (s.brain.localUrl = url))} />
          </div>
        </div>
      </div>
    </Section>
  );
}

function RemoteServers({ draft, update }: SectionProps) {
  const add = () =>
    update((s) =>
      s.brain.remotes.push({ id: `r${Date.now().toString(36)}`, name: "", url: "", apiKey: "", model: "" }),
    );
  const remove = (id: string) =>
    update((s) => {
      s.brain.remotes = s.brain.remotes.filter((r) => r.id !== id);
      s.brain.order = s.brain.order.filter((k) => k !== `remote:${id}`);
      s.brain.advisorOrder = s.brain.advisorOrder.filter((k) => k !== `remote:${id}`);
    });
  return (
    <div className="space-y-3 border-t border-line pt-4">
      <div>
        <h3 className="text-sm font-semibold">Remote servers</h3>
        <p className="text-sm text-ink-2">
          Any OpenAI-compatible server on another machine or online: llama.cpp's llama-server (including one pooling GPUs with <code className="rounded bg-panel-2 px-1">--rpc</code>), vLLM,
          LM Studio or Ollama on another PC, or a hosted API with a key. Add one, then put it in the primary or advisor list above.
        </p>
      </div>
      {draft.brain.remotes.map((r, i) => (
        <div key={r.id} className="grid gap-3 rounded-md border border-line p-3 sm:grid-cols-2">
          <TextField id={`remote-name-${r.id}`} label="Name" value={r.name} placeholder="LAN box" onChange={(v) => update((s) => (s.brain.remotes[i].name = v))} />
          <TextField
            id={`remote-url-${r.id}`}
            label="Address"
            value={r.url}
            placeholder="http://192.168.1.20:8080"
            onChange={(v) => update((s) => (s.brain.remotes[i].url = v))}
            hint="Easel adds /v1 unless the address already ends in /v1."
          />
          <TextField id={`remote-key-${r.id}`} label="API key (if the server needs one)" secret value={r.apiKey} onChange={(v) => update((s) => (s.brain.remotes[i].apiKey = v))} />
          <div>
            <TextField id={`remote-model-${r.id}`} label="Model" value={r.model} placeholder="Empty uses the first model it lists" onChange={(v) => update((s) => (s.brain.remotes[i].model = v))} />
            <div className="mt-2">
              <ListLoader label="Find models" load={() => api.localModels(r.url, r.apiKey)} current={r.model} onPick={(v) => update((s) => (s.brain.remotes[i].model = v))} />
            </div>
          </div>
          <div className="sm:col-span-2">
            <button type="button" className="btn btn-danger h-8 text-xs" onClick={() => remove(r.id)}>
              Remove this server
            </button>
          </div>
        </div>
      ))}
      <button type="button" className="btn h-8 text-xs" onClick={add}>
        Add a remote server
      </button>
    </div>
  );
}

function GeminiSection({ draft, update }: SectionProps) {
  return (
    <Section title="Gemini (Nano Banana)" intro="Image generation and editing from Google, billed to your own API key. Get a key at aistudio.google.com; it has a free tier.">
      <TextField id="gemini-key" label="Gemini API key" secret value={draft.gemini.apiKey} placeholder="AIza…" onChange={(v) => update((s) => (s.gemini.apiKey = v))} />
      <div>
        <TextField id="gemini-model" label="Image model" value={draft.gemini.imageModel} onChange={(v) => update((s) => (s.gemini.imageModel = v))} />
        <div className="mt-2">
          <ListLoader label="Find image models for this key" load={() => api.geminiModels(draft.gemini.apiKey)} current={draft.gemini.imageModel} onPick={(v) => update((s) => (s.gemini.imageModel = v))} />
        </div>
      </div>
    </Section>
  );
}

function ComfySection({ draft, update }: SectionProps) {
  const c = draft.comfy;
  return (
    <Section title="ComfyUI" intro="Runs image models on your own GPU, free and private. Start ComfyUI first, then load its checkpoint list here.">
      <TextField id="comfy-url" label="ComfyUI address" value={c.url} onChange={(v) => update((s) => (s.comfy.url = v))} hint="Usually http://127.0.0.1:8188" />
      <div>
        <TextField id="comfy-checkpoint" label="Checkpoint" value={c.checkpoint} onChange={(v) => update((s) => (s.comfy.checkpoint = v))} hint="Any single-file model: SD 1.5, SDXL, or a Flux all-in-one checkpoint." />
        <div className="mt-2">
          <ListLoader label="Find checkpoints" load={() => api.comfyCheckpoints(c.url)} current={c.checkpoint} onPick={(v) => update((s) => (s.comfy.checkpoint = v))} />
        </div>
      </div>
      <div className="grid gap-4 sm:grid-cols-3">
        <div>
          <label className="label" htmlFor="comfy-steps">
            Steps
          </label>
          <input id="comfy-steps" type="number" min={1} max={150} className="field" value={c.steps} onChange={(e) => update((s) => (s.comfy.steps = Number(e.target.value) || 1))} />
        </div>
        <div>
          <label className="label" htmlFor="comfy-cfg">
            Guidance (CFG)
          </label>
          <input id="comfy-cfg" type="number" step={0.5} min={0} max={30} className="field" value={c.cfg} onChange={(e) => update((s) => (s.comfy.cfg = Number(e.target.value)))} />
        </div>
        <div>
          <label className="label" htmlFor="comfy-size">
            Model size
          </label>
          <select id="comfy-size" className="field" value={c.baseSize} onChange={(e) => update((s) => (s.comfy.baseSize = Number(e.target.value)))}>
            <option value={512}>512 (SD 1.5)</option>
            <option value={768}>768</option>
            <option value={1024}>1024 (SDXL, Flux)</option>
          </select>
        </div>
        <TextField id="comfy-sampler" label="Sampler" value={c.sampler} onChange={(v) => update((s) => (s.comfy.sampler = v))} />
        <TextField id="comfy-scheduler" label="Scheduler" value={c.scheduler} onChange={(v) => update((s) => (s.comfy.scheduler = v))} />
      </div>
      <TextField id="comfy-negative" label="Things to avoid (default)" value={c.negative} onChange={(v) => update((s) => (s.comfy.negative = v))} hint="Flux ignores this; use guidance 1 to 3.5 and fewer steps for Flux." />
    </Section>
  );
}

function WorkflowSection({ draft, update }: SectionProps) {
  const [editing, setEditing] = useState<CustomWorkflow | null>(null);
  const [error, setError] = useState<string | null>(null);

  const saveWorkflow = () => {
    if (!editing) return;
    if (!editing.name.trim()) return setError("Give the workflow a name.");
    try {
      JSON.parse(editing.json);
    } catch {
      return setError("That isn't valid JSON. In ComfyUI use Workflow > Export (API) and paste the whole file.");
    }
    update((s) => {
      const list = s.comfy.workflows;
      const i = list.findIndex((w) => w.id === editing.id);
      if (i >= 0) list[i] = editing;
      else list.push(editing);
    });
    setEditing(null);
    setError(null);
  };

  return (
    <Section
      title="Your ComfyUI workflows"
      intro="Run any ComfyUI workflow from Easel, including local video (for example Wan). Export it with Workflow > Export (API), then put placeholders in its text fields: {{prompt}}, {{negative}}, {{seed}}, {{width}}, {{height}}, {{image}}, {{mask}}, {{strength}}."
    >
      <ul className="space-y-2">
        {draft.comfy.workflows.map((w) => (
          <li key={w.id} className="flex items-center gap-2 rounded-md border border-line bg-panel-2 px-3 py-2">
            <span className="flex-1">{w.name}</span>
            <button type="button" className="btn h-8 text-xs" onClick={() => setEditing({ ...w })}>
              Change
            </button>
            <button type="button" className="btn btn-danger h-8 text-xs" onClick={() => update((s) => (s.comfy.workflows = s.comfy.workflows.filter((x) => x.id !== w.id)))}>
              Remove
            </button>
          </li>
        ))}
      </ul>
      {editing ? (
        <div className="space-y-3 rounded-md border border-line p-3">
          <TextField id="wf-name" label="Name" value={editing.name} onChange={(v) => setEditing({ ...editing, name: v })} placeholder="Wan 2.2 video" />
          <div>
            <label className="label" htmlFor="wf-json">
              Workflow (API format JSON)
            </label>
            <textarea id="wf-json" rows={8} className="field h-auto py-2 font-mono text-xs" value={editing.json} onChange={(e) => setEditing({ ...editing, json: e.target.value })} spellCheck={false} />
          </div>
          <div className="flex gap-2">
            <button type="button" className="btn btn-primary h-8 text-xs" onClick={saveWorkflow}>
              Keep this workflow
            </button>
            <button type="button" className="btn h-8 text-xs" onClick={() => setEditing(null)}>
              Cancel
            </button>
          </div>
          <ErrorNote message={error} />
          <p className="text-xs text-ink-3">Remember to press Save settings at the bottom afterwards.</p>
        </div>
      ) : (
        <button type="button" className="btn h-8 text-xs" onClick={() => setEditing({ id: `wf-${Date.now().toString(36)}`, name: "", json: "" })}>
          Add a workflow
        </button>
      )}
    </Section>
  );
}

function FoldersSection({ draft, update }: SectionProps) {
  const pickFolder = async () => {
    const chosen = await open({ directory: true, multiple: false });
    if (typeof chosen === "string") update((s) => (s.outputDir = chosen));
  };
  const pickEditor = async () => {
    const chosen = await open({ directory: false, multiple: false });
    if (typeof chosen === "string") update((s) => (s.editorPath = chosen));
  };
  return (
    <Section title="Folders and editor" intro="Where results are saved, and which program “Open in editor” uses (for example PhotoCraft).">
      <div className="flex items-end gap-2">
        <div className="flex-1">
          <TextField id="output-dir" label="Save results in" value={draft.outputDir} placeholder="Pictures/Easel" onChange={(v) => update((s) => (s.outputDir = v))} />
        </div>
        <button type="button" className="btn" onClick={pickFolder}>
          Choose folder…
        </button>
      </div>
      <div className="flex items-end gap-2">
        <div className="flex-1">
          <TextField id="editor-path" label="Editor program" value={draft.editorPath} placeholder="/usr/bin/photocraft or C:\Program Files\PhotoCraft\photocraft.exe" onChange={(v) => update((s) => (s.editorPath = v))} />
        </div>
        <button type="button" className="btn" onClick={pickEditor}>
          Choose program…
        </button>
      </div>
    </Section>
  );
}

/** Runs one quick question through a single brain and shows how it went. */
function BrainTestButton({ kind }: { kind: BrainKind }) {
  const [state, setState] = useState<{ ok: boolean; text: string } | null>(null);
  const [busy, setBusy] = useState(false);
  const run = async () => {
    setBusy(true);
    setState(null);
    try {
      setState({ ok: true, text: await api.brainTest(kind) });
    } catch (err) {
      setState({ ok: false, text: errorMessage(err) });
    } finally {
      setBusy(false);
    }
  };
  return (
    <>
      <button type="button" className="btn h-8 text-xs" disabled={busy} onClick={run} title="Uses your saved settings: save first if you just changed them">
        {busy ? "Testing…" : "Test"}
      </button>
      {state && (
        <span role="status" className={`basis-full text-xs ${state.ok ? "text-ok" : "text-danger"}`}>
          {state.ok ? "Works: " : "Failed: "}
          {state.text}
        </span>
      )}
    </>
  );
}

/** Finds Ollama, LM Studio or llama-server running on this PC. */
function ServerDetector({ onPick }: { onPick: (url: string) => void }) {
  const [found, setFound] = useState<{ url: string; models: string[] }[] | null>(null);
  const [busy, setBusy] = useState(false);
  const run = async () => {
    setBusy(true);
    try {
      setFound(await api.detectLocalServers());
    } finally {
      setBusy(false);
    }
  };
  return (
    <div className="space-y-1">
      <button type="button" className="btn h-8 text-xs" disabled={busy} onClick={run}>
        {busy ? "Looking…" : "Detect running servers"}
      </button>
      {found && found.length === 0 && <p className="text-xs text-danger">Nothing answered on 11434 (Ollama), 1234 (LM Studio) or 8080 (llama-server).</p>}
      {found?.map((f) => (
        <button key={f.url} type="button" className="block text-left text-xs text-ink-2 underline hover:text-ink" onClick={() => onPick(f.url)}>
          Use {f.url} ({f.models.length} model{f.models.length === 1 ? "" : "s"})
        </button>
      ))}
    </div>
  );
}
