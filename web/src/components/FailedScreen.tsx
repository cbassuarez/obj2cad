import { CircleAlert, ExternalLink } from "lucide-react";
import { motion } from "motion/react";
import { Button } from "@/components/ui/button";
import { CliHandoff } from "@/components/CliHandoff";
import { problemUrl, type Explained } from "@/lib/errors";
import type { Job } from "@/lib/files";
import type { Prefs } from "@/lib/settings";

export function FailedScreen({
  name,
  explained,
  job,
  prefs,
  onPick,
  onRetry,
}: {
  name: string;
  explained: Explained;
  /** The drawing that failed, for "Open in obj2cad" when the command line is the way on. */
  job?: Job;
  prefs: Prefs;
  onPick: () => void;
  onRetry?: () => void;
}) {
  const handoff = explained.cli && job;
  const e = explained;
  return (
    <main className="paper flex h-dvh items-center justify-center overflow-hidden px-4 pt-[76px] pb-4">
      <motion.div initial={{ opacity: 0, y: 10 }} animate={{ opacity: 1, y: 0 }} className="panel flex max-h-full w-full max-w-[620px] flex-col gap-5 overflow-y-auto p-6" role="alert">
        <div className="flex gap-3 rounded-[4px] bg-danger-soft p-4 text-danger">
          <CircleAlert className="mt-0.5 size-5 shrink-0" aria-hidden="true" />
          <div className="min-w-0">
            <div className="font-semibold">{e.title}</div>
            <div className="truncate text-[13.5px] text-fg-2">{name}</div>
          </div>
        </div>
        <p className="m-0 text-[14px]">{e.action}</p>
        {e.lines.length > 0 && (
          <pre className="num m-0 max-h-[240px] overflow-auto rounded-[4px] bg-panel-2 p-4 text-[12.5px] whitespace-pre-wrap text-fg">
            {e.lines.join("\n")}
            {e.more && "\n…"}
          </pre>
        )}
        {handoff ? (
          <CliHandoff job={job} prefs={prefs}>
            <Button variant="secondary" onClick={onPick}>
              Choose a file…
            </Button>
          </CliHandoff>
        ) : (
          <div className="flex flex-wrap items-center gap-3">
            {onRetry && (
              <Button variant="primary" onClick={onRetry}>
                Try again
              </Button>
            )}
            <Button variant={onRetry ? "secondary" : "primary"} onClick={onPick}>
              Choose a file…
            </Button>
            {e.report && (
              <Button variant="link" asChild>
                <a href={problemUrl(e.title, e.lines.join("\n"))} target="_blank" rel="noreferrer">
                  Report a problem <ExternalLink />
                </a>
              </Button>
            )}
          </div>
        )}
      </motion.div>
    </main>
  );
}
