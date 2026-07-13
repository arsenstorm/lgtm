import { RiErrorWarningLine, RiRestartLine } from "@remixicon/react";
import { Component, type ReactNode } from "react";
import { Button } from "@/components/ui/button";

type ErrorBoundaryProps = { children: ReactNode };
type ErrorBoundaryState = { error: Error | null };

/**
 * Last-resort catch for render errors so a crash shows a recoverable screen
 * instead of a blank window. Class component: React still has no hook
 * equivalent of getDerivedStateFromError.
 */
// biome-ignore lint/style/useReactFunctionComponents: error boundaries require a class component
export class ErrorBoundary extends Component<
  ErrorBoundaryProps,
  ErrorBoundaryState
> {
  state: ErrorBoundaryState = { error: null };

  static getDerivedStateFromError(error: Error): ErrorBoundaryState {
    return { error };
  }

  render() {
    if (this.state.error) {
      return <CrashScreen error={this.state.error} />;
    }
    return this.props.children;
  }
}

function CrashScreen({ error }: { error: Error }) {
  return (
    <div className="flex h-screen flex-col items-center justify-center gap-4 bg-background p-8 text-center">
      <div className="flex size-12 items-center justify-center rounded-2xl bg-destructive/10 text-destructive">
        <RiErrorWarningLine aria-hidden className="size-6" />
      </div>
      <div className="flex flex-col gap-1">
        <h1 className="font-medium text-sm">Something went wrong</h1>
        <p className="max-w-md text-muted-foreground text-sm">
          LGTM hit an unexpected error. Your repository was not touched — LGTM
          never writes to it.
        </p>
      </div>
      <pre className="max-h-40 max-w-lg overflow-auto rounded-md bg-muted p-3 text-left font-mono text-muted-foreground text-xs">
        {error.message || String(error)}
      </pre>
      <Button onClick={() => window.location.reload()} type="button">
        <RiRestartLine aria-hidden />
        Reload
      </Button>
    </div>
  );
}
