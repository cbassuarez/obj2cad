import { useState } from "react";
import { Check, ChevronRight, CircleCheck, Copy, FileText, Info, Palette } from "lucide-react";
import { AnimatePresence, motion } from "motion/react";
import { Button } from "@/components/ui/button";
import type { Report } from "@/lib/engine";
import { bytes, cap, fmt } from "@/lib/format";
import { designerNotes } from "@/lib/notes";
import { cn } from "@/lib/utils";

/** The "Exact" seal (B), re-animated whenever the result changes. */
function Seal({ report }: { report: Report }) {
  return (
    <div className="flex items-center gap-4 border-b border-line-soft px-5 pt-5 pb-4">
      <AnimatePresence mode="popLayout" initial={false}>
        <motion.div
          key={report.parity_hash}
          initial={{ scale: 0.7, opacity: 0 }}
          animate={{ scale: 1, opacity: 1 }}
          exit={{ scale: 0.85, opacity: 0 }}
          transition={{ type: "spring", stiffness: 380, damping: 24 }}
          className="grid size-12 shrink-0 place-items-center rounded-[3px] border-[1.5px] border-exact bg-exact-soft"
        >
          <svg viewBox="0 0 20 20" className="size-6 text-exact" fill="none" stroke="currentColor" strokeWidth={2} aria-hidden="true">
            <motion.path d="m5 10.4 3.1 3.1 6.8-7.2" initial={{ pathLength: 0 }} animate={{ pathLength: 1 }} transition={{ duration: 0.45, delay: 0.1 }} />
          </svg>
        </motion.div>
      </AnimatePresence>
      <div className="min-w-0">
        <div className="font-display text-[22px] leading-tight font-semibold tracking-tight text-exact">Exact copy</div>
        <div className="text-[13.5px] text-fg-2">Every shape lands in the DXF exactly as modeled.</div>
      </div>
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

/** Hashes, sources, timings and every engine note, for developers and for audits. */
function TechnicalDetails({ report, ms, timings, exporter }: { report: Report; ms: number; timings: Record<string, number>; exporter: string | null }) {
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
                <p className="m-0 text-fg-3">Canonical hash of the written geometry, also stored in the drawing (DWGPROPS → Custom).</p>
              </div>
              <dl className="m-0 grid grid-cols-[96px_minmax(0,1fr)] gap-x-3 gap-y-1.5">
                <dt className="text-fg-3">Source</dt>
                <dd className="num m-0">{bytes(report.input.bytes)}, {exporter ?? "exporter not stated"}</dd>
                <dt className="text-fg-3">SHA-256</dt>
                <dd className="num m-0 truncate" title={report.input.sha256}>{report.input.sha256}</dd>
                <dt className="text-fg-3">Vertices</dt>
                <dd className="num m-0">{fmt(report.input.vertices)} in, {fmt(report.output.vertices_written)} written</dd>
                <dt className="text-fg-3">Entities</dt>
                <dd className="num m-0">
                  {fmt(report.output.mesh_entities)} MESH, {fmt(report.output.polylines)} POLYLINE, {fmt(report.output.points)} POINT
                </dd>
                <dt className="text-fg-3">Time</dt>
                <dd className="num m-0" title={Object.entries(timings).map(([k, v]) => `${k}: ${Math.round(v)} ms`).join("\n")}>
                  {Math.round(ms)} ms
                </dd>
                <dt className="text-fg-3">Engine</dt>
                <dd className="num m-0">{report.engine_version}</dd>
              </dl>
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
            </div>
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  );
}

export function Inspector({
  report,
  ms,
  timings,
  exporter,
  onAddMtl,
  onDownloadReport,
}: {
  report: Report;
  ms: number;
  timings: Record<string, number>;
  exporter: string | null;
  onAddMtl: () => void;
  onDownloadReport: () => void;
}) {
  const notes = designerNotes(report);
  const layers = report.layers.filter((l) => l.faces + l.polylines + l.points > 0).length;

  return (
    <aside className="panel pointer-events-auto flex max-h-full min-h-0 w-full flex-col overflow-hidden" aria-label="Result">
      <Seal report={report} />

      <div className="min-h-0 flex-1 overflow-y-auto">
        <div className="grid grid-cols-3 gap-3 border-b border-line-soft px-5 py-4">
          <Stat value={fmt(report.output.faces + report.output.polylines + report.output.points)} label={report.output.polylines + report.output.points ? "shapes" : "faces"} />
          <Stat value={fmt(layers)} label={layers === 1 ? "layer" : "layers"} />
          <Stat value={bytes(report.output.bytes)} label="DXF file" />
        </div>

        <div className="flex flex-col gap-2 px-5 py-4">
          <div className="label">Good to know</div>
          {notes.length === 0 ? (
            <div className="flex items-center gap-2.5 rounded-[3px] bg-exact-soft px-3 py-2.5 text-[13px] text-exact">
              <CircleCheck className="size-4 shrink-0" />
              Everything in your file is included.
            </div>
          ) : (
            <ul className="m-0 flex list-none flex-col gap-1.5 p-0">
              {notes.map((n) => (
                <li key={n.code} className="flex gap-2.5 rounded-[3px] bg-panel-2 px-3 py-2.5 text-[13px] leading-snug">
                  {n.action ? <Palette className="mt-0.5 size-4 shrink-0 text-accent" /> : <Info className="mt-0.5 size-4 shrink-0 text-fg-3" />}
                  <span className="min-w-0 flex-1">
                    {n.text}
                    {n.action === "add-mtl" && (
                      <Button variant="link" className="ml-1.5 inline h-auto p-0 text-[13px]" onClick={onAddMtl}>
                        Add .mtl…
                      </Button>
                    )}
                  </span>
                </li>
              ))}
            </ul>
          )}
        </div>

        <TechnicalDetails report={report} ms={ms} timings={timings} exporter={exporter} />
      </div>

      <div className="border-t border-line-soft p-3">
        <Button variant="ghost" className="w-full" onClick={onDownloadReport}>
          <FileText />
          Download report
        </Button>
      </div>
    </aside>
  );
}
