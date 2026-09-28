// Controls shared by the result layouts: each layout arranges the same pieces.
import { Box, Check, ChevronDown, CircleCheck, Download, Rotate3d, Scan } from "lucide-react";
import { motion } from "motion/react";
import { Menu } from "@mantine/core";
import { Button } from "@/components/ui/button";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import { Tip } from "@/components/ui/tooltip";
import type { Decisions, Report } from "@/lib/engine";
import { canPickSaveLocation } from "@/lib/files";
import { bytes, measure } from "@/lib/format";
import { FORMATS, UNITS, UPS, formatInfo, unitName, unitSymbol, type Format, type UpAxis, type Units } from "@/lib/settings";
import { cn } from "@/lib/utils";
import type { ViewName } from "@/viewer/Viewer";

export const menuStyles = { dropdown: "panel !p-1", item: "!rounded-[3px] !text-[13px]", label: "!text-[11px]" };

export const VIEWS: { value: ViewName; label: string }[] = [
  { value: "iso", label: "Isometric" },
  { value: "top", label: "Top" },
  { value: "front", label: "Front" },
  { value: "right", label: "Right" },
];

const UNITS_FROM: Record<Decisions["units_from"], string | undefined> = { chosen: undefined, file: "auto", size: "auto", default: "default", none: undefined };

export const shortFormat = (f: Format) => formatInfo(f).label.replace(" (binary)", "");

/** Everything a result layout shows and changes. */
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
  onEdges: (on: boolean) => void;
  onView: (v: ViewName) => void;
  onOrtho: (on: boolean) => void;
  onFit: () => void;
  onDownload: (pick: boolean) => void;
  onDownloadVisible: () => void;
  onDownloadReport: () => void;
  onAddMtl: () => void;
  onAnother: () => void;
}

export const upChanged = (d: Decisions) => d.up_from === "chosen" && d.up_axis !== d.detected_up_axis;
export const unitsChanged = (d: Decisions) => d.units_from === "chosen" && d.units !== d.detected_units;
export const upLabel = (d: Decisions) => UPS.find((u) => u.value === d.up_axis)!.label;
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

export function UpToggle({ decisions, onUp, className }: Pick<ResultProps, "decisions" | "onUp"> & { className?: string }) {
  return (
    <ToggleGroup type="single" value={decisions.up_axis} onValueChange={(v) => v && onUp(v as UpAxis)} aria-label="Up direction" className={className}>
      {UPS.map((u) => (
        <ToggleGroupItem key={u.value} value={u.value} className="flex-1">
          {u.label}
        </ToggleGroupItem>
      ))}
    </ToggleGroup>
  );
}

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

/** Segmented format choice: the format is always visible, never hidden behind a chevron. */
export function FormatToggle({ format, busy, onFormat, className }: Pick<ResultProps, "format" | "busy" | "onFormat"> & { className?: string }) {
  return (
    <ToggleGroup type="single" value={format} onValueChange={(v) => v && onFormat(v as Format)} aria-label="Format" className={cn("w-full", className)} disabled={busy !== null}>
      {FORMATS.map((f) => (
        <ToggleGroupItem key={f.value} value={f.value} className="flex-1 gap-1 px-2">
          {f.value === "dxf-binary" ? "DXF binary" : f.label}
          {f.beta && <span className="rounded-[2px] bg-warn-soft px-1 text-[10px] font-medium text-warn">beta</span>}
        </ToggleGroupItem>
      ))}
    </ToggleGroup>
  );
}

/** The one filled button on the page. Hidden layers are left out when `visibleOnly`. */
export function DownloadButton({
  report,
  format,
  busy,
  hiddenCount,
  layerCount,
  onDownload,
  onDownloadVisible,
  className,
}: Pick<ResultProps, "report" | "format" | "busy" | "hiddenCount" | "layerCount" | "onDownload" | "onDownloadVisible"> & { className?: string }) {
  const visibleOnly = hiddenCount > 0 && hiddenCount < layerCount;
  return (
    <Button variant="primary" size="lg" className={cn("w-full justify-between px-5", className)} onClick={() => (visibleOnly ? onDownloadVisible() : onDownload(false))} disabled={busy !== null}>
      <span className="flex items-center gap-2.5">
        <Download />
        Download {shortFormat(format)}
      </span>
      <span className="num text-[12px] font-normal opacity-80">{busy !== null ? "…" : visibleOnly ? `${layerCount - hiddenCount} of ${layerCount} layers` : bytes(report.output.bytes)}</span>
    </Button>
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
  const visibleOnly = hiddenCount > 0 && hiddenCount < layerCount;
  if (downloaded && busy === null)
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
      {visibleOnly ? <span>Hidden layers are left out</span> : <span>Everything in the viewer is included</span>}
      {canPickSaveLocation() && !visibleOnly && (
        <Button variant="link" className="ml-auto h-auto px-0 text-[12.5px]" onClick={() => onDownload(true)} disabled={busy !== null}>
          Save as…
        </Button>
      )}
    </div>
  );
}

/** Edges, fit, named views and projection. `icons` draws a compact icon-only strip. */
export function ViewTools({
  edges,
  ortho,
  onEdges,
  onFit,
  onView,
  onOrtho,
  icons = false,
  vertical = false,
}: Pick<ResultProps, "edges" | "ortho" | "onEdges" | "onFit" | "onView" | "onOrtho"> & { icons?: boolean; vertical?: boolean }) {
  const on = "data-[on=true]:bg-panel-solid data-[on=true]:text-fg data-[on=true]:shadow-[inset_0_0_0_1px_var(--line)]";
  const side = vertical ? "left" : "top";
  const item = (label: string, node: React.ReactElement) =>
    icons ? (
      <Tip label={label} side={side}>
        {node}
      </Tip>
    ) : (
      node
    );
  return (
    <div className={cn("flex gap-0.5 rounded-[4px] bg-panel-2 p-[3px]", vertical && "flex-col")}>
      {item(
        "Edges",
        <Button
          variant="ghost"
          size={icons ? "icon-sm" : "sm"}
          className={cn(!icons && "h-8 flex-1 px-2.5", on)}
          data-on={edges}
          aria-pressed={edges}
          aria-label="Edges"
          onClick={() => onEdges(!edges)}
        >
          <Box />
          {!icons && "Edges"}
        </Button>,
      )}
      {item(
        "Fit to view",
        <Button variant="ghost" size={icons ? "icon-sm" : "sm"} className={cn(!icons && "h-8 flex-1 px-2.5")} aria-label="Fit" onClick={onFit}>
          <Scan />
          {!icons && "Fit"}
        </Button>,
      )}
      <Menu position={vertical ? "left-start" : "bottom-end"} offset={8} width={180} classNames={menuStyles}>
        <Menu.Target>
          <Button variant="ghost" size={icons ? "icon-sm" : "sm"} className={cn(!icons && "h-8 flex-1 px-2.5")} aria-label="Views">
            {icons ? <Rotate3d /> : "Views"}
            {!icons && <ChevronDown />}
          </Button>
        </Menu.Target>
        <Menu.Dropdown>
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
  );
}
