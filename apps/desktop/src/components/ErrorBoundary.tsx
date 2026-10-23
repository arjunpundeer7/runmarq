import type { ErrorInfo, ReactNode } from "react";
import { Component } from "react";

interface State {
  error: Error | null;
}

/** Keeps a rendering failure in one page from blanking the whole window. */
export class ErrorBoundary extends Component<{ children: ReactNode; resetKey?: string }, State> {
  override state: State = { error: null };

  static getDerivedStateFromError(error: Error): State {
    return { error };
  }

  override componentDidCatch(error: Error, info: ErrorInfo) {
    console.error("runmarq UI error", error, info.componentStack);
  }

  override componentDidUpdate(prev: { resetKey?: string }) {
    if (prev.resetKey !== this.props.resetKey && this.state.error) this.setState({ error: null });
  }

  override render() {
    if (!this.state.error) return this.props.children;
    return (
      <div role="alert" className="rounded-lg bg-danger-muted px-4 py-3 text-[13px] text-danger">
        <p className="font-medium">This view could not be displayed.</p>
        <p className="mt-1 opacity-80">{this.state.error.message}</p>
        <button type="button" className="mt-2 underline underline-offset-2" onClick={() => this.setState({ error: null })}>
          Try again
        </button>
      </div>
    );
  }
}
