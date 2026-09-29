import { Check, ChevronDown, CircleAlert, Download, TriangleAlert } from "lucide-react";
import { motion } from "motion/react";
import { Menu } from "@mantine/core";
import { Button } from "@/components/ui/button";
import type { Result } from "@/lib/engine";
import type { Explained } from "@/lib/errors";
import { bytes, fmt } from "@/lib/format";
import { FORMATS, formatInfo, type Format } from "@/lib/settings";
import { summarize } from "@/lib/summary";
import { cn } from "@/lib/utils";

export interface BatchItem {
  id: number;
  file: File;
  status: "waiting" | "converting" | "done" | "failed";
  result?: Result;
  error?: Explained;
}

function Status({ item }: { item: BatchItem }) {
  if (item.status === "waiting") return <span className="text-fg-3">Waiting</span>;
  if (item.status === "converting")
    return (
      <span className="flex items-center gap-2 text-fg-2">
        <span className="size-3 animate-spin rounded-full border-2 border-line border-t-accent" />
        Converting
      </span>
    );
  if (item.status === "failed")
    return (
      <span className="flex min-w-0 items-center gap-1.5 text-danger" title={item.error?.lines.join("\n")}>
        <CircleAlert className="size-4 shrink-0" />
        <span className="truncate">{item.error?.title ?? "Failed"}</span>
      </span>
    );
  const s = summarize(item.result!.report);
  return (
    <span className={cn("flex min-w-0 items-center gap-1.5", s.status === "exact" ? "text-exact" : "text-warn")}>
      {s.status === "exact" ? <Check className="size-4 shrink-0" /> : <TriangleAlert className="size-4 shrink-0" />}
      <span className="truncate">{s.title}</span>
    </span>
  );
}

/** Several files at once: each converted with its own detected settings. */
export function BatchScreen({
  items,
  format,
  onFormat,
  onOpen,
  onDownload,
  onDownloadAll,
  zipping,
}: {
  items: BatchItem[];
  format: Format;
  onFormat: (f: Format) => void;
  onOpen: (item: BatchItem) => void;
  onDownload: (item: BatchItem) => void;
  onDownloadAll: () => void;
  zipping: boolean;
}) {
  const done = items.filter((i) => i.status === "done");
  const running = items.some((i) => i.status === "waiting" || i.status === "converting");
  return (
    <main className="paper flex h-dvh justify-center overflow-hidden px-4 pt-[76px] pb-4">
      <motion.section initial={{ opacity: 0, y: 10 }} animate={{ opacity: 1, y: 0 }} className="panel flex h-fit max-h-full min-h-0 w-full max-w-[860px] flex-col" aria-label="Files">
        <div className="flex flex-wrap items-center gap-3 border-b border-line-soft px-5 py-4">
          <h1 className="m-0 text-[16px] font-semibold">
            {fmt(items.length)} files
            <span className="ml-2 text-[13px] font-normal text-fg-3">
              {fmt(done.length)} converted{items.some((i) => i.status === "failed") && `, ${fmt(items.filter((i) => i.status === "failed").length)} failed`}
            </span>
          </h1>
          <div className="flex-1" />
          <Menu position="bottom-end" offset={6} width={200} classNames={{ dropdown: "panel !p-1", item: "!rounded-[3px] !text-[13px]" }}>
            <Menu.Target>
              <Button size="sm" disabled={running}>
                {formatInfo(format).label}
                <ChevronDown />
              </Button>
            </Menu.Target>
            <Menu.Dropdown>
              {FORMATS.map((f) => (
                <Menu.Item key={f.value} onClick={() => onFormat(f.value)} rightSection={format === f.value ? <Check className="size-3.5" /> : null}>
                  {f.label}
                  {f.beta && <span className="ml-1.5 rounded-[2px] bg-warn-soft px-1 py-px text-[10.5px] font-medium text-warn">beta</span>}
                </Menu.Item>
              ))}
            </Menu.Dropdown>
          </Menu>
          <Button variant="primary" size="sm" onClick={onDownloadAll} disabled={running || done.length === 0 || zipping}>
            <Download />
            {zipping ? "Zipping…" : "Download all (.zip)"}
          </Button>
        </div>
        <ul className="m-0 min-h-0 flex-1 list-none overflow-y-auto p-2">
          {items.map((item) => (
            <li key={item.id} className="grid grid-cols-[minmax(0,1.4fr)_minmax(0,1.3fr)_auto] items-center gap-3 rounded-[3px] px-3 py-2 hover:bg-panel-2 sm:grid-cols-[minmax(0,1.4fr)_minmax(0,1.3fr)_90px_auto]">
              <button type="button" className="min-w-0 cursor-pointer truncate text-left text-[14px] font-medium hover:text-accent disabled:cursor-default disabled:hover:text-fg" onClick={() => onOpen(item)} disabled={item.status !== "done"}>
                {item.file.name}
              </button>
              <span className="min-w-0 text-[13px]">
                <Status item={item} />
              </span>
              <span className="num hidden text-right text-[12px] text-fg-3 sm:block">{item.result ? bytes(item.result.report.output.bytes) : bytes(item.file.size)}</span>
              <Button variant="ghost" size="icon-sm" onClick={() => onDownload(item)} disabled={item.status !== "done"} aria-label={`Download ${item.file.name}`}>
                <Download />
              </Button>
            </li>
          ))}
        </ul>
      </motion.section>
    </main>
  );
}
