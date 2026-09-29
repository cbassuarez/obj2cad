import { Layers, Files } from "lucide-react";
import { motion } from "motion/react";
import { Button } from "@/components/ui/button";
import { baseName, type Job } from "@/lib/files";

/** Several loose models: one drawing with all of them, or one drawing each. */
export function ChoiceScreen({ separate, onCombine, onSeparate }: { separate: Job[]; onCombine: () => void; onSeparate: () => void }) {
  const names = separate.map((j) => baseName(j.sources[0].path));
  return (
    <main className="paper flex min-h-dvh items-center justify-center px-4 pt-20 pb-10">
      <motion.div initial={{ opacity: 0, y: 10 }} animate={{ opacity: 1, y: 0 }} className="panel flex w-full max-w-[520px] flex-col gap-5 p-6" role="dialog" aria-labelledby="choice-title">
        <div>
          <h1 id="choice-title" className="m-0 text-[17px] font-semibold">
            {names.length} models
          </h1>
          <p className="m-0 mt-1 truncate text-[13.5px] text-fg-3" title={names.join(", ")}>
            {names.join(", ")}
          </p>
        </div>
        <div className="flex flex-wrap gap-3">
          <Button variant="primary" onClick={onCombine}>
            <Layers />
            Combine into one drawing
          </Button>
          <Button onClick={onSeparate}>
            <Files />
            Convert separately
          </Button>
        </div>
      </motion.div>
    </main>
  );
}
