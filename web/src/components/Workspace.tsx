import { useEffect, useRef, useState } from "react";
import { AnimatePresence, motion } from "motion/react";
import { Viewer, type AxisDirs } from "@/viewer/Viewer";
import { AxisGizmo } from "@/components/AxisGizmo";
import { LayersCard } from "@/components/LayersCard";
import { ResultCard } from "@/components/ResultCard";
import { ViewTools, type ResultProps } from "@/components/controls";
import type { Inspection, PreviewBuffers, Result } from "@/lib/engine";
import type { Format, LayerMode, Prefs, UpAxis, Units } from "@/lib/settings";
import { unitSymbol } from "@/lib/settings";

function cssVar(name: string) {
  return getComputedStyle(document.documentElement).getPropertyValue(name).trim();
}

/** Preview buffers and the orientation they were built in. `id` changes per build;
 *  `fileId` per opened file (a new file re-frames the view). */
export interface PreviewState {
  buffers: PreviewBuffers;
  builtUp: UpAxis;
  id: number;
  fileId: number;
}

export function Workspace({
  result,
  preview,
  inspection,
  prefs,
  busy,
  downloaded,
  onUp,
  onUnits,
  onHouseUnits,
  onKeepLoose,
  onLayerMode,
  onFormat,
  onIncludeName,
  onDownload,
  onDownloadVisible,
  onDownloadReport,
  onAddMtl,
  onAnother,
}: {
  result: Result;
  preview: PreviewState | null;
  inspection: Inspection | null;
  prefs: Prefs;
  busy: string | null;
  downloaded: string | null;
  onUp: (u: UpAxis | null) => void;
  onUnits: (u: Units | null) => void;
  onHouseUnits: (u: Units | null) => void;
  onKeepLoose: (keep: boolean) => void;
  onLayerMode: (m: LayerMode) => void;
  onFormat: (f: Format) => void;
  onIncludeName: (on: boolean) => void;
  onDownload: (pick: boolean) => void;
  onDownloadVisible: (hiddenLayers: string[]) => void;
  onDownloadReport: () => void;
  onAddMtl: () => void;
  onAnother: () => void;
}) {
  const host = useRef<HTMLDivElement>(null);
  const viewer = useRef<Viewer | null>(null);
  const shownFile = useRef<number | null>(null);
  const [axes, setAxes] = useState<AxisDirs | null>(null);
  const [edges, setEdges] = useState(false);
  const [ortho, setOrtho] = useState(false);
  const [hidden, setHidden] = useState<Set<number>>(new Set());
  const { report, decisions } = result;

  useEffect(() => {
    const v = new Viewer(host.current!);
    v.onAxes = setAxes;
    v.setTheme({ grid: cssVar("--grid"), gridMajor: cssVar("--grid-major"), dim: cssVar("--dim"), edge: cssVar("--text") });
    viewer.current = v;
    return () => {
      v.dispose();
      viewer.current = null;
    };
  }, []);

  // New buffers: rebuild (and re-frame only for a new file).
  useEffect(() => {
    const v = viewer.current;
    if (!v || !preview) return;
    v.show(preview.buffers, preview.builtUp, shownFile.current !== preview.fileId);
    shownFile.current = preview.fileId;
    v.setEdges(edges);
    setHidden(new Set());
    // `edges` is applied on its own below; new buffers must not re-run on toggles.
  }, [preview]);

  // A new orientation turns the existing preview.
  useEffect(() => viewer.current?.setOrientation(decisions.up_axis), [decisions.up_axis, preview]);
  useEffect(() => viewer.current?.setUnit(unitSymbol(decisions.units)), [decisions.units]);
  useEffect(() => viewer.current?.setEdges(edges), [edges]);

  const toggleLayer = (layer: number) => {
    const next = new Set(hidden);
    if (next.has(layer)) next.delete(layer);
    else next.add(layer);
    setHidden(next);
    viewer.current?.setLayerVisible(layer, !next.has(layer));
  };

  const available = preview?.buffers.available ?? true;
  const layerCount = report.layers.filter((l) => l.faces + l.polylines + l.points > 0).length;
  const downloadVisible = () => onDownloadVisible(report.layers.filter((_, i) => hidden.has(i)).map((l) => l.name));
  const onOrtho = (on: boolean) => {
    setOrtho(on);
    viewer.current?.setOrtho(on);
  };
  const panel: ResultProps = {
    report,
    decisions,
    format: prefs.format,
    ms: result.ms,
    timings: result.timings,
    exporter: inspection?.hints.exporter ?? null,
    busy,
    downloaded,
    unitsStated: inspection?.hints.units_source === "exporter",
    houseUnits: prefs.houseUnits,
    includeName: prefs.includeName,
    edges,
    ortho,
    hiddenCount: hidden.size,
    layerCount,
    onUp,
    onUnits,
    onHouseUnits,
    onKeepLoose,
    onFormat,
    onIncludeName,
    onEdges: setEdges,
    onView: (v) => viewer.current?.setView(v),
    onOrtho,
    onFit: () => viewer.current?.fit(),
    onDownload,
    onDownloadVisible: downloadVisible,
    onDownloadReport,
    onAddMtl,
    onAnother,
  };

  return (
    <main className="relative flex h-dvh flex-col overflow-hidden bg-viewport lg:block">
      <div className="relative min-h-[200px] flex-1 lg:absolute lg:inset-0">
        <div ref={host} className="absolute inset-0" />
        {!available && (
          <div className="absolute inset-0 grid place-items-center p-6">
            <div className="panel px-5 py-4 text-[13.5px] font-semibold">No preview for this model</div>
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
              {busy}
            </motion.div>
          )}
        </AnimatePresence>

        <div className="pointer-events-none absolute bottom-4 left-4 hidden sm:block">
          <AxisGizmo axes={axes} unit={unitSymbol(decisions.units)} />
        </div>

        <div className="absolute right-4 bottom-4 lg:top-[76px] lg:right-[396px] lg:bottom-auto">
          <ViewTools edges={edges} ortho={ortho} onEdges={setEdges} onFit={panel.onFit} onView={panel.onView} onOrtho={onOrtho} />
        </div>
      </div>

      {/* Panels float over the viewer on large screens; on small ones they share a bottom
          sheet that scrolls by itself, so the page never does. */}
      <div className="flex max-h-[58%] shrink-0 flex-col gap-3 overflow-y-auto border-t border-line bg-bg p-3 lg:contents">
        <motion.div
          initial={{ opacity: 0, x: 16 }}
          animate={{ opacity: 1, x: 0 }}
          transition={{ delay: 0.1 }}
          className="pointer-events-none lg:absolute lg:top-[76px] lg:right-4 lg:flex lg:max-h-[calc(100%-92px)] lg:w-[364px]"
        >
          <ResultCard {...panel} />
        </motion.div>
        <motion.div
          initial={{ opacity: 0, x: -16 }}
          animate={{ opacity: 1, x: 0 }}
          transition={{ delay: 0.05 }}
          className="pointer-events-none lg:absolute lg:top-[76px] lg:left-4 lg:flex lg:max-h-[calc(100%-200px)] lg:w-[268px]"
        >
          <LayersCard report={report} mode={prefs.layerMode} hidden={hidden} busy={busy !== null} onMode={onLayerMode} onToggle={toggleLayer} />
        </motion.div>
      </div>
    </main>
  );
}
