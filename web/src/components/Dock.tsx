import { Box, Download, Scan } from "lucide-react";
import { AnimatePresence, motion } from "motion/react";
import { Button } from "@/components/ui/button";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import { Tip } from "@/components/ui/tooltip";
import { UNITS, UPS, type Settings, type Units, type Up } from "@/lib/settings";
import { bytes } from "@/lib/format";

export interface Suggestion {
  text: string;
  apply: () => void;
}

/** Bottom command dock (B): every setting that changes the output, plus export. */
export function Dock({
  settings,
  onUnits,
  onUp,
  edges,
  onEdges,
  onFit,
  onExport,
  exportBytes,
  busy,
  suggestion,
}: {
  settings: Settings;
  onUnits: (u: Units) => void;
  onUp: (u: Up) => void;
  edges: boolean;
  onEdges: (on: boolean) => void;
  onFit: () => void;
  onExport: () => void;
  exportBytes: number | null;
  busy: boolean;
  suggestion: Suggestion | null;
}) {
  return (
    <div className="pointer-events-none flex flex-col items-center gap-2">
      <AnimatePresence>
        {suggestion && (
          <motion.div
            initial={{ opacity: 0, y: 8 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, y: 8 }}
            className="panel pointer-events-auto flex items-center gap-3 !rounded-[4px] py-1.5 pr-1.5 pl-4 text-[13px] text-fg-2"
            role="status"
          >
            <span className="size-1.5 rounded-full bg-info" aria-hidden="true" />
            {suggestion.text}
            <Button variant="secondary" size="sm" className="!rounded-[4px]" onClick={suggestion.apply}>
              Use
            </Button>
          </motion.div>
        )}
      </AnimatePresence>

      <motion.div
        initial={{ opacity: 0, y: 24 }}
        animate={{ opacity: 1, y: 0 }}
        transition={{ type: "spring", stiffness: 300, damping: 30 }}
        className="panel pointer-events-auto flex max-w-full flex-wrap items-center justify-center gap-2 !rounded-[4px] p-2"
      >
        <Tip label="What one unit in your file means. This labels the drawing so CAD scales it correctly; your coordinates are never changed.">
          <ToggleGroup type="single" value={settings.units} onValueChange={(v) => v && onUnits(v as Units)} aria-label="Units">
            {UNITS.map((u) => (
              <ToggleGroupItem key={u.value} value={u.value} className="num px-2.5" aria-label={u.name}>
                {u.label}
              </ToggleGroupItem>
            ))}
          </ToggleGroup>
        </Tip>

        <Tip label={<span>Which way is up. Blender, Maya and most 3D apps export Y-up; CAD is Z-up.<br />Rotating is exact: (x, y, z) → (x, −z, y).</span>}>
          <ToggleGroup type="single" value={settings.up} onValueChange={(v) => v && onUp(v as Up)} aria-label="Orientation">
            {UPS.map((u) => (
              <ToggleGroupItem key={u.value} value={u.value} aria-label={u.label}>
                {u.short}
              </ToggleGroupItem>
            ))}
          </ToggleGroup>
        </Tip>

        <div className="flex gap-1 rounded-[4px] bg-panel-2 p-[3px]">
          <Tip label={edges ? "Hide face edges" : "Show face edges (the file's real polygons)"}>
            <Button
              variant="ghost"
              size="icon-sm"
              className="size-9 data-[on=true]:bg-panel-solid data-[on=true]:text-accent"
              data-on={edges}
              aria-pressed={edges}
              aria-label="Show face edges"
              onClick={() => onEdges(!edges)}
            >
              <Box />
            </Button>
          </Tip>
          <Tip label="Fit to view">
            <Button variant="ghost" size="icon-sm" className="size-9" aria-label="Fit to view" onClick={onFit}>
              <Scan />
            </Button>
          </Tip>
        </div>

        <Button variant="primary" className="h-[42px] min-w-[150px] rounded-[4px] px-5 text-[15px]" onClick={onExport} disabled={busy || exportBytes === null}>
          <Download />
          Export DXF
          {exportBytes !== null && <span className="num text-[12px] font-normal opacity-70">{bytes(exportBytes)}</span>}
        </Button>
      </motion.div>
    </div>
  );
}
