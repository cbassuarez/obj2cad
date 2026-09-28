import { useEffect, useRef, useState } from "react";
import { AnimatePresence, motion } from "motion/react";
import { Viewer, type AxisDirs } from "@/viewer/Viewer";
import { AxisGizmo } from "@/components/AxisGizmo";
import { Dock, type AutoChoice } from "@/components/Dock";
import { Inspector } from "@/components/Inspector";
import { LayersCard } from "@/components/LayersCard";
import type { ConvertResult, Report } from "@/lib/engine";
import type { Settings, Units, Up } from "@/lib/settings";

function cssVar(name: string) {
  return getComputedStyle(document.documentElement).getPropertyValue(name).trim();
}

export function Workspace({
  result,
  report,
  settings,
  busy,
  exporter,
  units,
  orientation,
  onAddMtl,
  onUnits,
  onUp,
  onExport,
  onDownloadReport,
}: {
  result: ConvertResult;
  report: Report;
  settings: Settings;
  busy: boolean;
  exporter: string | null;
  units: AutoChoice<Units> | null;
  orientation: AutoChoice<Up> | null;
  onAddMtl: () => void;
  onUnits: (u: Units) => void;
  onUp: (u: Up) => void;
  onExport: () => void;
  onDownloadReport: () => void;
}) {
  const host = useRef<HTMLDivElement>(null);
  const viewer = useRef<Viewer | null>(null);
  const [axes, setAxes] = useState<AxisDirs | null>(null);
  const [edges, setEdges] = useState(false);
  const [hidden, setHidden] = useState<Set<number>>(new Set());

  useEffect(() => {
    const v = new Viewer(host.current!);
    v.onAxes = setAxes;
    viewer.current = v;
    return () => {
      v.dispose();
      viewer.current = null;
    };
  }, []);

  useEffect(() => {
    viewer.current?.setTheme({
      grid: cssVar("--grid"),
      gridMajor: cssVar("--grid-major"),
      dim: cssVar("--dim"),
      edge: cssVar("--text"),
      line: cssVar("--accent"),
    });
  }, []);

  useEffect(() => {
    viewer.current?.setUnit(settings.units === "unitless" ? "" : settings.units);
  }, [settings.units]);

  useEffect(() => {
    viewer.current?.show(result);
    viewer.current?.setEdges(edges);
    setHidden(new Set());
    // `edges` is applied on its own below; a new result should not re-run on toggles.
  }, [result]);

  useEffect(() => viewer.current?.setEdges(edges), [edges]);

  const toggleLayer = (layer: number) => {
    const next = new Set(hidden);
    if (next.has(layer)) next.delete(layer);
    else next.add(layer);
    setHidden(next);
    viewer.current?.setLayerVisible(layer, !next.has(layer));
  };

  return (
    <main className="bg-viewport pb-2 lg:relative lg:h-dvh lg:min-h-[700px] lg:overflow-hidden lg:pb-0">
      {/* viewport */}
      <div className="relative h-[60vh] min-h-[360px] lg:absolute lg:inset-0 lg:h-auto">
        <div ref={host} className="absolute inset-0" />
        {!result.previewAvailable && (
          <div className="absolute inset-0 grid place-items-center p-6">
            <div className="panel max-w-sm p-5 text-center text-[13.5px] text-fg-2">
              <div className="mb-1 font-semibold text-fg">No preview for this model</div>
              Its coordinates are too large to draw on screen. The DXF is unaffected and still exact.
            </div>
          </div>
        )}

        <AnimatePresence>
          {busy && (
            <motion.div
              initial={{ opacity: 0, y: -6 }}
              animate={{ opacity: 1, y: 0 }}
              exit={{ opacity: 0 }}
              className="panel absolute top-[72px] left-1/2 z-10 flex -translate-x-1/2 items-center gap-2.5 !rounded-[4px] px-4 py-2 text-[13px] text-fg-2 lg:top-5"
              role="status"
            >
              <span className="size-3.5 animate-spin rounded-full border-2 border-line border-t-accent" />
              Converting…
            </motion.div>
          )}
        </AnimatePresence>

        <div className="pointer-events-none absolute bottom-4 left-4 hidden sm:block">
          <AxisGizmo axes={axes} unit={settings.units} />
        </div>

      </div>

      <div className="flex justify-center px-4 pt-4 lg:pointer-events-none lg:absolute lg:right-[396px] lg:bottom-4 lg:left-[300px] lg:p-0">
          <Dock
            settings={settings}
            onUnits={onUnits}
            onUp={onUp}
            edges={edges}
            onEdges={setEdges}
            onFit={() => viewer.current?.fit()}
            onExport={onExport}
            exportBytes={busy ? null : result.dxf.byteLength}
            busy={busy}
            units={units}
            orientation={orientation}
          />
        </div>
      {/* panels: floating on large screens, stacked below the viewer on small ones */}
      <motion.div
        initial={{ opacity: 0, x: -16 }}
        animate={{ opacity: 1, x: 0 }}
        transition={{ delay: 0.05 }}
        className="pointer-events-none p-4 lg:absolute lg:top-[76px] lg:left-4 lg:max-h-[calc(100%-200px)] lg:w-[268px] lg:p-0"
      >
        <LayersCard report={report} hidden={hidden} onToggle={toggleLayer} />
      </motion.div>
      <motion.div
        initial={{ opacity: 0, x: 16 }}
        animate={{ opacity: 1, x: 0 }}
        transition={{ delay: 0.1 }}
        className="pointer-events-none px-4 pb-6 lg:absolute lg:top-[76px] lg:right-4 lg:bottom-4 lg:flex lg:w-[364px] lg:p-0"
      >
        <Inspector report={report} ms={result.ms} timings={result.timings} exporter={exporter} onAddMtl={onAddMtl} onDownloadReport={onDownloadReport} />
      </motion.div>
    </main>
  );
}
