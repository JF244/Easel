import { fileUrl, type GalleryItem } from "../lib/api";

export function MediaThumb({ item, className = "" }: { item: GalleryItem; className?: string }) {
  const alt = item.meta?.prompt ? item.meta.prompt.slice(0, 120) : item.fileName;
  if (item.isVideo) {
    return <video src={fileUrl(item.path)} className={className} muted loop playsInline controls aria-label={alt} />;
  }
  return <img src={fileUrl(item.path)} alt={alt} loading="lazy" className={className} />;
}
