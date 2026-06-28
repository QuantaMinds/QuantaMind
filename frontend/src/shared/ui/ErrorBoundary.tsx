import { Component, Fragment, type ErrorInfo, type ReactNode } from "react";
import { ErrorCard } from "./ErrorCard";

interface Props {
  /// Identifies the wrapped panel in both the fallback card and the log, so a
  /// scoped throw is traceable to its tab.
  label?: string;
  children: ReactNode;
}

interface State {
  error: Error | null;
  /// Bumped on reset to force the wrapped subtree to remount fresh (a boundary that
  /// catches but never remounts would leave the user parked on the error card).
  resetKey: number;
}

/// Panel-scoped error boundary. Every tab is always-mounted (`App` renders them all
/// behind `hidden=`), so an unbounded render throw unmounts the WHOLE app — this
/// degrades it to a compact card for the one panel instead. It ALWAYS logs the real
/// error + component stack (the card is UX only; the throw must stay diagnosable),
/// and its Retry remounts the subtree.
export class ErrorBoundary extends Component<Props, State> {
  state: State = { error: null, resetKey: 0 };

  static getDerivedStateFromError(error: Error): Partial<State> {
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
      // Reuse the app's consistent error surface; Retry remounts the subtree.
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
