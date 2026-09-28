import { Check, ChevronDown, Download, Eye, EyeOff } from "lucide-react";
import { Menu } from "@mantine/core";
import { Button } from "@/components/ui/button";
import type { Report } from "@/lib/engine";
import { fmt } from "@/lib/format";
import { LAYER_MODES, type LayerMode } from "@/lib/settings";
import { cn } from "@/lib/utils";

export function LayersCard({
  report,
  mode,
  hidden,
  busy,
  onMode,
  onToggle,
  onDownloadVisible,
}: {
  report: Report;
  mode: LayerMode;
  hidden: Set<number>;
  busy: boolean;
  onMode: (m: LayerMode) => void;
  onToggle: (layer: number) => void;
  onDownloadVisible: () => void;
}) {
  const rows = report.layers.map((l, i) => ({ ...l, i })).filter((l) => l.faces + l.polylines + l.points > 0);
  const total = (l: (typeof rows)[number]) => l.faces + l.polylines + l.points;
  const onlyFaces = report.output.polylines + report.output.points === 0;
  const shown = rows.filter((l) => !hidden.has(l.i)).length;

  return (
    <section className="panel pointer-events-auto flex max-h-full min-h-0 w-full flex-col overflow-hidden" aria-label="Layers">
      <div className="flex items-center justify-between gap-2 px-4 pt-3 pb-2">
        <h2 className="label m-0">Layers · {rows.length}</h2>
        <Menu position="bottom-end" offset={6} width={170} classNames={{ dropdown: "panel !p-1", item: "!rounded-[3px] !text-[13px]", label: "!text-[11px]" }}>
          <Menu.Target>
            <button type="button" className="inline-flex cursor-pointer items-center gap-0.5 text-[12.5px] text-fg-2 hover:text-fg" disabled={busy}>
              From {LAYER_MODES.find((m) => m.value === mode)!.label.toLowerCase()}
              <ChevronDown className="size-3.5" />
            </button>
          </Menu.Target>
          <Menu.Dropdown>
            <Menu.Label>Layers from</Menu.Label>
            {LAYER_MODES.map((m) => (
              <Menu.Item key={m.value} onClick={() => onMode(m.value)} rightSection={mode === m.value ? <Check className="size-3.5" /> : null}>
                {m.label}
              </Menu.Item>
            ))}
          </Menu.Dropdown>
        </Menu>
      </div>
      <div className="flex justify-end px-4 pb-1 text-[11px] text-fg-3">{onlyFaces ? "faces" : "shapes"}</div>
      <ul className="m-0 min-h-0 flex-1 list-none overflow-y-auto px-2 pb-2">
        {rows.map((l) => {
          const off = hidden.has(l.i);
          return (
            <li key={`${l.i}-${l.name}`} className={cn("flex items-center gap-2.5 rounded-[3px] py-1 pr-1 pl-2 hover:bg-panel-2", off && "opacity-50")}>
              <span className="size-3 shrink-0 rounded-[2px] border border-black/20" style={{ background: l.color }} />
              <span className="min-w-0 flex-1 truncate text-[13.5px]" title={l.name}>
                {l.name}
              </span>
              {l.entity_colors.length > 0 && (
                <span className="flex shrink-0 gap-0.5" aria-hidden="true">
                  {l.entity_colors.slice(0, 4).map((c) => (
                    <span key={c} className="size-2 rounded-full border border-black/20" style={{ background: c }} />
                  ))}
                </span>
              )}
              <span className="num text-[12px] text-fg-3">{fmt(total(l))}</span>
              <Button variant="ghost" size="icon-sm" onClick={() => onToggle(l.i)} aria-label={`${off ? "Show" : "Hide"} layer ${l.name}`} aria-pressed={!off}>
                {off ? <EyeOff /> : <Eye />}
              </Button>
            </li>
          );
        })}
      </ul>
      {hidden.size > 0 && shown > 0 && (
        <div className="border-t border-line-soft p-2">
          <Button variant="ghost" size="sm" className="w-full" onClick={onDownloadVisible} disabled={busy}>
            <Download />
            Download visible layers only ({shown} of {rows.length})
          </Button>
        </div>
      )}
    </section>
  );
}
