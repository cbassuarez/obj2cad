import { Button } from "@/components/ui/button";
import { bytes } from "@/lib/format";

/** A file too large to be comfortable in a browser tab. */
export function PreflightScreen({ name, size, onContinue, onCancel, cliUrl }: { name: string; size: number; onContinue: () => void; onCancel: () => void; cliUrl: string }) {
  return (
    <main className="paper flex h-dvh items-center justify-center overflow-hidden px-4" role="alertdialog" aria-labelledby="preflight-title">
      <div className="panel flex w-full max-w-[520px] flex-col gap-4 p-6">
        <div id="preflight-title" className="text-[16px] font-semibold">
          {name} is {bytes(size)}
        </div>
        <div className="flex flex-wrap gap-3">
          <Button variant="primary" asChild>
            <a href={cliUrl} target="_blank" rel="noreferrer">
              Command-line version
            </a>
          </Button>
          <Button onClick={onContinue}>Convert here anyway</Button>
          <Button variant="ghost" onClick={onCancel}>
            Cancel
          </Button>
        </div>
      </div>
    </main>
  );
}
