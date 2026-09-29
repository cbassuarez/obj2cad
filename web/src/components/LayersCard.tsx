import { useEffect, useRef, useState } from "react";
import { Check, ChevronDown, PencilLine, Search } from "lucide-react";
import { Menu } from "@mantine/core";
import { Button } from "@/components/ui/button";
import { Tip } from "@/components/ui/tooltip";
import type { Inspection, Report } from "@/lib/engine";
import { fmt } from "@/lib/format";
import { LAYER_MODES, type LayerMode } from "@/lib/settings";
import { cn } from "@/lib/utils";

/** Rows are virtualized above this many layers (exports can have thousands of objects). */
const VIRTUAL = 300;
const ROW = 32;
/** A filter box appears above this many layers. */
const FILTER = 12;

const NOUN: Record<LayerMode, string> = { objects: "object", groups: "group", materials: "material", single: "" };

const plural = (n: number, one: string) => `${fmt(n)} ${one}${n === 1 ? "" : "s"}`;

/** "6 faces · 2 lines": what a layer holds, for its tooltip. */
const contents = (l: { faces: number; polylines: number; points: number; surfaces: number }) =>
  [l.faces && plural(l.faces, "face"), l.surfaces && plural(l.surfaces, "curved surface"), l.polylines && plural(l.polylines, "line"), l.points && plural(l.points, "point")]
    .filter(Boolean)
    .join(" · ");

/**
 * The drawing's layers. A ticked layer is in the drawing; an unticked one is left out of
 * the download and hidden in the viewer. Hovering a row (or picking the model) lights that
 * layer up in the viewer without changing anything.
 */
export function LayersCard({
  report,
  mode,
  hidden,
  busy,
  selected,
  inspection,
  onMode,
  onHidden,
  onSelect,
  onHover,
}: {
  report: Report;
  mode: LayerMode;
  /** Layers left out (indices into `report.layers`). */
  hidden: Set<number>;
  busy: boolean;
  /** The layer picked in the viewer or here. */
  selected: number | null;
  inspection: Inspection | null;
  onMode: (m: LayerMode) => void;
  onHidden: (next: Set<number>) => void;
  onSelect: (layer: number | null) => void;
  onHover: (layer: number | null) => void;
}) {
  const rows = report.layers.map((l, i) => ({ ...l, i, total: l.faces + l.polylines + l.points + l.surfaces })).filter((l) => l.total > 0);
  const [query, setQuery] = useState("");
  const q = query.trim().toLowerCase();
  const shownRows = q ? rows.filter((l) => l.name.toLowerCase().includes(q) || l.source.toLowerCase().includes(q)) : rows;
  const included = rows.filter((l) => !hidden.has(l.i)).length;
  const single = rows.length === 1;
  const onlyFaces = report.output.polylines + report.output.points + (report.curves?.length ?? 0) === 0;
  const list = useRef<HTMLUListElement>(null);
  const [scroll, setScroll] = useState({ top: 0, height: 400 });
  const virtual = shownRows.length > VIRTUAL;

  const counts: Record<LayerMode, number | null> = {
    objects: inspection?.objects ?? null,
    groups: inspection?.groups ?? null,
    materials: inspection?.materials.length ?? null,
    single: null,
  };

  const set = (layers: number[], include: boolean) => {
    const next = new Set(hidden);
    for (const i of layers) {
      if (include) next.delete(i);
      else next.add(i);
    }
    onHidden(next);
  };
  const toggle = (i: number, solo: boolean) => {
    if (!solo) return set([i], hidden.has(i));
    // ⌥/Alt-click: only this layer; again on the only one puts every layer back.
    const alone = included === 1 && !hidden.has(i);
    onHidden(alone ? new Set() : new Set(rows.filter((l) => l.i !== i).map((l) => l.i)));
  };
  const actions = [
    [
      "All",
      () =>
        set(
          shownRows.map((l) => l.i),
          true,
        ),
    ],
    [
      "None",
      () =>
        set(
          shownRows.map((l) => l.i),
          false,
        ),
    ],
    ["Invert", () => onHidden(new Set(rows.filter((l) => shownRows.includes(l) !== hidden.has(l.i)).map((l) => l.i)))],
  ] as const;

  // Bring the layer picked in the viewer into view.
  useEffect(() => {
    const el = list.current;
    if (selected === null || !el) return;
    const at = shownRows.findIndex((l) => l.i === selected);
    if (at < 0) return;
    if (virtual) {
      if (at * ROW < el.scrollTop || (at + 1) * ROW > el.scrollTop + el.clientHeight) el.scrollTop = at * ROW - el.clientHeight / 2;
    } else el.querySelector(`[data-layer="${selected}"]`)?.scrollIntoView({ block: "nearest" });
    // Only when the pick changes.
  }, [selected]);

  // Arrow keys move between rows; Space ticks (a native checkbox).
  const onKeyDown = (e: React.KeyboardEvent) => {
    if (e.key !== "ArrowDown" && e.key !== "ArrowUp") return;
    const boxes = [...(list.current?.querySelectorAll<HTMLInputElement>("input[type=checkbox]") ?? [])];
    const next = boxes[boxes.indexOf(document.activeElement as HTMLInputElement) + (e.key === "ArrowDown" ? 1 : -1)];
    if (next) {
      e.preventDefault();
      next.focus();
    }
  };

  const first = virtual ? Math.max(0, Math.floor(scroll.top / ROW) - 10) : 0;
  const last = virtual ? Math.min(shownRows.length, Math.ceil((scroll.top + scroll.height) / ROW) + 10) : shownRows.length;

  return (
    <section className="panel pointer-events-auto flex max-h-full min-h-0 w-full flex-col overflow-hidden" aria-label="Layers">
      <div className="flex items-center justify-between gap-2 px-4 pt-3 pb-2">
        <h2 className="label m-0">Layers · {fmt(rows.length)}</h2>
        <Menu position="bottom-end" offset={6} width={220} classNames={{ dropdown: "panel !p-1", item: "!rounded-[3px] !text-[13px]", label: "!text-[11px]" }}>
          <Menu.Target>
            <button type="button" className="inline-flex cursor-pointer items-center gap-0.5 text-[12.5px] text-fg-2 hover:text-fg" disabled={busy}>
              From {LAYER_MODES.find((m) => m.value === mode)!.label.toLowerCase()}
              <ChevronDown className="size-3.5" />
            </button>
          </Menu.Target>
          <Menu.Dropdown>
            <Menu.Label>Layers from</Menu.Label>
            {LAYER_MODES.map((m) => (
              <Menu.Item
                key={m.value}
                onClick={() => onMode(m.value)}
                rightSection={
                  mode === m.value ? (
                    <Check className="size-3.5" />
                  ) : counts[m.value] ? (
                    <span className="num text-[11.5px] text-fg-3" title={`${plural(counts[m.value]!, NOUN[m.value])} in the file`}>
                      {fmt(counts[m.value]!)}
                    </span>
                  ) : counts[m.value] === 0 ? (
                    <span className="text-[11.5px] text-fg-3">none in file</span>
                  ) : null
                }
              >
                {m.label}
              </Menu.Item>
            ))}
            {hidden.size > 0 && <Menu.Label>Changing this puts every layer back in.</Menu.Label>}
          </Menu.Dropdown>
        </Menu>
      </div>

      {rows.length > FILTER && (
        <label className="mx-3 mb-2 flex h-8 items-center gap-2 rounded-[3px] border border-line bg-panel-solid px-2 text-[13px] focus-within:border-fg-3">
          <Search className="size-3.5 shrink-0 text-fg-3" aria-hidden="true" />
          <input
            className="min-w-0 flex-1 bg-transparent outline-none placeholder:text-fg-3"
            placeholder={`Filter ${fmt(rows.length)} layers`}
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            aria-label="Filter layers"
          />
          {q && <span className="num text-[11.5px] text-fg-3">{fmt(shownRows.length)}</span>}
        </label>
      )}

      <div className="flex items-center gap-2 px-4 pb-1 text-[11.5px] text-fg-3">
        {!single &&
          actions.map(([label, act], n) => (
            <span key={label} className="flex items-center gap-2">
              {n > 0 && <span aria-hidden="true">·</span>}
              <button type="button" className="cursor-pointer font-medium text-fg-2 hover:text-accent" onClick={act} title={q ? `${label}, of the ${fmt(shownRows.length)} shown` : undefined}>
                {label}
              </button>
            </span>
          ))}
        <span className="ml-auto">{onlyFaces ? "faces" : "shapes"}</span>
      </div>

      <ul
        ref={list}
        className="m-0 min-h-0 flex-1 list-none overflow-y-auto px-2 pb-2"
        onScroll={(e) => virtual && setScroll({ top: e.currentTarget.scrollTop, height: e.currentTarget.clientHeight })}
        onMouseLeave={() => onHover(null)}
        onKeyDown={onKeyDown}
        aria-label="Layers in the drawing"
      >
        {virtual && <li aria-hidden="true" style={{ height: first * ROW }} />}
        {shownRows.slice(first, last).map((l) => {
          const off = hidden.has(l.i);
          const renamed = l.source !== "" && l.source !== l.name;
          const loose = l.source === "" && mode !== "single" && l.name !== "0";
          return (
            <li
              key={`${l.i}-${l.name}`}
              data-layer={l.i}
              className={cn("flex h-8 items-center gap-2.5 rounded-[3px] px-2 hover:bg-panel-2", selected === l.i && "bg-accent-soft hover:bg-accent-soft")}
              onMouseEnter={() => onHover(l.i)}
            >
              {!single && (
                <input
                  type="checkbox"
                  className="size-3.5 shrink-0 cursor-pointer accent-[var(--accent)]"
                  checked={!off}
                  onChange={() => undefined}
                  onClick={(e) => toggle(l.i, e.altKey)}
                  onFocus={() => onHover(l.i)}
                  onBlur={() => onHover(null)}
                  aria-label={`${l.name} in the drawing`}
                  title="In the drawing (⌥/Alt-click: only this layer)"
                />
              )}
              <span className={cn("size-3 shrink-0 rounded-[2px] border border-black/20", off && "opacity-40")} style={{ background: l.color }} title={`Layer color ${l.color}`} />
              <button
                type="button"
                className={cn("min-w-0 flex-1 cursor-pointer truncate text-left text-[13.5px]", off && "text-fg-3 line-through decoration-fg-3/50")}
                onClick={() => onSelect(selected === l.i ? null : l.i)}
                title={loose ? `${l.name}: geometry not in any ${NOUN[mode]}` : l.name}
              >
                {l.name}
              </button>
              {renamed && (
                <Tip label={<>Renamed for CAD. In the OBJ: “{l.source}”</>}>
                  <span className="shrink-0 text-fg-3" aria-label={`Renamed from ${l.source}`} role="img" tabIndex={0}>
                    <PencilLine className="size-3" aria-hidden="true" />
                  </span>
                </Tip>
              )}
              {l.entity_colors.length > 0 && (
                <span className="flex shrink-0 gap-0.5" title={`Material colors: ${l.entity_colors.join(", ")}`}>
                  {l.entity_colors.slice(0, 4).map((c) => (
                    <span key={c} className="size-2 rounded-full border border-black/20" style={{ background: c }} />
                  ))}
                </span>
              )}
              <span className="num shrink-0 text-[12px] text-fg-3" title={contents(l)}>
                {fmt(l.total)}
              </span>
            </li>
          );
        })}
        {virtual && <li aria-hidden="true" style={{ height: (shownRows.length - last) * ROW }} />}
        {q && shownRows.length === 0 && <li className="px-2 py-3 text-[12.5px] text-fg-3">No layer matches “{query}”</li>}
      </ul>

      {hidden.size > 0 && (
        <div className="flex items-baseline gap-3 border-t border-line-soft px-4 py-2 text-[12px] text-fg-3">
          <span className="min-w-0 flex-1">{included > 0 ? `${fmt(included)} of ${fmt(rows.length)} in the drawing` : "No layer is in the drawing"}</span>
          <Button variant="link" className="h-auto shrink-0 px-0 text-[12px]" onClick={() => onHidden(new Set())}>
            Include all
          </Button>
        </div>
      )}
    </section>
  );
}
