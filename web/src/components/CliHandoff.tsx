import { useState, type ReactNode } from "react";
import { Button } from "@/components/ui/button";
import { CLI_URL } from "@/lib/errors";
import type { Job } from "@/lib/files";
import { downloadsFor, handoffItem, handoffLink, openLink } from "@/lib/handoff";
import type { Prefs } from "@/lib/settings";

const LINK = "text-accent underline-offset-2 hover:underline";

/** "Open in obj2cad": the command-line version on this computer takes the file over,
 *  with the same settings. `children`: the other actions, beside it. */
export function CliHandoff({ job, prefs, children }: { job: Job; prefs: Prefs; children?: ReactNode }) {
  const [opened, setOpened] = useState(false);
  const name = handoffItem(job).name;
  const downloads = downloadsFor(navigator.userAgent);
  return (
    <div className="flex flex-col gap-4">
      <div className="flex flex-wrap items-center gap-3">
        <Button
          variant="primary"
          onClick={() => {
            openLink(handoffLink(job, prefs));
            setOpened(true);
          }}
        >
          Open in obj2cad
        </Button>
        {children}
      </div>
      {opened && (
        <p className="m-0 text-[13.5px]" role="status">
          Your browser may ask to open obj2cad. In the window that opens, it looks for {name} in your Downloads, Desktop and Documents, or asks you to drag it in.
        </p>
      )}
      <p className="m-0 text-[13px] text-fg-2">
        First time?{" "}
        {downloads.length > 0 ? (
          <>
            Download obj2cad for{" "}
            {downloads.map((d, i) => (
              <span key={d.url}>
                {i > 0 && " or "}
                <a className={LINK} href={d.url}>
                  {d.label}
                </a>
              </span>
            ))}
            , open it once to set it up, then click Open in obj2cad again.{" "}
            <a className="underline underline-offset-2" href={CLI_URL} target="_blank" rel="noreferrer">
              Other systems
            </a>
          </>
        ) : (
          <>
            Download obj2cad from the{" "}
            <a className={LINK} href={CLI_URL} target="_blank" rel="noreferrer">
              releases page
            </a>
            , open it once to set it up, then click Open in obj2cad again.
          </>
        )}
      </p>
    </div>
  );
}
