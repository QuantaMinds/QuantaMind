import { useEffect, useRef, useState } from "react";
import { open as openUrl } from "@tauri-apps/plugin-shell";
import { DISCORD_INVITE_URL, GITHUB_REPO_URL, X_PROFILE_URL } from "../links";
import { useCommunityStore } from "../state/communityStore";

function DiscordMark() {
  return (
    <svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor" aria-hidden>
      <path d="M20.317 4.37a19.79 19.79 0 0 0-4.885-1.515.074.074 0 0 0-.079.037c-.21.375-.444.865-.608 1.25a18.27 18.27 0 0 0-5.487 0 12.64 12.64 0 0 0-.617-1.25.077.077 0 0 0-.079-.037A19.736 19.736 0 0 0 3.677 4.37a.07.07 0 0 0-.032.027C.533 9.046-.32 13.58.099 18.058a.082.082 0 0 0 .031.056 19.9 19.9 0 0 0 5.993 3.03.078.078 0 0 0 .084-.028 14.09 14.09 0 0 0 1.226-1.994.076.076 0 0 0-.041-.106 13.107 13.107 0 0 1-1.872-.892.077.077 0 0 1-.008-.128c.126-.094.252-.192.372-.291a.074.074 0 0 1 .077-.01c3.928 1.793 8.18 1.793 12.062 0a.074.074 0 0 1 .078.009c.12.099.246.198.373.292a.077.077 0 0 1-.006.127 12.299 12.299 0 0 1-1.873.892.077.077 0 0 0-.041.107c.36.698.772 1.362 1.225 1.993a.076.076 0 0 0 .084.028 19.839 19.839 0 0 0 6.002-3.03.077.077 0 0 0 .032-.054c.5-5.177-.838-9.674-3.549-13.66a.061.061 0 0 0-.031-.03zM8.02 15.33c-1.183 0-2.157-1.085-2.157-2.419 0-1.333.956-2.419 2.157-2.419 1.21 0 2.176 1.096 2.157 2.42 0 1.333-.956 2.418-2.157 2.418zm7.975 0c-1.183 0-2.157-1.085-2.157-2.419 0-1.333.955-2.419 2.157-2.419 1.21 0 2.176 1.096 2.157 2.42 0 1.333-.946 2.418-2.157 2.418Z" />
    </svg>
  );
}

/// Header "Discord" button + a one-time invite popover. Both only open links in
/// the OS browser (scoped shell allowlist) — nothing is sent or collected
/// (docs/security.md#trust-boundaries). The popover auto-opens once per install
/// (user_settings.community_prompt_shown), and only while online, since the
/// links are useless offline.
export function CommunityButton() {
  const [open, setOpen] = useState(false);
  const promptShown = useCommunityStore((s) => s.promptShown);
  const load = useCommunityStore((s) => s.load);
  const markShown = useCommunityStore((s) => s.markShown);
  const autoOpened = useRef(false);

  useEffect(() => {
    void load();
  }, [load]);

  useEffect(() => {
    if (promptShown === false && navigator.onLine && !autoOpened.current) {
      autoOpened.current = true;
      setOpen(true);
    }
  }, [promptShown]);

  const dismiss = () => {
    setOpen(false);
    if (promptShown === false) void markShown();
  };

  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") dismiss();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  const go = (url: string) => {
    void openUrl(url);
    dismiss();
  };

  return (
    <div className="relative" data-testid="community-control">
      <button
        type="button"
        onClick={() => (open ? dismiss() : setOpen(true))}
        title="Join the QuantaMind Discord"
        data-testid="community-button"
        className="flex items-center gap-1.5 border rounded px-2 py-1 text-sm text-gray-600 hover:text-ink"
      >
        <DiscordMark />
        <span>Discord</span>
      </button>
      {open && (
        <div
          role="dialog"
          aria-label="Join the community"
          data-testid="community-popover"
          className="absolute right-0 z-20 mt-1 w-72 bg-surface border rounded-lg shadow-lg p-3 text-left"
        >
          <div className="text-sm font-semibold mb-1">Help build QuantaMind</div>
          <p className="text-xs text-gray-600 mb-2">
            This tool is shaped by user feedback — join the Discord to tell us what works and
            what to build next, or use the Feedback button (bottom right) any time. If
            QuantaMind is useful to you, starring it on GitHub helps others find it. ⭐
          </p>
          <p className="text-xs text-gray-400 mb-2">
            These buttons only open your browser. Nothing about you or your machine is sent.
          </p>
          <div className="flex flex-wrap items-center gap-2 justify-end">
            <button
              type="button"
              onClick={dismiss}
              data-testid="community-dismiss"
              className="text-xs px-3 py-1 border rounded hover:bg-gray-50"
            >
              Got it
            </button>
            <button
              type="button"
              onClick={() => go(X_PROFILE_URL)}
              data-testid="community-x"
              className="text-xs px-3 py-1 border rounded hover:bg-gray-50"
            >
              Follow on X
            </button>
            <button
              type="button"
              onClick={() => go(GITHUB_REPO_URL)}
              data-testid="community-github"
              className="text-xs px-3 py-1 border rounded hover:bg-gray-50"
            >
              ⭐ Star on GitHub
            </button>
            <button
              type="button"
              onClick={() => go(DISCORD_INVITE_URL)}
              data-testid="community-discord"
              className="text-xs px-3 py-1 bg-blue-600 text-white rounded hover:bg-blue-700"
            >
              Join the Discord
            </button>
          </div>
        </div>
      )}
    </div>
  );
}
