import { useState } from "react";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { api, appForPath, errorMessage, type GalleryItem } from "../lib/api";
import { useApp } from "../lib/store";
import { MediaThumb } from "./MediaThumb";

/** Fresh results with quick actions underneath each one. */
export function ResultGrid({ items }: { items: GalleryItem[] }) {
  const editImage = useApp((s) => s.editImage);
  const apps = useApp((s) => s.apps);
  const [error, setError] = useState<string | null>(null);
  if (items.length === 0) return null;

  const run = async (action: () => Promise<void>) => {
    setError(null);
    try {
      await action();
    } catch (err) {
      setError(errorMessage(err));
    }
  };

  return (
    <div className="space-y-2">
      <div className="grid grid-cols-[repeat(auto-fill,minmax(220px,1fr))] gap-3">
        {items.map((item) => (
          <figure key={item.path} className="card overflow-hidden">
            <MediaThumb item={item} className="aspect-square w-full bg-black object-contain" />
            <figcaption className="flex flex-wrap gap-1 p-2">
              {item.kind === "image" && !item.path.toLowerCase().endsWith(".svg") && (
                <button type="button" className="btn h-8 px-2 text-xs" onClick={() => editImage(item.path)}>
                  Edit this
                </button>
              )}
              <button type="button" className="btn h-8 px-2 text-xs" onClick={() => run(() => api.openInEditor(item.path))}>
                {openLabel(apps, item.path)}
              </button>
              <button type="button" className="btn h-8 px-2 text-xs" onClick={() => run(() => revealItemInDir(item.path))}>
                Show in folder
              </button>
            </figcaption>
          </figure>
        ))}
      </div>
      {notes(items).map((note) => (
        <p key={note} className="text-sm text-ink-2">
          {note}
        </p>
      ))}
      {error && (
        <p role="alert" className="text-sm text-danger">
          {error}
        </p>
      )}
    </div>
  );
}

/** Each distinct engine note once, such as settings Easel adjusted for the checkpoint. */
function notes(items: GalleryItem[]): string[] {
  return [...new Set(items.map((item) => item.meta?.note?.trim() ?? "").filter(Boolean))];
}

export function openLabel(apps: ReturnType<typeof useApp.getState>["apps"], path: string): string {
  const app = appForPath(apps, path);
  return app?.gui ? `Open in ${app.name}` : "Open in editor";
}
