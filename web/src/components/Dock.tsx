import { Box, Check, ChevronDown, CircleCheck, Download, Scan } from "lucide-react";
import { motion } from "motion/react";
import { Menu } from "@mantine/core";
import { Button } from "@/components/ui/button";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import type { Decisions, Report } from "@/lib/engine";
import { bytes, measure } from "@/lib/format";
import { FORMATS, UNITS, UPS, formatInfo, unitName, unitSymbol, type Format, type UpAxis, type Units } from "@/lib/settings";
import { canPickSaveLocation } from "@/lib/files";
import type { ViewName } from "@/viewer/Viewer";

const menuStyles = { dropdown: "panel !p-1", item: "!rounded-[3px] !text-[13px]", label: "!text-[11px]" };

function Group({ label, tag, onReset, children }: { label: string; tag?: string; onReset?: () => void; children: React.ReactNode }) {
  return (
    <div className="flex flex-col gap-1">
      <span className="flex items-baseline gap-1.5 px-1 text-[11px] font-medium text-fg-3">
        {label}
        {onReset ? (
          <button type="button" className="cursor-pointer font-semibold text-accent hover:underline" onClick={onReset}>
            Reset
          </button>
        ) : (
          tag && <span className="text-fg-3/80">· {tag}</span>
        )}
      </span>
      {children}
    </div>
  );
}

const VIEWS: { value: ViewName; label: string }[] = [
  { value: "iso", label: "Isometric" },
  { value: "top", label: "Top" },
  { value: "front", label: "Front" },
  { value: "right", label: "Right" },
];

const UNITS_FROM: Record<Decisions["units_from"], string | undefined> = { chosen: undefined, file: "auto", size: "auto", default: "default", none: undefined };

/** Bottom command dock: up direction, view, the download, units and real size. */
export function Dock({
  report,
  decisions,
  format,
  busy,
  unitsStated,
  houseUnits,
  edges,
  ortho,
  downloaded,
  onUp,
  onUnits,
  onHouseUnits,
  onEdges,
  onView,
  onOrtho,
  onFit,
  onFormat,
  curves,
  onCurves,
  onDownload,
  onAnother,
}: {
  report: Report;
  decisions: Decisions;
  format: Format;
  busy: string | null;
  /** The file's exporter states its units. */
  unitsStated: boolean;
  houseUnits: Units | null;
  edges: boolean;
  ortho: boolean;
  downloaded: string | null;
  onUp: (u: UpAxis | null) => void;
  onUnits: (u: Units | null) => void;
  onHouseUnits: (u: Units | null) => void;
  onEdges: (on: boolean) => void;
  onView: (v: ViewName) => void;
  onOrtho: (on: boolean) => void;
  onFit: () => void;
  onFormat: (f: Format) => void;
  /** Also write recognized curved surfaces. */
  curves: boolean;
  onCurves: (on: boolean) => void;
  onDownload: (pick: boolean) => void;
  onAnother: () => void;
}) {
  const fmtInfo = formatInfo(format);
  const size = report.output.bounds && report.output.bounds[1].map((hi, a) => hi - report.output.bounds![0][a]);
  const symbol = unitSymbol(decisions.units);
  const upChanged = decisions.up_from === "chosen" && decisions.up_axis !== decisions.detected_up_axis;
  const unitsChanged = decisions.units_from === "chosen" && decisions.units !== decisions.detected_units;

  return (
    <motion.div
      initial={{ opacity: 0, y: 24 }}
      animate={{ opacity: 1, y: 0 }}
      transition={{ type: "spring", stiffness: 300, damping: 30 }}
      className="panel pointer-events-auto flex w-full max-w-[680px] flex-col gap-2.5 p-2.5"
    >
      <div className="flex flex-wrap items-end justify-center gap-x-3 gap-y-2">
        <Group label="Up direction" tag={decisions.up_from === "detected" ? "auto" : undefined} onReset={upChanged ? () => onUp(null) : undefined}>
          <ToggleGroup type="single" value={decisions.up_axis} onValueChange={(v) => v && onUp(v as UpAxis)} aria-label="Up direction">
            {UPS.map((u) => (
              <ToggleGroupItem key={u.value} value={u.value}>
                {u.label}
              </ToggleGroupItem>
            ))}
          </ToggleGroup>
        </Group>

        <Group label="View">
          <div className="flex gap-0.5 rounded-[4px] bg-panel-2 p-[3px]">
            <Button
              variant="ghost"
              size="sm"
              className="h-9 px-2.5 data-[on=true]:bg-panel-solid data-[on=true]:text-fg data-[on=true]:shadow-[inset_0_0_0_1px_var(--line)]"
              data-on={edges}
              aria-pressed={edges}
              onClick={() => onEdges(!edges)}
            >
              <Box />
              Edges
            </Button>
            <Button variant="ghost" size="sm" className="h-9 px-2.5" onClick={onFit}>
              <Scan />
              Fit
            </Button>
            <Menu position="top" offset={8} width={180} classNames={menuStyles}>
              <Menu.Target>
                <Button variant="ghost" size="sm" className="h-9 px-2.5" aria-label="Views">
                  Views
                  <ChevronDown />
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
        </Group>

        <div className="flex">
          <Button variant="primary" className="h-[42px] min-w-[180px] rounded-r-none px-5 text-[15px]" onClick={() => onDownload(false)} disabled={busy !== null}>
            <Download />
            Download {fmtInfo.label.replace(" (binary)", "")}
            <span className="num text-[12px] font-normal opacity-75">{busy === null ? bytes(report.output.bytes) : ""}</span>
          </Button>
          <Menu position="top-end" offset={8} width={220} classNames={menuStyles}>
            <Menu.Target>
              <Button variant="primary" className="h-[42px] rounded-l-none border-l border-white/25 px-2" aria-label="Download options" disabled={busy !== null}>
                <ChevronDown />
              </Button>
            </Menu.Target>
            <Menu.Dropdown>
              <Menu.Label>Format</Menu.Label>
              {FORMATS.map((f) => (
                <Menu.Item key={f.value} onClick={() => onFormat(f.value)} rightSection={format === f.value ? <Check className="size-3.5" /> : null}>
                  {f.label}
                  {f.beta && <span className="ml-1.5 rounded-[2px] bg-warn-soft px-1 py-px text-[10.5px] font-medium text-warn">beta</span>}
                </Menu.Item>
              ))}
              <Menu.Divider />
              <Menu.Item onClick={() => onCurves(!curves)} rightSection={curves ? <Check className="size-3.5" /> : null}>
                Curved surfaces
              </Menu.Item>
              {canPickSaveLocation() && (
                <>
                  <Menu.Divider />
                  <Menu.Item onClick={() => onDownload(true)}>Save as…</Menu.Item>
                </>
              )}
            </Menu.Dropdown>
          </Menu>
        </div>
      </div>

      <div className="flex flex-wrap items-center gap-x-4 gap-y-1 border-t border-line-soft px-1 pt-2.5 text-[12.5px] text-fg-3" role="status">
        <span className="flex items-center gap-1.5">
          Units
          <Menu position="top-start" offset={6} width={230} classNames={menuStyles}>
            <Menu.Target>
              <button type="button" className="inline-flex cursor-pointer items-center gap-0.5 font-semibold text-fg underline decoration-line underline-offset-2 hover:decoration-fg">
                {decisions.units === "unitless" ? "None" : unitName(decisions.units)}
                <ChevronDown className="size-3.5" />
              </button>
            </Menu.Target>
            <Menu.Dropdown>
              {UNITS.map((u) => (
                <Menu.Item key={u.value} onClick={() => onUnits(u.value)} rightSection={decisions.units === u.value ? <Check className="size-3.5" /> : null}>
                  {u.name}
                </Menu.Item>
              ))}
              {!unitsStated && decisions.units !== "unitless" && (
                <>
                  <Menu.Divider />
                  <Menu.Item
                    onClick={() => onHouseUnits(houseUnits === decisions.units ? null : decisions.units)}
                    rightSection={houseUnits === decisions.units ? <Check className="size-3.5" /> : null}
                  >
                    Use for files without units
                  </Menu.Item>
                </>
              )}
            </Menu.Dropdown>
          </Menu>
          {unitsChanged ? (
            <button type="button" className="cursor-pointer font-semibold text-accent hover:underline" onClick={() => onUnits(null)}>
              Reset
            </button>
          ) : (
            UNITS_FROM[decisions.units_from] && <span>· {UNITS_FROM[decisions.units_from]}</span>
          )}
        </span>
        {size && (
          <span className="num">
            Size in CAD {size.map(measure).join(" × ")}
            {symbol ? ` ${symbol}` : ""}
          </span>
        )}
        {busy && <span className="ml-auto text-accent">{busy}</span>}
      </div>

      {downloaded && busy === null && (
        <motion.div initial={{ opacity: 0, height: 0 }} animate={{ opacity: 1, height: "auto" }} className="flex items-center gap-2 px-1 text-[13px]">
          <CircleCheck className="size-4 shrink-0 text-exact" />
          <span className="min-w-0 truncate">
            Downloaded <span className="num">{downloaded}</span>
          </span>
          <Button variant="link" className="ml-auto text-[13px]" onClick={onAnother}>
            Convert another file
          </Button>
        </motion.div>
      )}
    </motion.div>
  );
}
