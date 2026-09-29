import { ArrowLeft, Check, ChevronDown, FolderOpen, FolderTree, Lock, RefreshCw } from "lucide-react";
import { Menu } from "@mantine/core";
import { Button } from "@/components/ui/button";
import { Tip } from "@/components/ui/tooltip";
import { Logo } from "@/components/brand";
import { bytes } from "@/lib/format";
import { shortcut } from "@/lib/utils";

declare const __APP_VERSION__: string;

export interface FileInfo {
  name: string;
  size: number;
  exporter: string | null;
  /** Files the drawing was made from. */
  files: number;
}

export function TopBar({
  file,
  offlineReady,
  onOpen,
  onOpenFolder,
  onBack,
  backLabel,
  watch,
  onWatch,
}: {
  file: FileInfo | null;
  offlineReady: boolean;
  onOpen: () => void;
  /** Choose a folder (one drawing of everything in it). */
  onOpenFolder: () => void;
  /** Back to the file list (batch). */
  onBack?: () => void;
  backLabel?: string;
  /** Reload when the file changes on disk: `null` when not possible for this file. */
  watch: boolean | null;
  onWatch: (on: boolean) => void;
}) {
  const meta = file && [bytes(file.size), file.exporter, file.files > 1 ? `${file.files} files` : null].filter(Boolean).join(" · ");
  return (
    <header className="pointer-events-none absolute inset-x-0 top-0 z-20 flex items-start gap-2 p-4">
      <div className="panel pointer-events-auto flex h-11 shrink-0 items-center gap-2.5 pr-4 pl-3">
        <Logo className="size-5 text-accent" />
        <span className="text-[15px] font-semibold tracking-tight">obj2cad</span>
        <span className="num hidden text-[11.5px] text-fg-3 sm:inline">{__APP_VERSION__}</span>
      </div>

      {onBack && (
        <Button className="panel pointer-events-auto h-11 !rounded-[4px]" variant="ghost" onClick={onBack}>
          <ArrowLeft />
          <span className="hidden sm:inline">{backLabel}</span>
        </Button>
      )}

      {file && (
        <Menu position="bottom-start" offset={6} width={260} classNames={{ dropdown: "panel !p-1.5", item: "!rounded-[3px] !text-[13.5px]" }}>
          <Menu.Target>
            <button type="button" className="panel pointer-events-auto flex h-11 min-w-0 cursor-pointer items-center gap-3 px-4 text-left hover:border-fg-3 sm:max-w-[40vw]">
              <span className="truncate text-[14px] font-medium">{file.name}</span>
              <span className="num hidden truncate text-[12px] text-fg-3 sm:inline">{meta}</span>
              <ChevronDown className="size-4 shrink-0 text-fg-3" />
            </button>
          </Menu.Target>
          <Menu.Dropdown>
            <Menu.Item leftSection={<FolderOpen className="size-4" />} onClick={onOpen}>
              Open another file…
            </Menu.Item>
            <Menu.Item leftSection={<FolderTree className="size-4" />} onClick={onOpenFolder}>
              Open a folder…
            </Menu.Item>
            {watch !== null && (
              <Menu.Item leftSection={<RefreshCw className="size-4" />} rightSection={watch ? <Check className="size-3.5" /> : null} onClick={() => onWatch(!watch)}>
                Reload when the file changes
              </Menu.Item>
            )}
          </Menu.Dropdown>
        </Menu>
      )}

      <div className="min-w-0 flex-1" />

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
      <Tip label={`Open files (${shortcut("O")})`} side="bottom">
        <Button variant="secondary" className="pointer-events-auto h-11 shadow-panel" onClick={onOpen} aria-label="Open files">
          <FolderOpen />
          <span className="hidden sm:inline">Open</span>
        </Button>
      </Tip>
    </header>
  );
}
