// The station rail: opening a drawing as the steps it really takes, in order. Each
// station shows its own measure: a bar only for what is counted against a known total,
// three dots taking turns for a step with nothing to count. A halo marks what is running;
// a tick draws in when a step is done, and the line to the next station turns green.
import { ArrowDownToLine, Box, Cpu, Hash, ListTree, PenLine, Spline, type LucideIcon } from "lucide-react";
import { motion } from "motion/react";
import { Button } from "@/components/ui/button";
import { runLabel, type Run, type Station, type StationKey } from "@/lib/run";
import { cn } from "@/lib/utils";

const ICONS: Record<StationKey, LucideIcon> = {
  engine: Cpu,
  read: ArrowDownToLine,
  model: ListTree,
  curves: Spline,
  view: Box,
  hash: Hash,
  write: PenLine,
};

function Disc({ s }: { s: Station }) {
  const Icon = ICONS[s.key];
  const done = s.state === "done";
  const active = s.state === "active";
  return (
    <div
      className={cn(
        "relative grid size-8 shrink-0 place-items-center rounded-full border-2 transition-colors duration-300",
        done ? "border-exact bg-exact text-white" : active ? "border-accent bg-panel-solid text-accent" : s.state === "paused" ? "border-accent/40 bg-panel-solid text-accent/70" : "border-line bg-panel-solid text-fg-3",
      )}
    >
      {active && <span className="absolute -inset-[5px] animate-halo rounded-full border-2 border-accent" aria-hidden="true" />}
      {done ? (
        <svg viewBox="0 0 20 20" className="size-4" fill="none" stroke="currentColor" strokeWidth={2.4} strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
          <motion.path d="m5 10.4 3.1 3.1 6.8-7.2" initial={{ pathLength: 0 }} animate={{ pathLength: 1 }} transition={{ duration: 0.26, ease: "easeOut" }} />
        </svg>
      ) : (
        <Icon className="size-[15px]" strokeWidth={1.8} aria-hidden="true" />
      )}
    </div>
  );
}

function Measure({ s }: { s: Station }) {
  if (s.frac !== null && s.state !== "idle")
    return (
      <div className="h-[3px] w-full max-w-24 overflow-hidden rounded-[2px] bg-line-soft">
        <div className={cn("h-full transition-[width] duration-150 ease-linear", s.state === "done" ? "bg-exact" : "bg-accent")} style={{ width: `${(s.frac * 100).toFixed(1)}%` }} />
      </div>
    );
  if (s.state === "active")
    return (
      <div className="flex h-[3px] items-center gap-[3px]" aria-hidden="true">
        {[0, 150, 300].map((d) => (
          <span key={d} className="size-1 animate-hop rounded-full bg-accent" style={{ animationDelay: `${d}ms` }} />
        ))}
      </div>
    );
  return <div className="h-[3px]" />;
}

export function LoadRail({ run, label, onCancel }: { run: Run; /** What the rail is for ("Opening site.zip"). */ label: string; onCancel?: () => void }) {
  const n = run.stations.length;
  return (
    <div
      className="panel pointer-events-auto relative flex w-full max-w-[720px] items-start gap-2 px-3 pt-3 pb-2.5 sm:px-4"
      role="status"
      aria-label={label}
      aria-live="polite"
    >
      <ol className="m-0 grid min-w-0 flex-1 list-none p-0" style={{ gridTemplateColumns: `repeat(${n}, minmax(0, 1fr))` }}>
        {run.stations.map((s, i) => (
          <li key={s.key} className="relative flex min-w-0 flex-col items-center gap-1.5" aria-current={s.state === "active" ? "step" : undefined}>
            {i < n - 1 && (
              <span className={cn("absolute top-[15px] left-1/2 h-0.5 w-full transition-colors duration-300", s.state === "done" ? "bg-exact" : "bg-line-soft")} aria-hidden="true" />
            )}
            <Disc s={s} />
            <span className={cn("hidden max-w-full truncate text-[12.5px] sm:block", s.state === "idle" ? "font-medium text-fg-3" : "font-semibold text-fg")}>{s.label}</span>
            <span className="num hidden h-4 max-w-full truncate text-[11px] text-fg-3 sm:block" title={s.value}>
              {s.value}
            </span>
            <Measure s={s} />
            <span className="sr-only">
              {s.label}: {s.state === "idle" ? "waiting" : s.state === "paused" ? "part done" : s.state}
              {s.value && `, ${s.value}`}
            </span>
          </li>
        ))}
      </ol>
      {/* Small screens: the stations as discs, and what is running in words. */}
      <div className="num absolute right-3 -bottom-6 left-3 truncate text-center text-[11.5px] text-fg-2 sm:hidden" aria-hidden="true">
        {runLabel(run)}
      </div>
      {onCancel && (
        <Button variant="ghost" size="sm" className="shrink-0 self-center" onClick={onCancel}>
          Cancel
        </Button>
      )}
    </div>
  );
}
