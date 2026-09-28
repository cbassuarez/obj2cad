import { Component, type ReactNode } from "react";
import { Button } from "@/components/ui/button";

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
      <main className="paper flex h-full items-center justify-center p-6" role="alert">
        <div className="panel flex max-w-md flex-col gap-3 p-6">
          <div className="text-[16px] font-semibold">{stale ? "obj2cad was just updated" : "Something went wrong"}</div>
          <p className="m-0 text-[13.5px] text-fg-2">
            {stale
              ? "Reload to get the new version. Your files never left this computer, so just drop them again."
              : "Reload and try again. If it keeps happening, please report it with the file that caused it."}
          </p>
          {!stale && <pre className="num m-0 rounded-[3px] bg-panel-2 p-3 text-[12px] whitespace-pre-wrap text-fg-3">{error.message}</pre>}
          <div>
            <Button variant="primary" onClick={() => location.reload()}>
              Reload
            </Button>
          </div>
        </div>
      </main>
    );
  }
}
