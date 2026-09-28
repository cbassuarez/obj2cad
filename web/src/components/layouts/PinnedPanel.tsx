// Layout A, "Pinned": one inspector column read top to bottom (result, adjust, view,
// details) with the export pinned to its foot, so the download is always the last thing
// in the column and the only filled button on the page.
import { Stat, Seal, Notes, TechnicalDetails } from "@/components/Inspector";
import {
  DownloadAfter,
  DownloadButton,
  FormatToggle,
  Provenance,
  UnitsMenu,
  UpToggle,
  ViewTools,
  shortFormat,
  sizeText,
  unitsChanged,
  unitsTag,
  upChanged,
  upTag,
  type ResultProps,
} from "@/components/controls";
import { useState } from "react";
import { ChevronRight } from "lucide-react";
import { AnimatePresence, motion } from "motion/react";
import { bytes, fmt } from "@/lib/format";
import { summarize } from "@/lib/summary";
import { cn } from "@/lib/utils";

function Section({ label, aside, children, className }: { label: string; aside?: React.ReactNode; children: React.ReactNode; className?: string }) {
  return (
    <section className={cn("flex flex-col gap-2 border-b border-line-soft px-5 py-4", className)}>
      <div className="flex items-baseline justify-between gap-2">
        <h3 className="label m-0">{label}</h3>
        {aside}
      </div>
      {children}
    </section>
  );
}

export function PinnedPanel(p: ResultProps) {
  const { report, decisions } = p;
  const s = summarize(report);
  const size = sizeText(report, decisions);
  const [details, setDetails] = useState(false);

  return (
    <aside className="panel pointer-events-auto flex max-h-full min-h-0 w-full flex-col overflow-hidden" aria-label="Result">
      <Seal summary={s} id={`${report.parity_hash}${s.status}`} />

      <div className="min-h-0 flex-1 overflow-y-auto">
        <div className="grid grid-cols-3 gap-3 border-b border-line-soft px-5 py-4">
          <Stat value={fmt(s.shapes.count)} label={s.shapes.noun} />
          <Stat value={fmt(p.layerCount)} label={p.layerCount === 1 ? "layer" : "layers"} />
          <Stat value={bytes(report.output.bytes)} label={`${shortFormat(p.format)} file`} />
        </div>

        <Notes summary={s} onKeepLoose={p.onKeepLoose} onAddMtl={p.onAddMtl} className="border-b border-line-soft px-5 py-4" />

        <Section label="Up direction" aside={<Provenance tag={upTag(decisions)} onReset={upChanged(decisions) ? () => p.onUp(null) : undefined} />}>
          <UpToggle decisions={decisions} onUp={p.onUp} className="w-full" />
        </Section>

        <Section label="Units" aside={<Provenance tag={unitsTag(decisions)} onReset={unitsChanged(decisions) ? () => p.onUnits(null) : undefined} />}>
          <div className="flex items-baseline justify-between gap-3 text-[13.5px]">
            <UnitsMenu decisions={decisions} unitsStated={p.unitsStated} houseUnits={p.houseUnits} onUnits={p.onUnits} onHouseUnits={p.onHouseUnits} />
            {size && <span className="num truncate text-[12.5px] text-fg-3">{size}</span>}
          </div>
        </Section>

        <Section label="View">
          <ViewTools edges={p.edges} ortho={p.ortho} onEdges={p.onEdges} onFit={p.onFit} onView={p.onView} onOrtho={p.onOrtho} />
        </Section>

        <button
          type="button"
          onClick={() => setDetails(!details)}
          aria-expanded={details}
          className="flex w-full cursor-pointer items-center gap-2 px-5 py-3 text-left text-[13px] text-fg-3 hover:text-fg"
        >
          <ChevronRight className={cn("size-4 transition-transform", details && "rotate-90")} />
          Technical details
        </button>
        <AnimatePresence initial={false}>
          {details && (
            <motion.div initial={{ height: 0, opacity: 0 }} animate={{ height: "auto", opacity: 1 }} exit={{ height: 0, opacity: 0 }} className="overflow-hidden px-5 pb-5">
              <TechnicalDetails
                plain
                report={report}
                ms={p.ms}
                timings={p.timings}
                exporter={p.exporter}
                includeName={p.includeName}
                onIncludeName={p.onIncludeName}
                onDownloadReport={p.onDownloadReport}
              />
            </motion.div>
          )}
        </AnimatePresence>
      </div>

      {/* The export: pinned, on solid ground, set apart from everything above. */}
      <div className="sticky bottom-0 flex flex-col gap-2.5 border-t border-line bg-panel-solid px-5 pt-4 pb-4">
        <div className="flex items-baseline justify-between">
          <h3 className="label m-0">Export</h3>
          <span className="num text-[11.5px] text-fg-3">
            {shortFormat(p.format)} · {bytes(report.output.bytes)}
          </span>
        </div>
        <FormatToggle format={p.format} busy={p.busy} onFormat={p.onFormat} />
        <DownloadButton {...p} />
        <DownloadAfter {...p} />
      </div>
    </aside>
  );
}
