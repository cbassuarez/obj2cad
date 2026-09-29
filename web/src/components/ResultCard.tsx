// The result card: a short list that reads as one sentence about the drawing. Each value
// the app decided (up direction, units, format) is changed where it's read, and the
// download follows. The audit material (hashes, timings, the report) sits behind the "⋯"
// menu, so the card holds one button only: the download.
import { useState } from "react";
import { Menu, Modal } from "@mantine/core";
import { notifications } from "@mantine/notifications";
import { Check, ChevronDown, Copy, ExternalLink, FileText, Info, MoreHorizontal, Palette, TriangleAlert } from "lucide-react";
import { AnimatePresence, motion } from "motion/react";
import { Button } from "@/components/ui/button";
import { DownloadAfter, DownloadButton, Provenance, UnitsMenu, menuStyles, sizeText, unitsChanged, unitsLabel, unitsTag, upChanged, upTag, type ResultProps } from "@/components/controls";
import type { Report } from "@/lib/engine";
import { problemUrl } from "@/lib/errors";
import { bytes, cap, fmt } from "@/lib/format";
import { FORMATS, UPS, type UpAxis } from "@/lib/settings";
import { summarize, type Summary } from "@/lib/summary";
import { cn } from "@/lib/utils";

const token =
  "inline-flex cursor-pointer items-center gap-0.5 rounded-[3px] font-semibold text-fg underline decoration-line decoration-dotted underline-offset-[3px] hover:bg-panel-2 hover:decoration-fg";

const reportProblemUrl = (r: Report) => problemUrl("Problem with a conversion", `engine ${r.engine_version}, ${r.output.format}, parity ${r.parity_hash.slice(0, 12)}`);

/** The result seal, re-animated whenever the result changes. */
function Seal({ summary, id }: { summary: Summary; id: string }) {
  const exact = summary.status === "exact";
  return (
    <div className="flex min-w-0 items-center gap-3">
      <AnimatePresence mode="popLayout" initial={false}>
        <motion.div
          key={id}
          initial={{ scale: 0.7, opacity: 0 }}
          animate={{ scale: 1, opacity: 1 }}
          exit={{ scale: 0.85, opacity: 0 }}
          transition={{ type: "spring", stiffness: 380, damping: 24 }}
          className={cn("grid size-9 shrink-0 place-items-center rounded-[3px] border-[1.5px]", exact ? "border-exact bg-exact-soft text-exact" : "border-warn bg-warn-soft text-warn")}
        >
          {exact ? (
            <svg viewBox="0 0 20 20" className="size-5" fill="none" stroke="currentColor" strokeWidth={2} aria-hidden="true">
              <motion.path d="m5 10.4 3.1 3.1 6.8-7.2" initial={{ pathLength: 0 }} animate={{ pathLength: 1 }} transition={{ duration: 0.45, delay: 0.1 }} />
            </svg>
          ) : (
            <TriangleAlert className="size-5" aria-hidden="true" />
          )}
        </motion.div>
      </AnimatePresence>
      <div className={cn("font-display text-[17px] leading-tight font-semibold tracking-tight", exact ? "text-exact" : "text-warn")}>{summary.title}</div>
    </div>
  );
}

function Line({ label, children, aside }: { label: string; children: React.ReactNode; aside?: React.ReactNode }) {
  return (
    <div className="grid grid-cols-[92px_minmax(0,1fr)_auto] items-baseline gap-x-3 py-1.5 text-[13.5px]">
      <dt className="text-fg-3">{label}</dt>
      <dd className="m-0 min-w-0 truncate">{children}</dd>
      <span className="text-right">{aside}</span>
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

/** What was left out or can be added: nothing when the conversion is complete. */
function Notes({ summary: s, onKeepLoose, onAddMtl }: { summary: Summary; onKeepLoose: (keep: boolean) => void; onAddMtl: () => void }) {
  if (!(s.leftOut.length > 0 || s.loosePoints || s.needsMtl || s.notIncluded.length > 0)) return null;
  return (
    <ul className="m-0 flex list-none flex-col gap-1.5 p-0 pt-3">
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
          <Button variant="link" className="h-auto px-0 text-[13px]" onClick={() => onKeepLoose(!s.loosePoints!.included)}>
            {s.loosePoints.included ? "Leave out" : "Include"}
          </Button>
        </Row>
      )}
      {s.needsMtl && (
        <Row>
          <Palette className="size-4 shrink-0 text-accent" />
          <span className="min-w-0 flex-1">Material colors</span>
          <Button variant="link" className="h-auto px-0 text-[13px]" onClick={onAddMtl}>
            Add .mtl file…
          </Button>
        </Row>
      )}
      {s.notIncluded.length > 0 && <Row tone="muted">Not included: {s.notIncluded.join(", ")}</Row>}
    </ul>
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
}: Pick<ResultProps, "report" | "ms" | "timings" | "exporter" | "includeName" | "onIncludeName" | "onDownloadReport">) {
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
    <div className="flex flex-col gap-4 py-1 text-[12.5px]">
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
        <dd
          className="num m-0"
          title={Object.entries(timings)
            .map(([k, v]) => `${k}: ${Math.round(v)} ms`)
            .join("\n")}
        >
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
        <Button variant="link" className="h-auto px-0 text-[12.5px]" onClick={onDownloadReport}>
          <FileText className="!size-3.5" />
          Save conversion report (.json)
        </Button>
        <Button variant="link" className="h-auto px-0 text-[12.5px]" asChild>
          <a href={reportProblemUrl(report)} target="_blank" rel="noreferrer">
            Report a problem <ExternalLink className="!size-3.5" />
          </a>
        </Button>
      </div>
    </div>
  );
}

function ValueMenu<T extends string>({
  label,
  value,
  options,
  onPick,
  disabled,
}: {
  label: string;
  value: T;
  options: { value: T; label: string; beta?: boolean }[];
  onPick: (v: T) => void;
  disabled?: boolean;
}) {
  return (
    <Menu position="bottom-start" offset={6} width={200} classNames={menuStyles}>
      <Menu.Target>
        <button type="button" className={token} disabled={disabled}>
          {options.find((o) => o.value === value)!.label}
          <ChevronDown className="size-3.5" />
        </button>
      </Menu.Target>
      <Menu.Dropdown>
        <Menu.Label>{label}</Menu.Label>
        {options.map((o) => (
          <Menu.Item key={o.value} onClick={() => onPick(o.value)} rightSection={value === o.value ? <Check className="size-3.5" /> : null}>
            {o.label}
            {o.beta && <span className="ml-1.5 rounded-[2px] bg-warn-soft px-1 py-px text-[10.5px] font-medium text-warn">beta</span>}
          </Menu.Item>
        ))}
      </Menu.Dropdown>
    </Menu>
  );
}

export function ResultCard(p: ResultProps) {
  const { report, decisions } = p;
  const s = summarize(report);
  const size = sizeText(report, decisions);
  const [details, setDetails] = useState(false);
  const copyHash = () =>
    void navigator.clipboard
      ?.writeText(report.parity_hash)
      .then(() => notifications.show({ message: "Parity hash copied" }))
      .catch(() => notifications.show({ color: "red", message: "Couldn't copy: open Technical details to select the hash" }));

  return (
    <aside className="panel pointer-events-auto flex max-h-full min-h-0 w-full flex-col overflow-hidden" aria-label="Result">
      <div className="flex items-center justify-between gap-2 px-5 pt-4 pb-3">
        <Seal summary={s} id={`${report.parity_hash}${s.status}`} />
        <Menu position="bottom-end" offset={6} width={240} classNames={menuStyles}>
          <Menu.Target>
            <Button variant="ghost" size="icon-sm" aria-label="More">
              <MoreHorizontal />
            </Button>
          </Menu.Target>
          <Menu.Dropdown>
            <Menu.Item leftSection={<Info className="size-4" />} onClick={() => setDetails(true)}>
              Technical details…
            </Menu.Item>
            <Menu.Item leftSection={<Copy className="size-4" />} onClick={copyHash}>
              Copy parity hash
            </Menu.Item>
            <Menu.Item leftSection={<FileText className="size-4" />} onClick={p.onDownloadReport}>
              Save conversion report (.json)
            </Menu.Item>
            <Menu.Divider />
            <Menu.Item leftSection={<ExternalLink className="size-4" />} component="a" href={reportProblemUrl(report)} target="_blank" rel="noreferrer">
              Report a problem
            </Menu.Item>
          </Menu.Dropdown>
        </Menu>
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto px-5">
        <dl className="m-0 border-y border-line-soft py-1.5">
          <Line label="Contents">
            <span className="num">{fmt(s.shapes.count)}</span> {s.shapes.count === 1 ? s.shapes.noun.slice(0, -1) : s.shapes.noun} on <span className="num">{fmt(p.layerCount - p.hiddenCount)}</span>
            {p.hiddenCount > 0 && ` of ${fmt(p.layerCount)}`} {p.layerCount === 1 ? "layer" : "layers"}
          </Line>
          <Line label="Up" aside={<Provenance tag={upTag(decisions)} onReset={upChanged(decisions) ? () => p.onUp(null) : undefined} />}>
            <ValueMenu<UpAxis> label="Up direction" value={decisions.up_axis} options={UPS} onPick={p.onUp} />
          </Line>
          <Line label="Units" aside={<Provenance tag={unitsTag(decisions)} onReset={unitsChanged(decisions) ? () => p.onUnits(null) : undefined} />}>
            <UnitsMenu decisions={decisions} unitsStated={p.unitsStated} houseUnits={p.houseUnits} onUnits={p.onUnits} onHouseUnits={p.onHouseUnits}>
              <button type="button" className={token}>
                {unitsLabel(decisions)}
                <ChevronDown className="size-3.5" />
              </button>
            </UnitsMenu>
          </Line>
          {size && (
            <Line label="Size">
              <span className="num text-[13px]">{size}</span>
            </Line>
          )}
          <Line label="Format" aside={<span className="num text-[12px] text-fg-3">{bytes(report.output.bytes)}</span>}>
            <ValueMenu label="Format" value={p.format} options={FORMATS} onPick={p.onFormat} disabled={p.busy !== null} />
          </Line>
        </dl>

        <Notes summary={s} onKeepLoose={p.onKeepLoose} onAddMtl={p.onAddMtl} />
      </div>

      <div className="flex flex-col gap-2 px-5 pt-4 pb-4">
        <DownloadButton {...p} />
        <DownloadAfter {...p} />
      </div>

      <Modal
        opened={details}
        onClose={() => setDetails(false)}
        title="Technical details"
        centered
        size="lg"
        classNames={{ content: "panel !bg-panel-solid", header: "!bg-panel-solid", title: "label" }}
      >
        <TechnicalDetails {...p} />
      </Modal>
    </aside>
  );
}
