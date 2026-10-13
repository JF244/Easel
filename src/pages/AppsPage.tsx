import { useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";
import { api, errorMessage, type AppStatus } from "../lib/api";
import { useApp } from "../lib/store";
import { ErrorNote } from "../components/ErrorNote";

export default function AppsPage() {
  const { apps, refreshApps } = useApp();
  const installed = apps.filter((a) => a.gui).length;

  return (
    <div className="mx-auto max-w-4xl space-y-5 p-6">
      <header className="flex flex-wrap items-end justify-between gap-3">
        <div>
          <h1 className="text-xl font-semibold">Apps</h1>
          <p className="text-sm text-ink-2">
            The Crafting Apps are free, open-source editors that stand in for the Adobe suite. Easel opens your files in the right one, and the Claude Code brain can use their tools to make
            exact edits. {installed} of {apps.length} found on this PC.
          </p>
        </div>
        <button type="button" className="btn" onClick={() => void refreshApps()}>
          Look again
        </button>
      </header>

      <ul className="space-y-3">
        {apps.map((app) => (
          <AppCard key={app.id} app={app} />
        ))}
      </ul>

      <p className="text-xs text-ink-3">
        Easel looks on your PATH, in the usual install folders, for AppImages in ~/Applications or Downloads, and for Flatpak installs. If an app isn't found, set its path by hand. The brain needs
        each app's command-line tool (for example photocraft-cli), which every installer includes.
      </p>
    </div>
  );
}

function AppCard({ app }: { app: AppStatus }) {
  const { settings, setSettings, refreshApps } = useApp();
  const [error, setError] = useState<string | null>(null);
  const [editing, setEditing] = useState(false);
  const [gui, setGui] = useState(settings?.apps[app.id]?.gui ?? "");
  const [cli, setCli] = useState(settings?.apps[app.id]?.cli ?? "");
  const [saving, setSaving] = useState(false);

  const run = async (action: () => Promise<void>) => {
    setError(null);
    try {
      await action();
    } catch (err) {
      setError(errorMessage(err));
    }
  };

  const pick = async (set: (v: string) => void) => {
    const chosen = await open({ multiple: false, directory: false });
    if (typeof chosen === "string") set(chosen);
  };

  const savePaths = async () => {
    if (!settings) return;
    setSaving(true);
    await run(async () => {
      const next = structuredClone(settings);
      next.apps[app.id] = { gui: gui.trim(), cli: cli.trim() };
      setSettings(await api.saveSettings(next));
      await refreshApps();
      setEditing(false);
    });
    setSaving(false);
  };

  return (
    <li className="card space-y-3 p-4">
      <div className="flex flex-wrap items-start gap-3">
        <div className="min-w-0 flex-1">
          <h2 className="font-semibold">
            {app.name} <span className="font-normal text-ink-3">· instead of {app.replaces}</span>
          </h2>
          <p className="text-sm text-ink-2">{app.purpose}</p>
          <p className="mt-1 text-xs">
            {app.gui ? <span className="text-ok">Installed</span> : <span className="text-ink-3">Not found</span>}
            {app.gui && (
              <span className={app.cli ? "text-ink-3" : "text-danger"}>
                {" "}
                · {app.cli ? "the brain can use it" : "command-line tool not found, so the brain can't use it yet"}
              </span>
            )}
          </p>
        </div>
        <div className="flex flex-wrap gap-2">
          {app.gui ? (
            <button type="button" className="btn btn-primary" onClick={() => run(() => api.openWithApp(app.id))}>
              Open {app.name}
            </button>
          ) : (
            <button type="button" className="btn btn-primary" onClick={() => run(() => openUrl(app.releasesUrl))}>
              Get {app.name}
            </button>
          )}
          <button type="button" className="btn" aria-expanded={editing} onClick={() => setEditing((e) => !e)}>
            Set path
          </button>
        </div>
      </div>

      {(app.gui || app.cli) && (
        <p className="truncate text-xs text-ink-3" title={[app.gui, app.cli].filter(Boolean).join("\n")}>
          {app.gui}
          {app.cli && app.cli !== app.gui ? ` · ${app.cli}` : ""}
        </p>
      )}

      {editing && (
        <div className="space-y-3 rounded-md border border-line p-3">
          <PathField id={`${app.id}-gui`} label={`${app.name} program`} value={gui} onChange={setGui} onPick={() => pick(setGui)} placeholder={`/path/to/${app.id} or ${app.name}.exe`} />
          <PathField id={`${app.id}-cli`} label="Command-line tool (optional; found next to the program if empty)" value={cli} onChange={setCli} onPick={() => pick(setCli)} placeholder={`${app.id}-cli`} />
          <button type="button" className="btn btn-primary h-8 text-xs" disabled={saving} onClick={savePaths}>
            {saving ? "Saving…" : "Save paths"}
          </button>
        </div>
      )}
      <ErrorNote message={error} />
    </li>
  );
}

function PathField({ id, label, value, onChange, onPick, placeholder }: { id: string; label: string; value: string; onChange: (v: string) => void; onPick: () => void; placeholder: string }) {
  return (
    <div>
      <label className="label" htmlFor={id}>
        {label}
      </label>
      <div className="flex gap-2">
        <input id={id} className="field" value={value} placeholder={placeholder} spellCheck={false} onChange={(e) => onChange(e.target.value)} />
        <button type="button" className="btn shrink-0" onClick={onPick}>
          Choose…
        </button>
      </div>
    </div>
  );
}
