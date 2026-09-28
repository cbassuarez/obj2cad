import { useState } from "react";
import { Modal } from "@mantine/core";
import { Button } from "@/components/ui/button";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import type { Inspection } from "@/lib/engine";
import { cap } from "@/lib/format";
import { HINT_UNITS, HINT_UP, UNITS, UPS, type Settings } from "@/lib/settings";

/** First file only: OBJ records neither units nor up axis, so ask once, then remember. */
export function SetupDialog({
  inspection,
  onDone,
  onCancel,
}: {
  inspection: Inspection;
  onDone: (s: Settings, remember: boolean) => void;
  onCancel: () => void;
}) {
  const h = inspection.hints;
  const [s, setS] = useState<Settings>({ units: HINT_UNITS[h.units] ?? "mm", up: HINT_UP[h.up_axis] ?? "as-is" });
  const [remember, setRemember] = useState(true);
  const unitName = UNITS.find((u) => u.value === (HINT_UNITS[h.units] ?? "mm"))!.name;
  const upName = UPS.find((u) => u.value === (HINT_UP[h.up_axis] ?? "as-is"))!.label;

  return (
    <Modal
      opened
      onClose={onCancel}
      centered
      size={480}
      radius="lg"
      title="Two quick questions"
      overlayProps={{ backgroundOpacity: 1, color: "var(--backdrop)", blur: 3 }}
      classNames={{
        content: "!bg-panel-solid !border !border-line",
        header: "!bg-panel-solid !pb-1",
        title: "!font-display !text-[20px] !font-semibold !tracking-tight",
        close: "!text-fg-3 hover:!bg-panel-2",
      }}
    >
      <p className="m-0 mb-5 text-[14px] text-fg-2">
        OBJ files don't record units or which way is up. This only labels the drawing. Coordinates are never scaled or moved.
      </p>

      <fieldset className="m-0 mb-5 border-0 p-0">
        <legend className="mb-2 text-[13px] font-semibold">Units</legend>
        <ToggleGroup type="single" value={s.units} onValueChange={(v) => v && setS({ ...s, units: v as Settings["units"] })} className="grid w-full grid-cols-6">
          {UNITS.map((u) => (
            <ToggleGroupItem key={u.value} value={u.value} className="num" aria-label={u.name}>
              {u.label}
            </ToggleGroupItem>
          ))}
        </ToggleGroup>
        <p className="m-0 mt-2 text-[12.5px] text-info">
          {h.units === "unitless" ? "The file doesn't say. Pick the unit you modeled in." : `Suggested: ${unitName}. ${cap(h.units_reason)}.`}
        </p>
      </fieldset>

      <fieldset className="m-0 border-0 p-0">
        <legend className="mb-2 text-[13px] font-semibold">Orientation</legend>
        <ToggleGroup type="single" value={s.up} onValueChange={(v) => v && setS({ ...s, up: v as Settings["up"] })} className="grid w-full grid-cols-2">
          {UPS.map((u) => (
            <ToggleGroupItem key={u.value} value={u.value} className="flex h-auto flex-col items-start gap-0.5 px-3 py-2.5 text-left">
              <span className="text-[13.5px] font-semibold text-fg">{u.label}</span>
              <span className="text-[12px] font-normal text-fg-3">{u.detail}</span>
            </ToggleGroupItem>
          ))}
        </ToggleGroup>
        <p className="m-0 mt-2 text-[12.5px] text-info">
          {h.exporter ? `Suggested: ${upName}. ${cap(h.up_axis_reason)}.` : "The file doesn't say which app made it. You'll see the result in the preview and can switch any time."}
        </p>
      </fieldset>

      <label className="mt-5 flex cursor-pointer items-center gap-2.5 text-[13px] text-fg-2">
        <input type="checkbox" checked={remember} onChange={(e) => setRemember(e.target.checked)} className="size-4 accent-[var(--accent)]" />
        Remember for next files (you can change these any time in the dock)
      </label>

      <div className="mt-6 flex justify-end gap-2">
        <Button variant="ghost" onClick={onCancel}>
          Cancel
        </Button>
        <Button variant="primary" onClick={() => onDone(s, remember)} data-autofocus>
          Convert
        </Button>
      </div>
    </Modal>
  );
}
