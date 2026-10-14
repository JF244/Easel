import { useEffect, useRef, useState } from "react";
import { api, engineReady, errorMessage, type ChatTurn, type GalleryItem, type PlannedJob, type PlannerContext, type ProviderId } from "../lib/api";
import { useApp } from "../lib/store";
import { BrainNote, ErrorNote } from "../components/ErrorNote";
import { ImagePicker } from "../components/ImagePicker";
import { ResultGrid } from "../components/ResultGrid";

interface JobRun {
  job: PlannedJob;
  status: "running" | "done" | "failed";
  results: GalleryItem[];
  error?: string;
}

interface Message {
  id: number;
  role: "user" | "assistant";
  text: string;
  brain?: string;
  skipped?: string[];
  advisor?: string | null;
  advice?: string | null;
  runs?: JobRun[];
}

const RECENT_IMAGES = 10;
const HISTORY_KEY = "easel.brainChat.v1";
const HISTORY_LIMIT = 200;

/** The chat is kept in this window's storage so it survives restarting Easel. */
function loadHistory(): Message[] {
  try {
    const saved = JSON.parse(localStorage.getItem(HISTORY_KEY) ?? "[]") as Message[];
    // A job can't still be running after a restart.
    return saved.map((m) => ({
      ...m,
      runs: m.runs?.map((r) => (r.status === "running" ? { ...r, status: "failed", error: "Easel was closed before this finished." } : r)),
    }));
  } catch {
    return [];
  }
}

function saveHistory(messages: Message[]) {
  try {
    localStorage.setItem(HISTORY_KEY, JSON.stringify(messages.slice(-HISTORY_LIMIT)));
  } catch {
    // Storage full or blocked: the chat still works, it just won't survive a restart.
  }
}

const initialHistory = loadHistory();
let nextMessageId = initialHistory.reduce((max, m) => Math.max(max, m.id), 0) + 1;

export default function BrainPage() {
  const { settings, gallery, apps, addToGallery, setPage } = useApp();
  const [messages, setMessages] = useState<Message[]>(initialHistory);
  const [draft, setDraft] = useState("");
  const [attached, setAttached] = useState<string[]>([]);
  const [thinking, setThinking] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const bottomRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    bottomRef.current?.scrollIntoView({ behavior: "smooth" });
    saveHistory(messages);
  }, [messages, thinking]);

  const buildContext = (): PlannerContext => {
    const prompts = new Map(gallery.map((g) => [g.path, g.meta?.prompt ?? ""]));
    const recent = gallery.filter((g) => g.kind === "image" && !attached.includes(g.path)).slice(0, RECENT_IMAGES);
    // Editor jobs run through Claude Code, so only offer them when it's one of the brains.
    const claudeCodeOn = settings?.brain.order.includes("claudeCli") ?? false;
    return {
      geminiReady: engineReady(settings, "gemini") === null,
      comfyReady: engineReady(settings, "comfy") === null,
      workflows: settings?.comfy.workflows.map((w) => w.name) ?? [],
      editors: claudeCodeOn ? apps.filter((a) => a.cli).map((a) => ({ id: a.id, purpose: `${a.purpose}; replaces ${a.replaces}` })) : [],
      images: [
        ...attached.map((path) => ({ path, prompt: prompts.get(path) ?? "", attached: true })),
        ...recent.map((g) => ({ path: g.path, prompt: g.meta?.prompt ?? "", attached: false })),
      ],
    };
  };

  /** Turns a planned job into a generate request, refusing paths the brain made up. */
  const runJob = async (job: PlannedJob, allowedPaths: Set<string>): Promise<JobRun> => {
    const input = job.inputImage?.trim();
    if (input && !allowedPaths.has(input)) {
      return { job, status: "failed", results: [], error: "The brain pointed at an image that isn't in your gallery, so this job was skipped." };
    }
    if (job.engine === "editor") {
      const app = apps.find((a) => a.id === job.app);
      if (!app?.cli) return { job, status: "failed", results: [], error: `The editor "${job.app ?? ""}" isn't installed with its command-line tool. See the Apps page.` };
      try {
        const done = await api.runEditorJob({ app: app.id, instructions: job.prompt, inputPath: input || undefined });
        addToGallery([done.item]);
        return { job, status: "done", results: [done.item], error: undefined };
      } catch (err) {
        return { job, status: "failed", results: [], error: errorMessage(err) };
      }
    }

    let provider: ProviderId;
    let workflowId: string | undefined;
    if (job.engine === "gemini") provider = "gemini";
    else if (job.engine === "comfy") provider = "comfy";
    else if (job.engine === "workflow") {
      provider = "comfyWorkflow";
      const wf = settings?.comfy.workflows.find((w) => w.name === job.workflow || w.id === job.workflow);
      if (!wf) return { job, status: "failed", results: [], error: `There's no saved workflow called "${job.workflow ?? ""}".` };
      workflowId = wf.id;
    } else {
      return { job, status: "failed", results: [], error: `The brain asked for an engine Easel doesn't have ("${job.engine}").` };
    }
    try {
      const results = await api.generate({
        provider,
        workflowId,
        prompt: job.prompt,
        negative: job.negative ?? undefined,
        aspectRatio: job.aspectRatio ?? "1:1",
        count: job.count ?? 1,
        imagePaths: input ? [input] : [],
      });
      addToGallery(results);
      return { job, status: "done", results };
    } catch (err) {
      return { job, status: "failed", results: [], error: errorMessage(err) };
    }
  };

  const send = async () => {
    const text = draft.trim();
    if (!text || thinking) return;
    setError(null);
    const userMessage: Message = { id: nextMessageId++, role: "user", text: attached.length ? `${text}\n(attached ${attached.length} image${attached.length > 1 ? "s" : ""})` : text };
    const history = [...messages, userMessage];
    setMessages(history);
    setDraft("");
    setThinking(true);

    const context = buildContext();
    const turns: ChatTurn[] = history.map((m) => ({ role: m.role, text: m.text }));
    try {
      const plan = await api.brainPlan(turns, context);
      const runs: JobRun[] = plan.jobs.map((job) => ({ job, status: "running", results: [] }));
      const reply: Message = {
        id: nextMessageId++,
        role: "assistant",
        text: plan.reply || "Working on it.",
        brain: plan.brain,
        skipped: plan.skipped,
        advisor: plan.advisor,
        advice: plan.advice,
        runs,
      };
      setMessages([...history, reply]);
      setAttached([]);
      setThinking(false);

      const allowed = new Set(context.images.map((i) => i.path));
      // Run jobs one at a time so a local GPU isn't asked to do several at once.
      for (let i = 0; i < runs.length; i++) {
        const done = await runJob(runs[i].job, allowed);
        setMessages((current) =>
          current.map((m) => (m.id === reply.id ? { ...m, runs: m.runs?.map((r, j) => (j === i ? done : r)) } : m)),
        );
      }
    } catch (err) {
      setError(errorMessage(err));
      setThinking(false);
    }
  };

  const brainCount = settings?.brain.order.length ?? 0;

  return (
    <div className="mx-auto flex h-full max-w-4xl flex-col p-6">
      <header className="mb-4">
        <div className="flex items-center justify-between gap-3">
          <h1 className="text-xl font-semibold">Brain</h1>
          {messages.length > 0 && (
            <button type="button" className="btn h-8 text-xs" disabled={thinking} onClick={() => setMessages([])} title="Results stay in your gallery">
              Start a new chat
            </button>
          )}
        </div>
        <p className="text-sm text-ink-2">
          Say what you want made. The brain writes the prompts, picks Gemini or ComfyUI, and runs the jobs. If one brain is out of usage, the next one in Settings takes over.
        </p>
        {settings && brainCount === 0 && (
          <p className="mt-2 text-sm text-danger">
            No brain is turned on.{" "}
            <button type="button" className="underline" onClick={() => setPage("settings")}>
              Choose one in Settings
            </button>
            .
          </p>
        )}
      </header>

      <div className="flex-1 space-y-4 overflow-y-auto pb-4" aria-live="polite">
        {messages.length === 0 && (
          <div className="card p-6 text-sm text-ink-2">
            Try: “Make four cover images for a forklift safety course: bright, friendly, photo style, 16:9.” Then: “Make the second one at night.”
          </div>
        )}
        {messages.map((m) => (
          <div key={m.id} className={m.role === "user" ? "ml-auto max-w-[80%]" : "max-w-full"}>
            <div className={`rounded-lg px-4 py-3 text-sm whitespace-pre-wrap ${m.role === "user" ? "bg-accent/15 text-ink" : "card"}`}>{m.text}</div>
            {m.brain && (
              <div className="mt-1 px-1">
                <BrainNote brain={m.brain} skipped={m.skipped ?? []} />
              </div>
            )}
            {m.advisor && (
              <details className="mt-1 px-1 text-xs text-ink-3">
                <summary className="cursor-pointer">{m.advice ? `Advised by ${m.advisor}` : `Planned by the advisor, ${m.advisor}`}</summary>
                {m.advice && <p className="mt-1 whitespace-pre-wrap text-ink-2">{m.advice}</p>}
              </details>
            )}
            {m.runs?.map((run, j) => (
              <div key={j} className="mt-3 space-y-2">
                <p className="text-xs text-ink-3">
                  {run.status === "running" ? "Making" : run.status === "done" ? "Made" : "Couldn't make"} with {engineName(run.job, apps)}: <span className="text-ink-2">{run.job.prompt}</span>
                </p>
                {run.status === "running" && <div className="h-1 w-40 animate-pulse rounded bg-accent/60" aria-label="Working" />}
                {run.error && <ErrorNote message={run.error} />}
                <ResultGrid items={run.results} />
              </div>
            ))}
          </div>
        ))}
        {thinking && <p className="text-sm text-ink-3">The brain is planning…</p>}
        <ErrorNote message={error} />
        <div ref={bottomRef} />
      </div>

      <div className="card space-y-3 p-3">
        <ImagePicker label="Attach images to work from (optional)" paths={attached} onChange={setAttached} max={4} />
        <div className="flex gap-2">
          <label htmlFor="brain-input" className="sr-only">
            Message the brain
          </label>
          <textarea
            id="brain-input"
            rows={2}
            className="field h-auto flex-1 py-2"
            placeholder="Describe what you want… (start with /advisor to ask your frontier brain)"
            value={draft}
            onChange={(e) => setDraft(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && !e.shiftKey) {
                e.preventDefault();
                void send();
              }
            }}
          />
          <button type="button" className="btn btn-primary h-auto self-stretch px-5" disabled={!draft.trim() || thinking} onClick={() => void send()}>
            Send
          </button>
        </div>
      </div>
    </div>
  );
}

function engineName(job: PlannedJob, apps: { id: string; name: string }[]): string {
  if (job.engine === "editor") return `${apps.find((a) => a.id === job.app)?.name ?? job.app ?? "an editor"} (Claude Code doing the edit)`;
  if (job.engine === "workflow") return `your ComfyUI workflow "${job.workflow ?? ""}"`;
  if (job.engine === "comfy") return "ComfyUI";
  return "Gemini";
}
