import { motion } from "motion/react";
import { CubeArt } from "@/components/brand";
import { Button } from "@/components/ui/button";

/** Corner crop marks, drawn like a drafting sheet's registration marks. */
function CropMarks() {
  const base = "absolute size-4 border-fg";
  return (
    <>
      <span className={`${base} -top-2 -left-2 border-t-2 border-l-2`} />
      <span className={`${base} -top-2 -right-2 border-t-2 border-r-2`} />
      <span className={`${base} -bottom-2 -left-2 border-b-2 border-l-2`} />
      <span className={`${base} -right-2 -bottom-2 border-r-2 border-b-2`} />
    </>
  );
}

export function DropScreen({ onPick }: { onPick: () => void }) {
  return (
    <main className="paper relative flex min-h-dvh flex-col items-center justify-center px-4 pt-24 pb-12">
      <motion.div
        initial={{ opacity: 0, y: 12 }}
        animate={{ opacity: 1, y: 0 }}
        transition={{ duration: 0.5, ease: [0.2, 0.7, 0.2, 1] }}
        className="flex w-full max-w-[720px] flex-col items-center gap-10"
      >
        <div className="flex flex-col items-center gap-3 text-center">
          <h1 className="font-display text-[clamp(36px,6vw,56px)] leading-none font-semibold tracking-[-0.035em]">OBJ to DWG / DXF</h1>
          <p className="max-w-[560px] text-[16px] text-fg-2">Drop an .obj, or a .zip with its materials, textures and point clouds.</p>
        </div>

        <div className="relative w-full">
          <CropMarks />
          <svg className="pointer-events-none absolute inset-0 size-full" aria-hidden="true">
            <motion.rect
              x="1"
              y="1"
              rx="4"
              style={{ width: "calc(100% - 2px)", height: "calc(100% - 2px)" }}
              fill="none"
              stroke="var(--accent)"
              strokeOpacity={0.6}
              strokeWidth={1.5}
              strokeDasharray="10 10"
              animate={{ strokeDashoffset: [0, -40] }}
              transition={{ duration: 1.6, ease: "linear", repeat: Infinity }}
            />
          </svg>
          <div className="flex flex-col items-center gap-5 rounded-[4px] bg-accent-soft px-6 py-16 sm:py-20">
            <CubeArt className="size-16 text-accent" />
            <Button variant="primary" size="lg" onClick={onPick}>
              Choose files…
            </Button>
            <p className="text-center text-[14px] text-fg-3">or drop them anywhere on this page</p>
          </div>
        </div>
      </motion.div>
    </main>
  );
}
