import { useState } from "react";
import { Check, ChevronRight, Copy, ExternalLink, FileText, Palette, TriangleAlert } from "lucide-react";
import { AnimatePresence, motion } from "motion/react";
import { Button } from "@/components/ui/button";
import type { Report } from "@/lib/engine";
import { problemUrl } from "@/lib/errors";
import { bytes, cap, fmt } from "@/lib/format";
import { formatInfo, type Format } from "@/lib/settings";
import { summarize, type Summary } from "@/lib/summary";
import { cn } from "@/lib/utils";

/** The result seal, re-animated whenever the result changes. */
function Seal({ summary, id }: { summary: Summary; id: string }) {
  const exact = summary.status === "exact";
  return (
    <div className="flex items-center gap-4 border-b border-line-soft px-5 pt-5 pb-4">
      <AnimatePresence mode="popLayout" initial={false}>
        <motion.div
          key={id}
          initial={{ scale: 0.7, opacity: 0 }}
          animate={{ scale: 1, opacity: 1 }}
          exit={{ scale: 0.85, opacity: 0 }}
          transition={{ type: "spring", stiffness: 380, damping: 24 }}
          className={cn(
            "grid size-12 shrink-0 place-items-center rounded-[3px] border-[1.5px]",
            exact ? "border-exact bg-exact-soft text-exact" : "border-warn bg-warn-soft text-warn",
          )}
        >
          {exact ? (
            <svg viewBox="0 0 20 20" className="size-6" fill="none" stroke="currentColor" strokeWidth={2} aria-hidden="true">
              <motion.path d="m5 10.4 3.1 3.1 6.8-7.2" initial={{ pathLength: 0 }} animate={{ pathLength: 1 }} transition={{ duration: 0.45, delay: 0.1 }} />
            </svg>
          ) : (
            <TriangleAlert className="size-6" aria-hidden="true" />
          )}
        </motion.div>
      </AnimatePresence>
      <div className={cn("font-display text-[21px] leading-tight font-semibold tracking-tight", exact ? "text-exact" : "text-warn")}>{summary.title}</div>
    </div>
  );
}

function Stat({ value, label }: { value: string; label: string }) {
  return (
    <div className="flex min-w-0 flex-col gap-0.5">
      <span className="num truncate text-[17px] text-fg">{value}</span>
      <span className="text-[12px] text-fg-3">{label}</span>
    </div>
  );
}

function Row({ children, tone = "plain" }: { children: React.ReactNode; tone?: "plain" | "warn" | "muted" }) {
  return (
    <li
      className={cn(
        "flex items-center gap-2.5 rounded-[3px] px-3 py-2 text-[13px] leading-snug",
        tone === "warn" && "bg-warn-soft text-fg",
        tone === "plain" && "bg-panel-2",
        tone === "muted" && "px-1 text-fg-3",
      )}
    >
      {children}
    </li>
  );
}

/** Hashes, sources, timings and every engine note, for developers and for audits. */
function TechnicalDetails({
  report,
  ms,
  timings,
  exporter,
  includeName,
  onIncludeName,
  onDownloadReport,
}: {
  report: Report;
  ms: number;
  timings: Record<string, number>;
  exporter: string | null;
  includeName: boolean;
  onIncludeName: (on: boolean) => void;
  onDownloadReport: () => void;
}) {
  const [open, setOpen] = useState(false);
  const [copied, setCopied] = useState(false);
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(report.parity_hash);
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    } catch {
      /* clipboard blocked: the hash stays selectable */
    }
  };
  return (
    <div className="border-t border-line-soft">
      <button
        type="button"
        onClick={() => setOpen(!open)}
        aria-expanded={open}
        className="flex w-full cursor-pointer items-center gap-2 px-5 py-3 text-left text-[13px] text-fg-3 hover:text-fg"
      >
        <ChevronRight className={cn("size-4 transition-transform", open && "rotate-90")} />
        Technical details
      </button>
      <AnimatePresence initial={false}>
        {open && (
          <motion.div initial={{ height: 0, opacity: 0 }} animate={{ height: "auto", opacity: 1 }} exit={{ height: 0, opacity: 0 }} className="overflow-hidden">
            <div className="flex flex-col gap-4 px-5 pb-5 text-[12.5px]">
              <div className="flex flex-col gap-1.5">
                <div className="label">Parity hash</div>
                <div className="flex items-center gap-2">
                  <code className="num min-w-0 flex-1 truncate text-fg select-all" title={report.parity_hash}>
                    {report.parity_hash}
                  </code>
                  <Button variant="ghost" size="icon-sm" onClick={copy} aria-label="Copy parity hash">
                    {copied ? <Check className="text-exact" /> : <Copy />}
                  </Button>
                </div>
              </div>
              <dl className="m-0 grid grid-cols-[96px_minmax(0,1fr)] gap-x-3 gap-y-1.5">
                <dt className="text-fg-3">Source</dt>
                <dd className="num m-0">
                  {bytes(report.input.bytes)}, {exporter ?? "exporter not stated"}
                </dd>
                <dt className="text-fg-3">SHA-256</dt>
                <dd className="num m-0 truncate" title={report.input.sha256}>
                  {report.input.sha256}
                </dd>
                <dt className="text-fg-3">Vertices</dt>
                <dd className="num m-0">
                  {fmt(report.input.vertices)} in, {fmt(report.output.vertices_written)} written
                </dd>
                <dt className="text-fg-3">Entities</dt>
                <dd className="num m-0">
                  {fmt(report.output.mesh_entities)} MESH, {fmt(report.output.polylines)} POLYLINE, {fmt(report.output.points)} POINT
                </dd>
                <dt className="text-fg-3">Format</dt>
                <dd className="num m-0">{report.output.format}</dd>
                <dt className="text-fg-3">Time</dt>
                <dd className="num m-0" title={Object.entries(timings).map(([k, v]) => `${k}: ${Math.round(v)} ms`).join("\n")}>
                  {Math.round(ms)} ms
                </dd>
                <dt className="text-fg-3">Engine</dt>
                <dd className="num m-0">{report.engine_version}</dd>
              </dl>
              <label className="flex cursor-pointer items-center gap-2 text-fg-2">
                <input type="checkbox" className="size-3.5 accent-[var(--accent)]" checked={includeName} onChange={(e) => onIncludeName(e.target.checked)} />
                File name in drawing properties
              </label>
              {report.diagnostics.length > 0 && (
                <ul className="m-0 flex list-none flex-col gap-1 p-0">
                  {report.diagnostics.map((d) => (
                    <li key={d.code} className="flex gap-2 text-fg-2">
                      <span className="num shrink-0 text-fg-3">{d.severity === "warning" ? "warn" : "info"}</span>
                      <span className="min-w-0 flex-1">
                        {cap(d.message)}
                        {d.line > 0 && <span className="num text-fg-3"> · line {d.line}</span>}
                        {d.count > 1 && <span className="num text-fg-3"> ×{fmt(d.count)}</span>}
                      </span>
                    </li>
                  ))}
                </ul>
              )}
              <div className="flex flex-wrap items-center gap-x-4 gap-y-2">
                <Button size="sm" onClick={onDownloadReport}>
                  <FileText />
                  Download report
                </Button>
                <Button variant="link" className="text-[12.5px]" asChild>
                  <a href={problemUrl("Problem with a conversion", `engine ${report.engine_version}, ${report.output.format}, parity ${report.parity_hash.slice(0, 12)}`)} target="_blank" rel="noreferrer">
                    Report a problem <ExternalLink className="!size-3.5" />
                  </a>
                </Button>
              </div>
            </div>
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  );
}

export function Inspector({
  report,
  format,
  ms,
  timings,
  exporter,
  includeName,
  onIncludeName,
  onKeepLoose,
  onAddMtl,
  onDownloadReport,
}: {
  report: Report;
  format: Format;
  ms: number;
  timings: Record<string, number>;
  exporter: string | null;
  includeName: boolean;
  onIncludeName: (on: boolean) => void;
  onKeepLoose: (keep: boolean) => void;
  onAddMtl: () => void;
  onDownloadReport: () => void;
}) {
  const s = summarize(report);
  const layers = report.layers.filter((l) => l.faces + l.polylines + l.points > 0).length;

  return (
    <aside className="panel pointer-events-auto flex max-h-full min-h-0 w-full flex-col overflow-hidden" aria-label="Result">
      <Seal summary={s} id={`${report.parity_hash}${s.status}`} />

      <div className="min-h-0 flex-1 overflow-y-auto">
        <div className="grid grid-cols-3 gap-3 border-b border-line-soft px-5 py-4">
          <Stat value={fmt(s.shapes.count)} label={s.shapes.noun} />
          <Stat value={fmt(layers)} label={layers === 1 ? "layer" : "layers"} />
          <Stat value={bytes(report.output.bytes)} label={`${formatInfo(format).label.replace(" (binary)", "")} file`} />
        </div>

        {(s.leftOut.length > 0 || s.loosePoints || s.needsMtl || s.notIncluded.length > 0) && (
          <ul className="m-0 flex list-none flex-col gap-1.5 px-5 py-4">
            {s.leftOut.length > 0 && (
              <Row tone="warn">
                <TriangleAlert className="size-4 shrink-0 text-warn" />
                <span className="min-w-0 flex-1">Left out: {s.leftOut.join(", ")}</span>
              </Row>
            )}
            {s.loosePoints && (
              <Row>
                <span className="min-w-0 flex-1">
                  {fmt(s.loosePoints.count)} loose {s.loosePoints.count === 1 ? "point" : "points"} {s.loosePoints.included ? "included" : "not included"}
                </span>
                <Button variant="link" className="text-[13px]" onClick={() => onKeepLoose(!s.loosePoints!.included)}>
                  {s.loosePoints.included ? "Leave out" : "Include"}
                </Button>
              </Row>
            )}
            {s.needsMtl && (
              <Row>
                <Palette className="size-4 shrink-0 text-accent" />
                <span className="min-w-0 flex-1">Material colors</span>
                <Button variant="link" className="text-[13px]" onClick={onAddMtl}>
                  Add .mtl file…
                </Button>
              </Row>
            )}
            {s.notIncluded.length > 0 && <Row tone="muted">Not included: {s.notIncluded.join(", ")}</Row>}
          </ul>
        )}

        <TechnicalDetails
          report={report}
          ms={ms}
          timings={timings}
          exporter={exporter}
          includeName={includeName}
          onIncludeName={onIncludeName}
          onDownloadReport={onDownloadReport}
        />
      </div>
    </aside>
  );
}
