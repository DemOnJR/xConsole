import { Component, type ErrorInfo, type ReactNode } from "react";

/** Keeps a single canvas node crash from blanking the whole window. */
export class NodeErrorBoundary extends Component<
  { label: string; children: ReactNode },
  { error: string | null }
> {
  state: { error: string | null } = { error: null };

  static getDerivedStateFromError(error: Error) {
    return { error: error.message || String(error) };
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    console.error(`[${this.props.label} node]`, error, info.componentStack);
  }

  render() {
    if (this.state.error) {
      return (
        <div className="flex h-full w-full items-center justify-center rounded-lg border border-red-900/40 bg-[var(--bg)] px-3 text-center text-[11px] text-red-300">
          {this.props.label} failed to render.
        </div>
      );
    }
    return this.props.children;
  }
}
