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
    <main className="paper relative flex min-h-full flex-col items-center justify-center overflow-y-auto px-4 pt-24 pb-10">
      <motion.div
        initial={{ opacity: 0, y: 12 }}
        animate={{ opacity: 1, y: 0 }}
        transition={{ duration: 0.5, ease: [0.2, 0.7, 0.2, 1] }}
        className="flex w-full max-w-[820px] flex-col items-center gap-10"
      >
        <div className="flex flex-col items-center gap-4 text-center">
          <h1 className="font-display text-[clamp(52px,9vw,96px)] leading-none font-semibold tracking-[-0.045em]">Drop an OBJ.</h1>
          <p className="max-w-[560px] text-[clamp(16px,2vw,20px)] text-fg-2">
            Get a DXF with every coordinate intact. Nothing uploaded, nothing rounded.
          </p>
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
            <p className="text-center text-[14px] text-fg-3">or drag an .obj and its .mtl anywhere on this page</p>
          </div>
        </div>

        <section aria-labelledby="mapping" className="w-full">
          <h2 id="mapping" className="label mb-3 text-center">
            What gets converted
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
            DXF can't store texture coordinates, normals or smoothing groups. If your file has them, the report lists each one.
          </p>
        </section>
      </motion.div>
    </main>
  );
}
