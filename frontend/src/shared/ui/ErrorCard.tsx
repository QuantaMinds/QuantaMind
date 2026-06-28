import { Component, Fragment, type ErrorInfo, type ReactNode } from "react";
import { open } from "@tauri-apps/plugin-shell";

type Action = { label: string; onClick: () => void };

type Props = {
  title: string;
  body: string;
  learnMore?: string;
  action?: Action;
};

/// Consistent, actionable error surface: a title, helpful body, an optional
/// primary action (Retry / Open Settings / …), and an optional "Learn more"
/// link that opens the docs in the system browser.
export function ErrorCard({ title, body, learnMore, action }: Props) {
  return (
    <div
      role="alert"
      data-testid="error-card"
      className="border border-red-300 bg-red-50 rounded p-3 text-sm flex flex-col gap-2"
    >
      <div className="font-medium text-red-700">{title}</div>
      <div className="text-gray-700">{body}</div>
      {(action || learnMore) && (
        <div className="flex gap-3 items-center">
          {action && (
            <button
              type="button"
              onClick={action.onClick}
              className="border rounded px-3 py-1 bg-surface hover:bg-gray-50"
              data-testid="error-action"
            >
              {action.label}
            </button>
          )}
          {learnMore && (
            <button
              type="button"
              onClick={() => void open(learnMore)}
              className="text-blue-700 hover:underline"
              data-testid="error-learn-more"
            >
              Learn more
            </button>
          )}
        </div>
      )}
    </div>
  );
}

interface BoundaryProps {
  /// Identifies the wrapped panel in both the fallback card and the log, so a
  /// scoped throw is traceable to its tab.
  label?: string;
  children: ReactNode;
}

interface BoundaryState {
  error: Error | null;
  /// Bumped on reset to force the wrapped subtree to remount fresh (a boundary that
  /// catches but never remounts would leave the user parked on the error card).
  resetKey: number;
}

/// Panel-scoped error boundary, co-located with the `ErrorCard` it renders as its
/// fallback. Every tab is always-mounted (`App` renders them all behind `hidden=`),
/// so an unbounded render throw unmounts the WHOLE app — this degrades it to a card
/// for the one panel instead. It ALWAYS logs the real error + component stack (the
/// card is UX only; the throw must stay diagnosable), and its Retry remounts the subtree.
export class ErrorBoundary extends Component<BoundaryProps, BoundaryState> {
  state: BoundaryState = { error: null, resetKey: 0 };

  static getDerivedStateFromError(error: Error): Partial<BoundaryState> {
    return { error };
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    // Never swallow — log the underlying throw + stack so the bug is findable.
    console.error(`[ErrorBoundary${this.props.label ? ` · ${this.props.label}` : ""}]`, error, info.componentStack);
  }

  private reset = () => this.setState((s) => ({ error: null, resetKey: s.resetKey + 1 }));

  render() {
    const { error } = this.state;
    if (error) {
      return (
        <div data-testid="error-boundary-fallback" style={{ margin: 16 }}>
          <ErrorCard
            title={`This panel hit an error${this.props.label ? ` (${this.props.label})` : ""}.`}
            body={error.message || String(error)}
            action={{ label: "Retry", onClick: this.reset }}
          />
        </div>
      );
    }
    // Keying on `resetKey` makes Retry remount the children; a normal re-render keeps
    // the key stable, so it never remounts spuriously.
    return <Fragment key={this.state.resetKey}>{this.props.children}</Fragment>;
  }
}
