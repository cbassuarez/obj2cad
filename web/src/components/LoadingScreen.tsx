import { Check } from "lucide-react";
import { useEffect, useState } from "react";
import { CubeArt } from "@/components/brand";
import { Button } from "@/components/ui/button";
import type { Progress } from "@/lib/engine";
import { bytes } from "@/lib/format";

type Stage = Progress["stage"];

/** The steps of opening a drawing, in order, as the person watching reads them. */
function steps(curves: boolean, dwg: boolean): { stage: Stage; label: string }[] {
  return [
    ...(dwg ? [{ stage: "engine" as const, label: "Loading the DWG writer" }] : []),
    { stage: "read", label: "Reading the files" },
    { stage: "parse", label: "Reading the model" },
    ...(curves ? [{ stage: "curves" as const, label: "Finding curved surfaces" }] : []),
    // The model is shown next; the file is written while it is (see App).
    { stage: "preview", label: "Preparing the 3D view" },
  ];
}

/** Seconds since `since`, updated every second, once `after` ms have passed. */
function useElapsed(since: number, after = 2500): number | null {
  const [now, setNow] = useState(() => performance.now());
  useEffect(() => {
    const t = setInterval(() => setNow(performance.now()), 1000);
    return () => clearInterval(t);
  }, []);
  const ms = now - since;
  return ms >= after ? Math.floor(ms / 1000) : null;
}

/** Opening a file: every step named as it happens, with progress where it can be counted,
 *  so a big file never looks frozen. */
export function LoadingScreen({
  name,
  progress,
  format,
  curves,
  startedAt,
  onCancel,
}: {
  name: string;
  progress: Progress | null;
  /** "DXF" or "DWG". */
  format: string;
  curves: boolean;
  startedAt: number;
  onCancel: () => void;
}) {
  const list = steps(curves, format === "DWG");
  const stage: Stage = progress?.stage ?? (format === "DWG" ? "engine" : "read");
  // Steps after the list's last (a file with nothing to show is written before it is
  // reported empty) count as the last.
  const found = list.findIndex((s) => s.stage === stage);
  const at = found < 0 ? list.length - 1 : found;
  const counted = progress && progress.total > 0 && (stage === "read" || stage === "parse");
  const frac = counted ? Math.min(progress.done / progress.total, 1) : null;
  const elapsed = useElapsed(startedAt);
  return (
    <main className="paper flex h-dvh items-center justify-center overflow-hidden px-4" role="status" aria-live="polite" aria-label={`Opening ${name}`}>
      <div className="panel flex w-full max-w-[420px] flex-col gap-5 px-8 py-7">
        <div className="flex items-center gap-3">
          <div className="animate-turn">
            <CubeArt className="size-9 text-accent" />
          </div>
          <div className="min-w-0 flex-1">
            <div className="truncate text-[14px] font-semibold" title={name}>
              {name}
            </div>
            <div className="num text-[12px] text-fg-3">{elapsed !== null ? `${elapsed} s` : " "}</div>
          </div>
        </div>
        <ol className="m-0 flex list-none flex-col gap-2 p-0">
          {list.map((s, i) => {
            const state = i < at ? "done" : i === at ? "active" : "pending";
            return (
              <li key={s.stage} className="flex flex-col gap-1.5" aria-current={state === "active" ? "step" : undefined}>
                <div className={`flex items-center gap-2.5 text-[13.5px] ${state === "pending" ? "text-fg-3" : state === "done" ? "text-fg-2" : "font-medium text-fg"}`}>
                  <span className="flex size-4 shrink-0 items-center justify-center" aria-hidden="true">
                    {state === "done" ? (
                      <Check className="size-4 text-accent" />
                    ) : state === "active" ? (
                      <span className="size-2 animate-blink rounded-full bg-accent" />
                    ) : (
                      <span className="size-1.5 rounded-full bg-line" />
                    )}
                  </span>
                  <span className="min-w-0 flex-1">{s.label}</span>
                  {state === "active" && counted && progress && (
                    <span className="num text-[12px] font-normal text-fg-3">
                      {bytes(progress.done)} / {bytes(progress.total)}
                    </span>
                  )}
                </div>
                {state === "active" && (
                  <div className="ml-[26px] h-1 overflow-hidden rounded-[2px] bg-panel-2">
                    {frac === null ? (
                      <div className="h-full w-1/3 animate-indeterminate bg-accent" />
                    ) : (
                      <div className="h-full bg-accent transition-[width] duration-200" style={{ width: `${Math.round(frac * 100)}%` }} />
                    )}
                  </div>
                )}
              </li>
            );
          })}
        </ol>
        <div className="flex justify-end">
          <Button variant="ghost" size="sm" onClick={onCancel}>
            Cancel
          </Button>
        </div>
      </div>
    </main>
  );
}
/** A file too large to be comfortable in a browser tab. */
export function PreflightScreen({ name, size, onContinue, onCancel, cliUrl }: { name: string; size: number; onContinue: () => void; onCancel: () => void; cliUrl: string }) {
  return (
    <main className="paper flex h-dvh items-center justify-center overflow-hidden px-4" role="alertdialog" aria-labelledby="preflight-title">
      <div className="panel flex w-full max-w-[520px] flex-col gap-4 p-6">
        <div id="preflight-title" className="text-[16px] font-semibold">
          {name} is {bytes(size)}
        </div>
        <div className="flex flex-wrap gap-3">
          <Button variant="primary" asChild>
            <a href={cliUrl} target="_blank" rel="noreferrer">
              Command-line version
            </a>
          </Button>
          <Button onClick={onContinue}>Convert here anyway</Button>
          <Button variant="ghost" onClick={onCancel}>
            Cancel
          </Button>
        </div>
      </div>
    </main>
  );
}
