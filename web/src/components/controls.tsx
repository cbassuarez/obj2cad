// The result card's controls and the viewport's view tools.
import { Box, Check, ChevronDown, CircleCheck, Download, Rotate3d, Scan } from "lucide-react";
import { motion } from "motion/react";
import { Menu } from "@mantine/core";
import { Button } from "@/components/ui/button";
import { Tip } from "@/components/ui/tooltip";
import type { Decisions, Report } from "@/lib/engine";
import { canPickSaveLocation } from "@/lib/files";
import { bytes, fmt, measure } from "@/lib/format";
import { UNITS, formatInfo, unitName, unitSymbol, type Format, type UpAxis, type Units } from "@/lib/settings";
import { cn, shortcut } from "@/lib/utils";
import type { ViewName } from "@/viewer/Viewer";

export const menuStyles = { dropdown: "panel !p-1", item: "!rounded-[3px] !text-[13px]", label: "!text-[11px]" };

const VIEWS: { value: ViewName; label: string }[] = [
  { value: "iso", label: "Isometric" },
  { value: "top", label: "Top" },
  { value: "front", label: "Front" },
  { value: "right", label: "Right" },
];

const UNITS_FROM: Record<Decisions["units_from"], string | undefined> = { chosen: undefined, file: "auto", size: "auto", default: "default", none: undefined };

export const shortFormat = (f: Format) => formatInfo(f).label.replace(" (binary)", "");

/** Everything the result card shows and changes. */
export interface ResultProps {
  report: Report;
  decisions: Decisions;
  format: Format;
  ms: number;
  timings: Record<string, number>;
  exporter: string | null;
  busy: string | null;
  downloaded: string | null;
  /** The file's exporter states its units. */
  unitsStated: boolean;
  houseUnits: Units | null;
  includeName: boolean;
  /** Also write recognized curved surfaces. */
  curves: boolean;
  edges: boolean;
  ortho: boolean;
  /** Layers hidden in the viewer, and how many layers have geometry. */
  hiddenCount: number;
  layerCount: number;
  onUp: (u: UpAxis | null) => void;
  onUnits: (u: Units | null) => void;
  onHouseUnits: (u: Units | null) => void;
  onKeepLoose: (keep: boolean) => void;
  onFormat: (f: Format) => void;
  onIncludeName: (on: boolean) => void;
  onCurves: (on: boolean) => void;
  onEdges: (on: boolean) => void;
  onView: (v: ViewName) => void;
  onOrtho: (on: boolean) => void;
  onFit: () => void;
  /** Download the drawing, leaving out hidden layers; `pick` asks where to save. */
  onDownload: (pick: boolean) => void;
  onDownloadReport: () => void;
  onAddMtl: () => void;
  onAnother: () => void;
}

export const upChanged = (d: Decisions) => d.up_from === "chosen" && d.up_axis !== d.detected_up_axis;
export const unitsChanged = (d: Decisions) => d.units_from === "chosen" && d.units !== d.detected_units;
export const unitsLabel = (d: Decisions) => (d.units === "unitless" ? "None" : unitName(d.units));

/** "auto" / "default" beside a value the app decided, or a Reset link once the user changed it. */
export function Provenance({ tag, onReset, className }: { tag?: string; onReset?: () => void; className?: string }) {
  if (onReset)
    return (
      <button type="button" className={cn("cursor-pointer text-[12px] font-semibold text-accent hover:underline", className)} onClick={onReset}>
        Reset
      </button>
    );
  return tag ? <span className={cn("text-[12px] text-fg-3", className)}>{tag}</span> : null;
}

export const upTag = (d: Decisions) => (d.up_from === "detected" ? "auto" : undefined);
export const unitsTag = (d: Decisions) => UNITS_FROM[d.units_from];

/** The units, as an underlined value that opens the list. */
export function UnitsMenu({
  decisions,
  unitsStated,
  houseUnits,
  onUnits,
  onHouseUnits,
  position = "bottom-start",
  children,
}: Pick<ResultProps, "decisions" | "unitsStated" | "houseUnits" | "onUnits" | "onHouseUnits"> & {
  position?: "bottom-start" | "top-start" | "bottom-end";
  /** Custom trigger; defaults to the underlined unit name. */
  children?: React.ReactElement;
}) {
  return (
    <Menu position={position} offset={6} width={230} classNames={menuStyles}>
      <Menu.Target>
        {children ?? (
          <button type="button" className="inline-flex cursor-pointer items-center gap-0.5 font-semibold text-fg underline decoration-line underline-offset-2 hover:decoration-fg">
            {unitsLabel(decisions)}
            <ChevronDown className="size-3.5" />
          </button>
        )}
      </Menu.Target>
      <Menu.Dropdown>
        <Menu.Label>Units</Menu.Label>
        {UNITS.map((u) => (
          <Menu.Item key={u.value} onClick={() => onUnits(u.value)} rightSection={decisions.units === u.value ? <Check className="size-3.5" /> : null}>
            {u.name}
          </Menu.Item>
        ))}
        {!unitsStated && decisions.units !== "unitless" && (
          <>
            <Menu.Divider />
            <Menu.Item onClick={() => onHouseUnits(houseUnits === decisions.units ? null : decisions.units)} rightSection={houseUnits === decisions.units ? <Check className="size-3.5" /> : null}>
              Use for files without units
            </Menu.Item>
          </>
        )}
      </Menu.Dropdown>
    </Menu>
  );
}

/** "120 × 80 × 40 mm", or null when the drawing is empty. */
export function sizeText(report: Report, decisions: Decisions): string | null {
  const b = report.output.bounds;
  if (!b) return null;
  const symbol = unitSymbol(decisions.units);
  return `${b[1]
    .map((hi, a) => hi - b[0][a])
    .map(measure)
    .join(" × ")}${symbol ? ` ${symbol}` : ""}`;
}

/** What the download holds, given the layers hidden in the viewer. */
export const layersShown = ({ hiddenCount, layerCount }: Pick<ResultProps, "hiddenCount" | "layerCount">) => (hiddenCount === 0 ? "all" : hiddenCount < layerCount ? "some" : "none");

/** The one filled button on the page. Hidden layers are left out. */
export function DownloadButton({
  report,
  format,
  busy,
  hiddenCount,
  layerCount,
  onDownload,
  className,
}: Pick<ResultProps, "report" | "format" | "busy" | "hiddenCount" | "layerCount" | "onDownload"> & { className?: string }) {
  const shown = layersShown({ hiddenCount, layerCount });
  return (
    <Tip label={`Download (${shortcut("S")})`}>
      <Button variant="primary" size="lg" className={cn("w-full justify-between px-5", className)} onClick={() => onDownload(false)} disabled={busy !== null || shown === "none"}>
        <span className="flex items-center gap-2.5">
          <Download />
          Download {shortFormat(format)}
        </span>
        <span className="num text-[12px] font-normal opacity-80">{busy !== null ? "…" : shown === "all" ? bytes(report.output.bytes) : `${fmt(layerCount - hiddenCount)} of ${fmt(layerCount)} layers`}</span>
      </Button>
    </Tip>
  );
}

/** "Save as…" and the downloaded confirmation, under the download button. */
export function DownloadAfter({
  busy,
  downloaded,
  hiddenCount,
  layerCount,
  onDownload,
  onAnother,
}: Pick<ResultProps, "busy" | "downloaded" | "hiddenCount" | "layerCount" | "onDownload" | "onAnother">) {
  const shown = layersShown({ hiddenCount, layerCount });
  if (downloaded && busy === null && shown !== "none")
    return (
      <motion.div initial={{ opacity: 0, height: 0 }} animate={{ opacity: 1, height: "auto" }} className="flex items-center gap-2 text-[13px]">
        <CircleCheck className="size-4 shrink-0 text-exact" />
        <span className="min-w-0 truncate">
          Downloaded <span className="num">{downloaded}</span>
        </span>
        <Button variant="link" className="ml-auto h-auto px-0 text-[13px]" onClick={onAnother}>
          Convert another
        </Button>
      </motion.div>
    );
  return (
    <div className="flex min-h-5 items-center gap-3 text-[12.5px] text-fg-3">
      <span>{shown === "all" ? "Everything in the viewer is included" : shown === "some" ? "Unticked layers are left out" : "No layer is ticked: tick one to download"}</span>
      {canPickSaveLocation() && shown === "all" && (
        <Button variant="link" className="ml-auto h-auto px-0 text-[12.5px]" onClick={() => onDownload(true)} disabled={busy !== null}>
          Save as…
        </Button>
      )}
    </div>
  );
}

/** Edges, fit, named views and projection: a vertical icon strip beside the result card. */
export function ViewTools({ edges, ortho, onEdges, onFit, onView, onOrtho }: Pick<ResultProps, "edges" | "ortho" | "onEdges" | "onFit" | "onView" | "onOrtho">) {
  return (
    <div className="panel pointer-events-auto p-1">
      <div className="flex flex-col gap-0.5 rounded-[4px] bg-panel-2 p-[3px]">
        <Tip label="Edges" side="left">
          <Button
            variant="ghost"
            size="icon-sm"
            className="data-[on=true]:bg-panel-solid data-[on=true]:text-fg data-[on=true]:shadow-[inset_0_0_0_1px_var(--line)]"
            data-on={edges}
            aria-pressed={edges}
            aria-label="Edges"
            onClick={() => onEdges(!edges)}
          >
            <Box />
          </Button>
        </Tip>
        <Tip label="Fit to view" side="left">
          <Button variant="ghost" size="icon-sm" aria-label="Fit to view" onClick={onFit}>
            <Scan />
          </Button>
        </Tip>
        <Menu position="left-start" offset={8} width={180} classNames={menuStyles}>
          <Menu.Target>
            <Button variant="ghost" size="icon-sm" aria-label="Views">
              <Rotate3d />
            </Button>
          </Menu.Target>
          <Menu.Dropdown>
            <Menu.Label>Views</Menu.Label>
            {VIEWS.map((v) => (
              <Menu.Item key={v.value} onClick={() => onView(v.value)}>
                {v.label}
              </Menu.Item>
            ))}
            <Menu.Divider />
            <Menu.Item onClick={() => onOrtho(!ortho)} rightSection={ortho ? <Check className="size-3.5" /> : null}>
              Orthographic
            </Menu.Item>
          </Menu.Dropdown>
        </Menu>
      </div>
    </div>
  );
}
