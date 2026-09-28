import { Box, Check, ChevronDown, Compass, Download, Ruler, Scan } from "lucide-react";
import { motion } from "motion/react";
import { Menu } from "@mantine/core";
import { Button } from "@/components/ui/button";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import { Tip } from "@/components/ui/tooltip";
import { UNITS, UPS, type Settings, type Units, type Up } from "@/lib/settings";
import { bytes, cap } from "@/lib/format";

/** An automatic choice, why it was made, and whether the user has changed it. */
export interface AutoChoice<T> {
  detected: T;
  reason: string;
  confident: boolean;
  overridden: boolean;
  reset: () => void;
}

function Group({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="flex flex-col gap-1">
      <span className="px-1 text-[11px] font-medium text-fg-3">{label}</span>
      {children}
    </div>
  );
}

function StatusLine({ icon, children, onReset }: { icon: React.ReactNode; children: React.ReactNode; onReset?: () => void }) {
  return (
    <div className="flex items-start gap-2 text-[12.5px] leading-snug text-fg-3">
      <span className="mt-[2px] shrink-0 [&_svg]:size-3.5">{icon}</span>
      <span className="min-w-0 flex-1">{children}</span>
      {onReset && (
        <Button variant="link" className="h-auto shrink-0 p-0 text-[12.5px]" onClick={onReset}>
          Use detected
        </Button>
      )}
    </div>
  );
}

const unitName = (u: Units) => UNITS.find((x) => x.value === u)!.name;

/** Bottom command dock: the view, the up direction, the download, and what was chosen automatically. */
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
  units,
  orientation,
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
  units: AutoChoice<Units> | null;
  orientation: AutoChoice<Up> | null;
}) {
  const detectedUp = orientation && (orientation.detected === "y-to-z" ? "stand upright" : "keep as exported");
  return (
    <motion.div
      initial={{ opacity: 0, y: 24 }}
      animate={{ opacity: 1, y: 0 }}
      transition={{ type: "spring", stiffness: 300, damping: 30 }}
      className="panel pointer-events-auto flex w-full max-w-[640px] flex-col gap-2.5 p-2.5"
    >
      <div className="flex flex-wrap items-end justify-center gap-x-3 gap-y-2">
        <Group label="Up direction">
          <ToggleGroup type="single" value={settings.up} onValueChange={(v) => v && onUp(v as Up)} aria-label="Up direction">
            {UPS.map((u) => (
              <ToggleGroupItem key={u.value} value={u.value} aria-label={u.label}>
                {u.short}
              </ToggleGroupItem>
            ))}
          </ToggleGroup>
        </Group>

        <Group label="View">
          <div className="flex gap-1 rounded-[4px] bg-panel-2 p-[3px]">
            <Tip label={edges ? "Hide face edges" : "Show face edges"}>
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
        </Group>

        <Button variant="primary" className="h-[42px] min-w-[170px] px-5 text-[15px]" onClick={onExport} disabled={busy || exportBytes === null}>
          <Download />
          Download DXF
          {exportBytes !== null && <span className="num text-[12px] font-normal opacity-70">{bytes(exportBytes)}</span>}
        </Button>
      </div>

      {(units || orientation) && (
        <div className="flex flex-col gap-1.5 border-t border-line-soft px-1 pt-2.5" role="status">
          {units && (
            <StatusLine icon={<Ruler />} onReset={units.overridden ? units.reset : undefined}>
              Units:{" "}
              <Menu position="top-start" offset={6} width={200} classNames={{ dropdown: "panel !p-1", item: "!rounded-[3px] !text-[13px]" }}>
                <Menu.Target>
                  <button type="button" className="inline-flex cursor-pointer items-center gap-0.5 font-semibold text-fg underline decoration-line underline-offset-2 hover:decoration-fg">
                    {settings.units === "unitless" ? "not set" : unitName(settings.units)}
                    <ChevronDown className="size-3.5" />
                  </button>
                </Menu.Target>
                <Menu.Dropdown>
                  <Menu.Label>Units in the file</Menu.Label>
                  {UNITS.map((u) => (
                    <Menu.Item
                      key={u.value}
                      onClick={() => onUnits(u.value)}
                      rightSection={settings.units === u.value ? <Check className="size-3.5" /> : null}
                    >
                      {u.value === "unitless" ? "Not set (unitless)" : cap(u.name)}
                    </Menu.Item>
                  ))}
                </Menu.Dropdown>
              </Menu>
              {units.overridden ? " (you changed this)." : `, because ${units.reason}.`}
            </StatusLine>
          )}
          {orientation && (
            <StatusLine icon={<Compass />} onReset={orientation.overridden ? orientation.reset : undefined}>
              Up direction: {UPS.find((u) => u.value === settings.up)!.short.toLowerCase()}
              {orientation.overridden
                ? ` (you changed this; detected: ${detectedUp}).`
                : orientation.confident
                  ? `, because ${orientation.reason}.`
                  : `, because ${orientation.reason}. If the preview is lying on its side, switch it.`}
            </StatusLine>
          )}
        </div>
      )}
    </motion.div>
  );
}
