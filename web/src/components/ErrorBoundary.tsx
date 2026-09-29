import { Component, type ReactNode } from "react";
import { Button } from "@/components/ui/button";
import { problemUrl } from "@/lib/errors";

/**
 * Catches render errors, most importantly a lazily loaded chunk that no longer exists
 * because a new version was deployed while this tab was open.
 */
export class ErrorBoundary extends Component<{ children: ReactNode }, { error: Error | null }> {
  state = { error: null as Error | null };

  static getDerivedStateFromError(error: Error) {
    return { error };
  }

  render() {
    const { error } = this.state;
    if (!error) return this.props.children;
    const stale = /dynamically imported module|Loading chunk|Failed to fetch/i.test(error.message);
    return (
      <main className="paper flex h-dvh items-center justify-center overflow-hidden p-6" role="alert">
        <div className="panel flex max-w-md flex-col gap-3 p-6">
          <div className="text-[16px] font-semibold">{stale ? "obj2cad was updated" : "Something went wrong"}</div>
          <p className="m-0 text-[13.5px] text-fg-2">Reload the page.</p>
          {!stale && <pre className="num m-0 rounded-[3px] bg-panel-2 p-3 text-[12px] whitespace-pre-wrap text-fg-3">{error.message}</pre>}
          <div className="flex items-center gap-4">
            <Button variant="primary" onClick={() => location.reload()}>
              Reload
            </Button>
            {!stale && (
              <Button variant="link" asChild>
                <a href={problemUrl("Something went wrong", `${error.message}\n${error.stack ?? ""}`)} target="_blank" rel="noreferrer">
                  Report a problem
                </a>
              </Button>
            )}
          </div>
        </div>
      </main>
    );
  }
}
