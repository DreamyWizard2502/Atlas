// Contain render errors: one broken widget or preview must never blank the
// whole window. Each boundary shows its own fallback and can reset.
import { Component, type ErrorInfo, type ReactNode } from "react";

interface Props {
  fallback: (error: Error, reset: () => void) => ReactNode;
  /** Changing this clears the error (e.g. a different file or screen). */
  resetKey?: unknown;
  children: ReactNode;
}

export class ErrorBoundary extends Component<Props, { error: Error | null }> {
  state = { error: null as Error | null };

  static getDerivedStateFromError(error: Error) {
    return { error };
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    console.error("Atlas view error:", error, info.componentStack);
  }

  componentDidUpdate(prev: Props) {
    if (prev.resetKey !== this.props.resetKey && this.state.error) this.setState({ error: null });
  }

  render() {
    return this.state.error ? this.props.fallback(this.state.error, () => this.setState({ error: null })) : this.props.children;
  }
}
