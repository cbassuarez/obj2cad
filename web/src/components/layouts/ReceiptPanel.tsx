// Layout C, "Receipt": a short card that reads as one sentence about the drawing. Each
// decided value (up direction, units, format) is underlined in place and changes where
// it's read. The download follows the sentence. The audit material (report, hashes,
// timings) moves behind the "⋯" menu into a dialog, so the card holds one button only.
import { useState } from "react";
import { Menu, Modal } from "@mantine/core";
import { Check, ChevronDown, Copy, ExternalLink, FileText, Info, MoreHorizontal } from "lucide-react";
import { Seal, Notes, TechnicalDetails } from "@/components/Inspector";
import { Button } from "@/components/ui/button";
import { DownloadAfter, DownloadButton, Provenance, UnitsMenu, menuStyles, sizeText, unitsChanged, unitsLabel, unitsTag, upChanged, upLabel, upTag, type ResultProps } from "@/components/controls";
import { problemUrl } from "@/lib/errors";
import { bytes, fmt } from "@/lib/format";
import { FORMATS, UPS, type UpAxis } from "@/lib/settings";
import { summarize } from "@/lib/summary";

const token =
  "inline-flex cursor-pointer items-center gap-0.5 rounded-[3px] font-semibold text-fg underline decoration-line decoration-dotted underline-offset-[3px] hover:bg-panel-2 hover:decoration-fg";

function Line({ label, children, aside }: { label: string; children: React.ReactNode; aside?: React.ReactNode }) {
  return (
    <div className="grid grid-cols-[92px_minmax(0,1fr)_auto] items-baseline gap-x-3 py-1.5 text-[13.5px]">
      <dt className="text-fg-3">{label}</dt>
      <dd className="m-0 min-w-0 truncate">{children}</dd>
      <span className="text-right">{aside}</span>
    </div>
  );
}

export function ReceiptPanel(p: ResultProps) {
  const { report, decisions } = p;
  const s = summarize(report);
  const size = sizeText(report, decisions);
  const [details, setDetails] = useState(false);
  const copyHash = () => void navigator.clipboard?.writeText(report.parity_hash).catch(() => {});

  return (
    <aside className="panel pointer-events-auto flex max-h-full min-h-0 w-full flex-col overflow-hidden" aria-label="Result">
      <div className="flex items-center justify-between gap-2 px-5 pt-4 pb-3">
        <Seal compact summary={s} id={`${report.parity_hash}${s.status}`} />
        <Menu position="bottom-end" offset={6} width={240} classNames={menuStyles}>
          <Menu.Target>
            <Button variant="ghost" size="icon-sm" aria-label="More">
              <MoreHorizontal />
            </Button>
          </Menu.Target>
          <Menu.Dropdown>
            <Menu.Item leftSection={<Info className="size-4" />} onClick={() => setDetails(true)}>
              Technical details…
            </Menu.Item>
            <Menu.Item leftSection={<Copy className="size-4" />} onClick={copyHash}>
              Copy parity hash
            </Menu.Item>
            <Menu.Item leftSection={<FileText className="size-4" />} onClick={p.onDownloadReport}>
              Save conversion report (.json)
            </Menu.Item>
            <Menu.Divider />
            <Menu.Item
              leftSection={<ExternalLink className="size-4" />}
              component="a"
              href={problemUrl("Problem with a conversion", `engine ${report.engine_version}, ${report.output.format}, parity ${report.parity_hash.slice(0, 12)}`)}
              target="_blank"
              rel="noreferrer"
            >
              Report a problem
            </Menu.Item>
          </Menu.Dropdown>
        </Menu>
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto px-5">
        <dl className="m-0 border-y border-line-soft py-1.5">
          <Line label="Contents">
            <span className="num">{fmt(s.shapes.count)}</span> {s.shapes.count === 1 ? s.shapes.noun.slice(0, -1) : s.shapes.noun} on <span className="num">{fmt(p.layerCount)}</span>{" "}
            {p.layerCount === 1 ? "layer" : "layers"}
          </Line>
          <Line label="Up" aside={<Provenance tag={upTag(decisions)} onReset={upChanged(decisions) ? () => p.onUp(null) : undefined} />}>
            <Menu position="bottom-start" offset={6} width={200} classNames={menuStyles}>
              <Menu.Target>
                <button type="button" className={token}>
                  {upLabel(decisions)}
                  <ChevronDown className="size-3.5" />
                </button>
              </Menu.Target>
              <Menu.Dropdown>
                <Menu.Label>Up direction</Menu.Label>
                {UPS.map((u) => (
                  <Menu.Item key={u.value} onClick={() => p.onUp(u.value as UpAxis)} rightSection={decisions.up_axis === u.value ? <Check className="size-3.5" /> : null}>
                    {u.label}
                  </Menu.Item>
                ))}
              </Menu.Dropdown>
            </Menu>
          </Line>
          <Line label="Units" aside={<Provenance tag={unitsTag(decisions)} onReset={unitsChanged(decisions) ? () => p.onUnits(null) : undefined} />}>
            <UnitsMenu decisions={decisions} unitsStated={p.unitsStated} houseUnits={p.houseUnits} onUnits={p.onUnits} onHouseUnits={p.onHouseUnits}>
              <button type="button" className={token}>
                {unitsLabel(decisions)}
                <ChevronDown className="size-3.5" />
              </button>
            </UnitsMenu>
          </Line>
          {size && (
            <Line label="Size">
              <span className="num text-[13px]">{size}</span>
            </Line>
          )}
          <Line label="Format" aside={<span className="num text-[12px] text-fg-3">{bytes(report.output.bytes)}</span>}>
            <Menu position="bottom-start" offset={6} width={200} classNames={menuStyles}>
              <Menu.Target>
                <button type="button" className={token} disabled={p.busy !== null}>
                  {FORMATS.find((f) => f.value === p.format)!.label}
                  <ChevronDown className="size-3.5" />
                </button>
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
          </Line>
        </dl>

        <Notes summary={s} onKeepLoose={p.onKeepLoose} onAddMtl={p.onAddMtl} className="pt-3" />
      </div>

      <div className="flex flex-col gap-2 px-5 pt-4 pb-4">
        <DownloadButton {...p} />
        <DownloadAfter {...p} />
      </div>

      <Modal
        opened={details}
        onClose={() => setDetails(false)}
        title="Technical details"
        centered
        size="lg"
        classNames={{ content: "panel !bg-panel-solid", header: "!bg-panel-solid", title: "label" }}
      >
        <TechnicalDetails plain report={report} ms={p.ms} timings={p.timings} exporter={p.exporter} includeName={p.includeName} onIncludeName={p.onIncludeName} onDownloadReport={p.onDownloadReport} />
      </Modal>
    </aside>
  );
}
