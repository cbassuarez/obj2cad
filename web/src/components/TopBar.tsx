import { ChevronDown, FileText, FolderOpen, Lock } from "lucide-react";
import { Menu } from "@mantine/core";
import { Tip } from "@/components/ui/tooltip";
import { Logo } from "@/components/brand";
import { bytes } from "@/lib/format";

declare const __APP_VERSION__: string;

export interface FileInfo {
  name: string;
  size: number;
  exporter: string | null;
  mtl: string | null;
}

export function TopBar({
  file,
  offlineReady,
  onOpen,
  onDownloadReport,
}: {
  file: FileInfo | null;
  offlineReady: boolean;
  onOpen: () => void;
  onDownloadReport?: () => void;
}) {
  const meta = file && [bytes(file.size), file.exporter, file.mtl ? "+ mtl" : null].filter(Boolean).join(" · ");
  return (
    <header className="pointer-events-none absolute inset-x-0 top-0 z-20 flex items-start gap-2 p-4">
      <div className="panel pointer-events-auto flex h-11 shrink-0 items-center gap-2.5 pr-4 pl-3">
        <Logo className="size-5 text-accent" />
        <span className="text-[15px] font-semibold tracking-tight">obj2cad</span>
        <span className="num hidden text-[11.5px] text-fg-3 sm:inline">{__APP_VERSION__}</span>
      </div>

      {file && (
        <Menu position="bottom-start" offset={6} width={240} classNames={{ dropdown: "panel !p-1.5", item: "!rounded-[3px] !text-[13.5px]" }}>
          <Menu.Target>
            <button type="button" className="panel pointer-events-auto flex h-11 min-w-0 cursor-pointer items-center gap-3 px-4 text-left hover:border-fg-3 sm:max-w-[46vw]">
              <span className="truncate text-[14px] font-medium">{file.name}</span>
              <span className="num hidden truncate text-[12px] text-fg-3 sm:inline">{meta}</span>
              <ChevronDown className="size-4 shrink-0 text-fg-3" />
            </button>
          </Menu.Target>
          <Menu.Dropdown>
            <Menu.Item leftSection={<FolderOpen className="size-4" />} onClick={onOpen}>
              Open another file…
            </Menu.Item>
            {onDownloadReport && (
              <Menu.Item leftSection={<FileText className="size-4" />} onClick={onDownloadReport}>
                Download report (.json)
              </Menu.Item>
            )}
          </Menu.Dropdown>
        </Menu>
      )}

      <div className="min-w-0 flex-1" />

      <Tip label="Conversion runs in this browser tab. Nothing is uploaded." side="bottom">
        <div className="panel pointer-events-auto hidden h-11 items-center gap-2 px-4 text-[13px] text-fg-2 md:flex">
          <Lock className="size-3.5 text-exact" aria-hidden="true" />
          Local only
          {offlineReady && (
            <>
              <span className="text-fg-3">·</span>
              <span className="size-1.5 rounded-full bg-exact" aria-hidden="true" />
              offline ready
            </>
          )}
        </div>
      </Tip>
    </header>
  );
}
