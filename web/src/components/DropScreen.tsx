import { motion } from "motion/react";
import { CubeArt } from "@/components/brand";
import { Button } from "@/components/ui/button";

const MAPPING: [string, string][] = [
  ["Faces & n-gons", "MESH"],
  ["Lines (l)", "3D POLYLINE"],
  ["Points (p)", "POINT"],
  ["Objects & groups", "LAYER"],
  ["Materials (Kd)", "TRUE COLOR"],
];

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
        className="flex w-full max-w-[820px] flex-col items-center gap-10"
      >
        <div className="flex flex-col items-center gap-3 text-center">
          <h1 className="font-display text-[clamp(36px,6vw,56px)] leading-none font-semibold tracking-[-0.035em]">OBJ to DXF</h1>
          <p className="max-w-[560px] text-[16px] text-fg-2">Drop your .obj file below. Add its .mtl too if you want to keep material colors.</p>
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
          <div className="flex flex-col items-center gap-5 rounded-[4px] bg-accent-soft px-6 py-14 sm:py-16">
            <CubeArt className="size-16 text-accent" />
            <Button variant="primary" size="lg" onClick={onPick}>
              Choose files…
            </Button>
            <p className="text-center text-[14px] text-fg-3">or drag the files anywhere on this page. They stay on this computer.</p>
          </div>
        </div>

        <ol className="m-0 grid w-full list-none grid-cols-1 gap-2 p-0 sm:grid-cols-3" aria-label="Steps">
          {["Drop your .obj file", "Check the preview", "Download the DXF"].map((step, i) => (
            <li key={step} className="panel flex items-center gap-3 px-4 py-3 text-[14px]">
              <span className="num grid size-6 shrink-0 place-items-center rounded-[3px] bg-accent-soft text-[12px] font-medium text-accent">{i + 1}</span>
              {step}
            </li>
          ))}
        </ol>

        <section aria-labelledby="mapping" className="w-full">
          <h2 id="mapping" className="label mb-3 text-center">
            What ends up in the DXF
          </h2>
          <div className="panel grid grid-cols-2 overflow-hidden sm:grid-cols-5">
            {MAPPING.map(([from, to]) => (
              <div key={to} className="flex flex-col gap-1 border-line-soft px-4 py-3 [&:not(:last-child)]:border-r">
                <span className="text-[13px] text-fg-2">{from}</span>
                <span className="num text-[12.5px] font-medium text-accent">{to}</span>
              </div>
            ))}
          </div>
          <p className="mt-3 text-center text-[13px] text-fg-3">
            Textures, UV maps and smooth shading aren't part of DXF, so they're left out. The shapes are copied exactly.
          </p>
        </section>
      </motion.div>
    </main>
  );
}
