// Layout B, "Tabbed": the verdict and the download sit at the top of the pane, where the
// eye lands first. Everything else is filed under three tabs (Adjust, Result, Details),
// so the report and the hashes are one deliberate click away from the main action.
// The view tools live in a status strip at the foot of the pane.
import { useState } from "react";
import { Menu } from "@mantine/core";
import { Tabs } from "radix-ui";
import { Check, ChevronDown } from "lucide-react";
import { Seal, Stat, Notes, TechnicalDetails } from "@/components/Inspector";
import { Button } from "@/components/ui/button";
import { DownloadAfter, DownloadButton, Provenance, UnitsMenu, UpToggle, ViewTools, menuStyles, sizeText, unitsChanged, unitsTag, upChanged, upTag, type ResultProps } from "@/components/controls";
import { bytes, fmt } from "@/lib/format";
import { FORMATS } from "@/lib/settings";
import { summarize } from "@/lib/summary";
import { cn } from "@/lib/utils";

function Field({ label, aside, children }: { label: string; aside?: React.ReactNode; children: React.ReactNode }) {
  return (
    <div className="flex flex-col gap-1.5">
      <div className="flex items-baseline justify-between gap-2 text-[12.5px] font-medium text-fg-2">
        {label}
        {aside}
      </div>
      {children}
    </div>
  );
}

const tab =
  "relative flex-1 cursor-pointer px-2 pt-2.5 pb-2 text-[13px] font-medium text-fg-3 outline-none hover:text-fg data-[state=active]:text-fg " +
  "after:absolute after:inset-x-2 after:-bottom-px after:h-[2px] after:rounded-full data-[state=active]:after:bg-fg";

export function TabbedPanel(p: ResultProps) {
  const { report, decisions } = p;
  const s = summarize(report);
  const size = sizeText(report, decisions);
  const notes = s.leftOut.length + (s.loosePoints ? 1 : 0) + (s.needsMtl ? 1 : 0);
  const [value, setValue] = useState(s.status === "partial" ? "result" : "adjust");

  return (
    <aside className="panel pointer-events-auto flex max-h-full min-h-0 w-full flex-col overflow-hidden" aria-label="Result">
      <div className="flex flex-col gap-3 border-b border-line px-5 pt-5 pb-4">
        <Seal compact summary={s} id={`${report.parity_hash}${s.status}`} />
        <div className="flex">
          <DownloadButton {...p} className="flex-1 rounded-r-none" />
          <Menu position="bottom-end" offset={6} width={220} classNames={menuStyles}>
            <Menu.Target>
              <Button variant="primary" size="lg" className="rounded-l-none border-l border-white/25 px-3" aria-label="Format" disabled={p.busy !== null}>
                <ChevronDown />
              </Button>
            </Menu.Target>
            <Menu.Dropdown>
              <Menu.Label>Format</Menu.Label>
              {FORMATS.map((f) => (
                <Menu.Item key={f.value} onClick={() => p.onFormat(f.value)} rightSection={p.format === f.value ? <Check className="size-3.5" /> : null}>
                  {f.label}
                  {f.beta && <span className="ml-1.5 rounded-[2px] bg-warn-soft px-1 py-px text-[10.5px] font-medium text-warn">beta</span>}
                </Menu.Item>
              ))}
            </Menu.Dropdown>
          </Menu>
        </div>
        <DownloadAfter {...p} />
      </div>

      <Tabs.Root value={value} onValueChange={setValue} className="flex min-h-0 flex-1 flex-col">
        <Tabs.List className="flex border-b border-line-soft px-3" aria-label="Result sections">
          <Tabs.Trigger value="adjust" className={tab}>
            Adjust
          </Tabs.Trigger>
          <Tabs.Trigger value="result" className={tab}>
            Result
            {notes > 0 && <span className={cn("num ml-1.5 rounded-full px-1.5 text-[11px]", s.leftOut.length ? "bg-warn-soft text-warn" : "bg-panel-2 text-fg-3")}>{notes}</span>}
          </Tabs.Trigger>
          <Tabs.Trigger value="details" className={tab}>
            Details
          </Tabs.Trigger>
        </Tabs.List>

        <div className="min-h-0 flex-1 overflow-y-auto">
          <Tabs.Content value="adjust" className="flex flex-col gap-5 px-5 py-4 outline-none">
            <Field label="Up direction" aside={<Provenance tag={upTag(decisions)} onReset={upChanged(decisions) ? () => p.onUp(null) : undefined} />}>
              <UpToggle decisions={decisions} onUp={p.onUp} className="w-full" />
            </Field>
            <Field label="Units" aside={<Provenance tag={unitsTag(decisions)} onReset={unitsChanged(decisions) ? () => p.onUnits(null) : undefined} />}>
              <div className="text-[13.5px]">
                <UnitsMenu decisions={decisions} unitsStated={p.unitsStated} houseUnits={p.houseUnits} onUnits={p.onUnits} onHouseUnits={p.onHouseUnits} />
              </div>
            </Field>
          </Tabs.Content>

          <Tabs.Content value="result" className="flex flex-col gap-4 px-5 py-4 outline-none">
            <div className="grid grid-cols-3 gap-3">
              <Stat value={fmt(s.shapes.count)} label={s.shapes.noun} />
              <Stat value={fmt(p.layerCount)} label={p.layerCount === 1 ? "layer" : "layers"} />
              <Stat value={bytes(report.output.bytes)} label="file size" />
            </div>
            <Notes summary={s} onKeepLoose={p.onKeepLoose} onAddMtl={p.onAddMtl} />
            {notes === 0 && s.notIncluded.length === 0 && <p className="m-0 text-[13px] text-fg-3">Every face, line and point in the file is in the drawing.</p>}
          </Tabs.Content>

          <Tabs.Content value="details" className="px-5 py-4 outline-none">
            <TechnicalDetails
              plain
              report={report}
              ms={p.ms}
              timings={p.timings}
              exporter={p.exporter}
              includeName={p.includeName}
              onIncludeName={p.onIncludeName}
              onDownloadReport={p.onDownloadReport}
            />
          </Tabs.Content>
        </div>
      </Tabs.Root>

      <div className="flex items-center justify-between gap-3 border-t border-line-soft bg-panel-2/60 px-3 py-2">
        <ViewTools icons edges={p.edges} ortho={p.ortho} onEdges={p.onEdges} onFit={p.onFit} onView={p.onView} onOrtho={p.onOrtho} />
        {size && (
          <span className="num truncate text-[12px] text-fg-3" title="Size in CAD">
            {size}
          </span>
        )}
      </div>
    </aside>
  );
}
