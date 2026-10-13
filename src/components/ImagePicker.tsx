import { useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { fileUrl } from "../lib/api";
import { useApp } from "../lib/store";

const IMAGE_FILTER = [{ name: "Images", extensions: ["png", "jpg", "jpeg", "webp"] }];

/** Choose images from the gallery or from anywhere on disk. */
export function ImagePicker({
  label,
  paths,
  onChange,
  max,
}: {
  label: string;
  paths: string[];
  onChange: (paths: string[]) => void;
  max: number;
}) {
  const gallery = useApp((s) => s.gallery);
  const [browsing, setBrowsing] = useState(false);
  const full = paths.length >= max;

  const add = (path: string) => {
    if (!paths.includes(path)) onChange([...paths, path].slice(-max));
    setBrowsing(false);
  };

  const pickFile = async () => {
    const chosen = await open({ multiple: false, directory: false, filters: IMAGE_FILTER });
    if (typeof chosen === "string") add(chosen);
  };

  return (
    <div>
      <span className="label">{label}</span>
      <div className="flex flex-wrap items-center gap-2">
        {paths.map((path) => (
          <div key={path} className="relative">
            <img src={fileUrl(path)} alt="" className="h-16 w-16 rounded-md border border-line object-cover" />
            <button
              type="button"
              aria-label="Remove this image"
              className="absolute -top-2 -right-2 h-6 w-6 rounded-full border border-line bg-panel text-xs"
              onClick={() => onChange(paths.filter((p) => p !== path))}
            >
              ×
            </button>
          </div>
        ))}
        {!full && (
          <>
            <button type="button" className="btn h-8 text-xs" onClick={() => setBrowsing((b) => !b)}>
              From gallery
            </button>
            <button type="button" className="btn h-8 text-xs" onClick={pickFile}>
              From a file
            </button>
          </>
        )}
      </div>
      {browsing && (
        <div className="mt-2 grid max-h-56 grid-cols-[repeat(auto-fill,minmax(72px,1fr))] gap-2 overflow-y-auto rounded-md border border-line p-2">
          {gallery.filter((g) => g.kind === "image" && !g.path.toLowerCase().endsWith(".svg")).length === 0 && <p className="col-span-full text-xs text-ink-3">Nothing in the gallery yet.</p>}
          {gallery
            .filter((g) => !g.isVideo)
            .slice(0, 60)
            .map((g) => (
              <button key={g.path} type="button" onClick={() => add(g.path)} aria-label={`Use ${g.fileName}`}>
                <img src={fileUrl(g.path)} alt="" loading="lazy" className="aspect-square w-full rounded object-cover hover:ring-2 hover:ring-accent" />
              </button>
            ))}
        </div>
      )}
    </div>
  );
}
