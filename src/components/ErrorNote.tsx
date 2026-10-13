export function ErrorNote({ message }: { message: string | null }) {
  if (!message) return null;
  return (
    <p role="alert" className="rounded-md border border-danger/40 bg-danger/10 px-3 py-2 text-sm text-danger">
      {message}
    </p>
  );
}

/** Which brain answered, and which were skipped on the way. */
export function BrainNote({ brain, skipped }: { brain: string; skipped: string[] }) {
  return (
    <p className="text-xs text-ink-3">
      Answered by {brain}
      {skipped.length > 0 && (
        <span title={skipped.join("\n")}>
          {" "}
          (skipped {skipped.length}: {skipped.map((s) => s.split(":")[0]).join(", ")})
        </span>
      )}
    </p>
  );
}
