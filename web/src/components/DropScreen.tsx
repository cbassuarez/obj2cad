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

export function DropScreen({ onPick, onPickFolder }: { onPick: () => void; onPickFolder: () => void }) {
  return (
    <main className="paper relative flex h-dvh flex-col items-center justify-center overflow-hidden px-4 pt-[76px] pb-[clamp(16px,5vh,48px)]">
      <motion.div
        initial={{ opacity: 0, y: 12 }}
        animate={{ opacity: 1, y: 0 }}
        transition={{ duration: 0.5, ease: [0.2, 0.7, 0.2, 1] }}
        className="flex max-h-[580px] min-h-0 w-full max-w-[720px] flex-1 flex-col items-center justify-center gap-[clamp(16px,4.5vh,40px)]"
      >
        <div className="flex shrink-0 flex-col items-center gap-[clamp(6px,1.5vh,12px)] text-center">
          <h1 className="m-0 font-display text-[clamp(28px,min(6vw,7vh),56px)] leading-none font-semibold tracking-[-0.035em]">OBJ to DWG / DXF</h1>
          <p className="m-0 max-w-[560px] text-[clamp(14px,2.2vh,16px)] text-fg-2">Drop an .obj, or a folder or .zip with its materials, textures and point clouds.</p>
        </div>

        <div className="relative flex min-h-[150px] w-full flex-1 [@media(max-height:420px)]:min-h-[120px]">
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
          <div className="flex w-full flex-col items-center justify-center gap-[clamp(10px,2.5vh,20px)] rounded-[4px] bg-accent-soft px-6 py-4">
            <CubeArt className="size-[clamp(40px,8vh,64px)] shrink-0 text-accent [@media(max-height:480px)]:hidden" />
            <Button variant="primary" size="lg" onClick={onPick}>
              Choose files…
            </Button>
            <p className="m-0 text-center text-[14px] text-fg-3">
              or{" "}
              <button type="button" className="cursor-pointer font-semibold text-accent hover:underline" onClick={onPickFolder}>
                choose a folder
              </button>
              <span className="[@media(pointer:coarse)]:hidden">, or drop them anywhere on this page</span>
            </p>
          </div>
        </div>
      </motion.div>
    </main>
  );
}
