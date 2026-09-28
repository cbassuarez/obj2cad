import { Eye, EyeOff } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Tip } from "@/components/ui/tooltip";
import type { Report } from "@/lib/engine";
import { fmt } from "@/lib/format";
import { cn } from "@/lib/utils";

// Same palette the engine uses for layers without a material color (crates/obj2cad-wasm).
const SWATCHES = ["#b0b8c4", "#7c9ec9", "#c9966e", "#80b496", "#be8cb4", "#d2be78", "#78aab4", "#b48282"];

export function LayersCard({
  report,
  hidden,
  onToggle,
}: {
  report: Report;
  hidden: Set<number>;
  onToggle: (layer: number) => void;
}) {
  const rows = report.layers
    .map((l, i) => ({ ...l, i }))
    .filter((l) => l.faces + l.polylines + l.points > 0 || l.source !== "");
  const total = (l: (typeof rows)[number]) => l.faces + l.polylines + l.points;

  return (
    <section className="panel pointer-events-auto flex max-h-full min-h-0 w-full flex-col overflow-hidden" aria-label="Layers">
      <div className="flex items-baseline justify-between px-4 pt-4 pb-2">
        <h2 className="label m-0">Layers · {rows.length}</h2>
        <span className="text-[11.5px] text-fg-3">faces</span>
      </div>
      <ul className="m-0 min-h-0 flex-1 list-none overflow-y-auto px-2 pb-2">
        {rows.map((l) => {
          const off = hidden.has(l.i);
          return (
            <li key={l.i} className={cn("flex items-center gap-2.5 rounded-[3px] py-1 pr-1 pl-2 hover:bg-panel-2", off && "opacity-50")}>
              <span className="size-2.5 shrink-0 rounded-[3px] border border-black/20" style={{ background: SWATCHES[l.i % SWATCHES.length] }} />
              <Tip label={l.name === "0" ? "AutoCAD's layer 0: faces that had no object or group name" : l.name !== l.source ? `Renamed from “${l.source}” to be a valid AutoCAD layer name` : `From “${l.source}” in your file`} side="right">
                <span className="min-w-0 flex-1 truncate text-[13.5px]">{l.name === "0" ? "Default layer" : l.name}</span>
              </Tip>
              <span className="num text-[12px] text-fg-3">{fmt(total(l))}</span>
              <Button variant="ghost" size="icon-sm" onClick={() => onToggle(l.i)} aria-label={`${off ? "Show" : "Hide"} layer ${l.name} in preview`} aria-pressed={!off}>
                {off ? <EyeOff /> : <Eye />}
              </Button>
            </li>
          );
        })}
      </ul>
      {hidden.size > 0 && <p className="m-0 border-t border-line-soft px-4 py-2.5 text-[12px] text-fg-3">Hidden in the preview only. The DXF always has every layer.</p>}
    </section>
  );
}
