import type { ProviderId, Settings } from "../lib/api";

export interface EngineChoice {
  provider: ProviderId;
  workflowId?: string;
}

export function engineKey(choice: EngineChoice): string {
  return choice.provider === "comfyWorkflow" ? `workflow:${choice.workflowId}` : choice.provider;
}

export function EngineSelect({
  id,
  settings,
  value,
  onChange,
}: {
  id: string;
  settings: Settings | null;
  value: EngineChoice;
  onChange: (choice: EngineChoice) => void;
}) {
  const workflows = settings?.comfy.workflows ?? [];
  return (
    <div>
      <label className="label" htmlFor={id}>
        Engine
      </label>
      <select
        id={id}
        className="field"
        value={engineKey(value)}
        onChange={(e) => {
          const v = e.target.value;
          if (v.startsWith("workflow:")) onChange({ provider: "comfyWorkflow", workflowId: v.slice("workflow:".length) });
          else onChange({ provider: v as ProviderId });
        }}
      >
        <option value="gemini">Gemini / Nano Banana (your API key)</option>
        <option value="comfy">ComfyUI on your PC</option>
        {workflows.map((w) => (
          <option key={w.id} value={`workflow:${w.id}`}>
            ComfyUI workflow: {w.name}
          </option>
        ))}
      </select>
    </div>
  );
}
