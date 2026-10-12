import { Button } from "@/components/ui/button";
import { CliHandoff } from "@/components/CliHandoff";
import type { Job } from "@/lib/files";
import { bytes } from "@/lib/format";
import type { Prefs } from "@/lib/settings";

/** A file large enough that obj2cad on the computer is the better place for it. */
export function PreflightScreen({ name, size, job, prefs, onContinue, onCancel }: { name: string; size: number; job: Job; prefs: Prefs; onContinue: () => void; onCancel: () => void }) {
  return (
    <main className="paper flex h-dvh items-center justify-center overflow-hidden px-4" role="alertdialog" aria-labelledby="preflight-title">
      <div className="panel flex w-full max-w-[560px] flex-col gap-4 p-6">
        <div id="preflight-title" className="text-[16px] font-semibold">
          {name} is {bytes(size)}
        </div>
        <p className="m-0 text-[14px]">It converts in the browser too, but takes several minutes and much of this computer's memory. obj2cad on your computer is faster, with the same settings.</p>
        <CliHandoff job={job} prefs={prefs}>
          <Button onClick={onContinue}>Convert in the browser</Button>
          <Button variant="ghost" onClick={onCancel}>
            Cancel
          </Button>
        </CliHandoff>
      </div>
    </main>
  );
}
