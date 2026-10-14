import { useEffect, useState } from "react";
import { api, type Preflight, type ProviderId } from "../lib/api";

/**
 * Before a ComfyUI job, asks Easel whether this GPU and ComfyUI's PyTorch fit
 * together, and shows any problem in plain words. It never blocks the button:
 * the check can be wrong, and you may know better.
 */
export function ComfyPreflight({ provider }: { provider: ProviderId }) {
  const [result, setResult] = useState<Preflight | null>(null);
  const comfy = provider !== "gemini";

  useEffect(() => {
    if (!comfy) return;
    let alive = true;
    api
      .comfyPreflight()
      .then((r) => alive && setResult(r))
      .catch(() => alive && setResult(null));
    return () => {
      alive = false;
    };
  }, [comfy]);

  if (!comfy || !result || result.warnings.length === 0) return null;
  return (
    <div role="status" className="space-y-1 rounded-md border border-accent/40 bg-accent/10 px-3 py-2 text-sm text-ink">
      {result.warnings.map((w) => (
        <p key={w}>{w}</p>
      ))}
      {(result.device || result.torch) && (
        <p className="text-xs text-ink-3">
          ComfyUI reports: {[result.device, result.torch && `PyTorch ${result.torch}`, result.comfyVersion && `ComfyUI ${result.comfyVersion}`].filter(Boolean).join(" · ")}
        </p>
      )}
    </div>
  );
}
