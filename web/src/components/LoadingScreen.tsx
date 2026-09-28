import { motion } from "motion/react";
import { CubeArt } from "@/components/brand";
import { Button } from "@/components/ui/button";
import type { Progress } from "@/lib/engine";
import { bytes } from "@/lib/format";

const STAGE: Record<Progress["stage"], string> = { read: "Reading", parse: "Reading" };

/** Opening a file: reading it, then parsing it, with progress for large files. */
export function LoadingScreen({ name, progress }: { name: string; progress: Progress | null }) {
  // Reading is the first half of the bar, parsing the second.
  const frac = progress ? (progress.stage === "read" ? 0 : 0.5) + (progress.done / Math.max(progress.total, 1)) * 0.5 : null;
  return (
    <main className="paper flex min-h-dvh items-center justify-center px-4" role="status" aria-live="polite">
      <div className="panel flex w-full max-w-[420px] flex-col items-center gap-4 px-10 py-8">
        <motion.div animate={{ rotate: [0, 0, 120, 120] }} transition={{ duration: 1.8, repeat: Infinity, times: [0, 0.3, 0.7, 1] }}>
          <CubeArt className="size-12 text-accent" />
        </motion.div>
        <div className="max-w-full truncate text-[14px] text-fg-2">
          {progress ? STAGE[progress.stage] : "Opening"} <span className="num text-fg">{name}</span>
        </div>
        <div className="h-1.5 w-full overflow-hidden rounded-[2px] bg-panel-2">
          {frac === null ? (
            <motion.div className="h-full w-1/3 bg-accent" animate={{ x: ["-100%", "300%"] }} transition={{ duration: 1.4, repeat: Infinity, ease: "easeInOut" }} />
          ) : (
            <div className="h-full bg-accent transition-[width] duration-200" style={{ width: `${Math.round(frac * 100)}%` }} />
          )}
        </div>
        {progress && (
          <div className="num text-[12px] text-fg-3">
            {bytes(progress.done)} / {bytes(progress.total)}
          </div>
        )}
      </div>
    </main>
  );
}

/** A file too large to be comfortable in a browser tab. */
export function PreflightScreen({ name, size, onContinue, onCancel, cliUrl }: { name: string; size: number; onContinue: () => void; onCancel: () => void; cliUrl: string }) {
  return (
    <main className="paper flex min-h-dvh items-center justify-center px-4" role="alertdialog" aria-labelledby="preflight-title">
      <div className="panel flex w-full max-w-[520px] flex-col gap-4 p-6">
        <div id="preflight-title" className="text-[16px] font-semibold">
          {name} is {bytes(size)}
        </div>
        <p className="m-0 text-[14px] text-fg-2">Files this large convert more reliably with the command-line version.</p>
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
