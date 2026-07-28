import { BackendSelector } from "./BackendSelector";
import { ModelSelector } from "./ModelSelector";
import { ParamsControl } from "./ParamsControl";
import { ServerControl } from "./features/workspace/components/status/ServerControl";
import { SttHeaderControl } from "./features/stt/components/SttHeaderControl";
import { CommunityButton } from "./features/community/components/CommunityButton";

/// The global header controls, shown on every view. Two independent groups so
/// one LLM and one STT run in parallel: the LLM group (backend dropdown, model
/// picker, params popover, play/stop) and the Speech-to-Text group (model
/// dropdown + play/stop), plus the community (Discord) button. Composed at the
/// shell level (features don't import each other) — see WorkspaceSidebar.
export function GlobalControls() {
  return (
    <div className="flex items-center gap-2" data-testid="global-controls">
      <ServerControl />
      <BackendSelector />
      <ModelSelector />
      <ParamsControl />
      <span className="mx-1 h-5 w-px bg-gray-200" aria-hidden />
      <SttHeaderControl />
      <CommunityButton />
    </div>
  );
}
