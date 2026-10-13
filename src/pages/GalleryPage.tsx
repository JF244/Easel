import { useEffect, useMemo, useState } from "react";
import { openPath, revealItemInDir } from "@tauri-apps/plugin-opener";
import { api, errorMessage, type GalleryItem } from "../lib/api";
import { useApp } from "../lib/store";
import { ErrorNote } from "../components/ErrorNote";
import { MediaThumb } from "../components/MediaThumb";
import { openLabel } from "../components/ResultGrid";

export default function GalleryPage() {
  const { gallery, apps, refreshGallery, removeFromGallery, editImage } = useApp();
  const [folder, setFolder] = useState("");
  const [search, setSearch] = useState("");
  const [selected, setSelected] = useState<GalleryItem | null>(null);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    void refreshGallery();
    api.outputDir().then(setFolder).catch(() => setFolder(""));
  }, [refreshGallery]);

  const shown = useMemo(() => {
    const q = search.trim().toLowerCase();
    if (!q) return gallery;
    return gallery.filter((g) => (g.meta?.prompt ?? "").toLowerCase().includes(q) || g.fileName.toLowerCase().includes(q));
  }, [gallery, search]);

  const run = async (action: () => Promise<void>) => {
    setError(null);
    try {
      await action();
    } catch (err) {
      setError(errorMessage(err));
    }
  };

  const remove = async (item: GalleryItem) => {
    await run(async () => {
      await api.galleryDelete(item.path);
      removeFromGallery(item.path);
      setSelected(null);
      setConfirmDelete(false);
    });
  };

  const copyPrompt = async (text: string) => {
    await run(async () => {
      await navigator.clipboard.writeText(text);
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    });
  };

  return (
    <div className="space-y-4 p-6">
      <header className="flex flex-wrap items-end justify-between gap-3">
        <div>
          <h1 className="text-xl font-semibold">Gallery</h1>
          <p className="text-sm text-ink-2">
            {gallery.length} item{gallery.length === 1 ? "" : "s"} in <span className="text-ink">{folder || "your output folder"}</span>
          </p>
        </div>
        <div className="flex gap-2">
          <label htmlFor="gallery-search" className="sr-only">
            Search prompts
          </label>
          <input id="gallery-search" className="field w-64" placeholder="Search prompts" value={search} onChange={(e) => setSearch(e.target.value)} />
          <button type="button" className="btn" onClick={() => run(() => refreshGallery())}>
            Refresh
          </button>
          {folder && (
            <button type="button" className="btn" onClick={() => run(() => openPath(folder))}>
              Open folder
            </button>
          )}
        </div>
      </header>

      <ErrorNote message={error} />

      {shown.length === 0 ? (
        <div className="card p-10 text-center text-ink-2">{gallery.length === 0 ? "Nothing here yet. Make something on the Brain or Create page." : "No prompts match that search."}</div>
      ) : (
        <div className="grid grid-cols-[repeat(auto-fill,minmax(180px,1fr))] gap-3">
          {shown.map((item) => (
            <button
              key={item.path}
              type="button"
              className="card overflow-hidden text-left hover:border-ink-3"
              onClick={() => {
                setSelected(item);
                setConfirmDelete(false);
              }}
              aria-label={`Open ${item.meta?.prompt?.slice(0, 60) || item.fileName}`}
            >
              {item.isVideo ? (
                <div className="flex aspect-square items-center justify-center bg-black text-xs text-ink-2">Video: {item.fileName}</div>
              ) : (
                <MediaThumb item={item} className="aspect-square w-full bg-black object-cover" />
              )}
            </button>
          ))}
        </div>
      )}

      {selected && (
        <div role="dialog" aria-modal="true" aria-label="Image details" className="fixed inset-0 z-10 flex bg-black/80 p-6" onClick={() => setSelected(null)}>
          <div className="m-auto flex max-h-full w-full max-w-6xl gap-4" onClick={(e) => e.stopPropagation()}>
            <div className="flex min-w-0 flex-1 items-center justify-center">
              <MediaThumb item={selected} className="max-h-[85vh] max-w-full rounded-md object-contain" />
            </div>
            <aside className="card w-80 shrink-0 space-y-3 overflow-y-auto p-4 text-sm">
              <div className="flex items-start justify-between gap-2">
                <h2 className="font-semibold break-all">{selected.fileName}</h2>
                <button type="button" className="btn h-8 w-8 px-0" aria-label="Close" onClick={() => setSelected(null)}>
                  ×
                </button>
              </div>
              {selected.meta ? (
                <dl className="space-y-2">
                  <div>
                    <dt className="label">Prompt</dt>
                    <dd className="whitespace-pre-wrap">{selected.meta.prompt || "(none)"}</dd>
                  </div>
                  <div className="flex gap-4">
                    <div>
                      <dt className="label">Engine</dt>
                      <dd>{selected.meta.provider}</dd>
                    </div>
                    <div>
                      <dt className="label">Kind</dt>
                      <dd>{selected.meta.mode}</dd>
                    </div>
                    {selected.meta.seed != null && (
                      <div>
                        <dt className="label">Seed</dt>
                        <dd>{selected.meta.seed}</dd>
                      </div>
                    )}
                  </div>
                  <div>
                    <dt className="label">Model</dt>
                    <dd className="break-all">{selected.meta.model}</dd>
                  </div>
                  {selected.meta.note && (
                    <div>
                      <dt className="label">Model's note</dt>
                      <dd className="whitespace-pre-wrap text-ink-2">{selected.meta.note}</dd>
                    </div>
                  )}
                </dl>
              ) : (
                <p className="text-ink-2">No prompt was saved with this file.</p>
              )}
              <div className="flex flex-wrap gap-2 pt-2">
                {selected.kind === "image" && !selected.path.toLowerCase().endsWith(".svg") && (
                  <button type="button" className="btn btn-primary" onClick={() => editImage(selected.path)}>
                    Edit this
                  </button>
                )}
                {selected.meta?.prompt && (
                  <button type="button" className="btn" onClick={() => copyPrompt(selected.meta!.prompt)}>
                    {copied ? "Copied" : "Copy prompt"}
                  </button>
                )}
                <button type="button" className="btn" onClick={() => run(() => api.openInEditor(selected.path))}>
                  {openLabel(apps, selected.path)}
                </button>
                <button type="button" className="btn" onClick={() => run(() => revealItemInDir(selected.path))}>
                  Show in folder
                </button>
                {confirmDelete ? (
                  <button type="button" className="btn btn-danger" onClick={() => remove(selected)}>
                    Yes, delete it
                  </button>
                ) : (
                  <button type="button" className="btn btn-danger" onClick={() => setConfirmDelete(true)}>
                    Delete
                  </button>
                )}
              </div>
              <ErrorNote message={error} />
            </aside>
          </div>
        </div>
      )}
    </div>
  );
}
