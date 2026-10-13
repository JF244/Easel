import { extensionOf, fileUrl, type GalleryItem } from "../lib/api";

export function MediaThumb({ item, className = "" }: { item: GalleryItem; className?: string }) {
  const alt = item.meta?.prompt ? item.meta.prompt.slice(0, 120) : item.fileName;
  if (item.kind === "document") {
    // PSDs, PDFs and raw photos need their app to view; show what the file is instead.
    return (
      <div className={`flex flex-col items-center justify-center gap-1 bg-panel-2 text-ink-2 ${className}`} role="img" aria-label={`${extensionOf(item.path)} file ${item.fileName}`}>
        <span className="rounded border border-line px-2 py-1 text-sm font-semibold text-ink">{extensionOf(item.path)}</span>
        <span className="max-w-[90%] truncate text-xs">{item.fileName}</span>
      </div>
    );
  }
  if (item.isVideo) {
    return <video src={fileUrl(item.path)} className={className} muted loop playsInline controls aria-label={alt} />;
  }
  return <img src={fileUrl(item.path)} alt={alt} loading="lazy" className={className} />;
}
