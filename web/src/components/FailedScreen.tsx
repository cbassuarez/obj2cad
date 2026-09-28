import { CircleAlert } from "lucide-react";
import { motion } from "motion/react";
import { Button } from "@/components/ui/button";

export function FailedScreen({ name, message, onPick }: { name: string; message: string; onPick: () => void }) {
  return (
    <main className="paper flex min-h-full items-center justify-center px-4 pt-20 pb-10">
      <motion.div initial={{ opacity: 0, y: 10 }} animate={{ opacity: 1, y: 0 }} className="panel flex w-full max-w-[600px] flex-col gap-5 p-6" role="alert">
        <div className="flex gap-3 rounded-[4px] bg-danger-soft p-4 text-danger">
          <CircleAlert className="mt-0.5 size-5 shrink-0" aria-hidden="true" />
          <div>
            <div className="font-semibold">This file can't be converted exactly</div>
            <div className="text-[13.5px] text-fg-2">obj2cad stops instead of guessing when a file is ambiguous. Nothing was written.</div>
          </div>
        </div>
        <pre className="num m-0 rounded-[4px] bg-panel-2 p-4 text-[12.5px] whitespace-pre-wrap text-fg">
          {name}
          {"\n"}
          {message}
        </pre>
        <div>
          <Button variant="primary" onClick={onPick}>
            Choose another file…
          </Button>
        </div>
      </motion.div>
    </main>
  );
}
