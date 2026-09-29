import { jsx as _jsx, jsxs as _jsxs } from "react/jsx-runtime";
import { useEffect, useMemo, useRef, useState } from "react";
import { AnimatePresence, motion } from "motion/react";
import { Viewer } from "@/viewer/Viewer";
import { AxisGizmo } from "@/components/AxisGizmo";
import { LayersCard } from "@/components/LayersCard";
import { ResultCard } from "@/components/ResultCard";
import { ViewTools } from "@/components/controls";
import { displayUnit, unitSymbol } from "@/lib/settings";
function cssVar(name) {
    return getComputedStyle(document.documentElement).getPropertyValue(name).trim();
}
export function Workspace({ result, visible, hidden: hiddenNames, preview, inspection, prefs, busy, downloaded, onUp, onUnits, onHouseUnits, onShowIn, onKeepLoose, onLayerMode, onFormat, onIncludeName, onCurves, onDownload, onHidden, onDownloadReport, onAddMtl, onAnother, }) {
    const host = useRef(null);
    const viewer = useRef(null);
    const shownFile = useRef(null);
    const [axes, setAxes] = useState(null);
    const [edges, setEdges] = useState(false);
    const [ortho, setOrtho] = useState(false);
    const { report, decisions } = result;
    const hidden = useMemo(() => {
        const names = new Set(hiddenNames);
        return new Set(report.layers.flatMap((l, i) => (names.has(l.name) ? [i] : [])));
    }, [hiddenNames, report]);
    /** The layer lit up in the viewer: the row under the pointer, else the one picked. */
    const [hovered, setHovered] = useState(null);
    const [selected, setSelected] = useState(null);
    useEffect(() => {
        const v = new Viewer(host.current);
        v.onAxes = setAxes;
        v.onPick = setSelected;
        shownFile.current = null; // a new viewer has framed nothing yet
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
        if (!v || !preview)
            return;
        v.show(preview.buffers, preview.builtUp, shownFile.current !== preview.fileId);
        shownFile.current = preview.fileId;
        v.setEdges(edges);
        setSelected(null);
        setHovered(null);
        // `edges` is applied on its own below; new buffers must not re-run on toggles.
    }, [preview]);
    // A new orientation turns the existing preview.
    useEffect(() => viewer.current?.setOrientation(decisions.up_axis), [decisions.up_axis, preview]);
    const shownUnit = displayUnit(decisions.units, prefs.showIn);
    useEffect(() => viewer.current?.setUnit(unitSymbol(shownUnit.unit), shownUnit.factor), [shownUnit.unit, shownUnit.factor]);
    useEffect(() => viewer.current?.setEdges(edges), [edges]);
    // Layers left out of the drawing are hidden in the viewer (after each rebuild too).
    useEffect(() => viewer.current?.setHidden(hidden), [hidden, preview]);
    useEffect(() => viewer.current?.highlight(hovered ?? selected), [hovered, selected, preview]);
    useEffect(() => {
        const onKey = (e) => e.key === "Escape" && setSelected(null);
        window.addEventListener("keydown", onKey);
        return () => window.removeEventListener("keydown", onKey);
    }, []);
    const changeHidden = (next) => onHidden(report.layers.filter((_, i) => next.has(i)).map((l) => l.name));
    const available = preview?.buffers.available ?? true;
    const layerCount = report.layers.filter((l) => l.faces + l.polylines + l.points + l.surfaces > 0).length;
    /** The download, as the button and the keyboard make it: hidden layers left out. */
    const download = (pickLocation) => {
        if (busy === null && hidden.size < layerCount)
            onDownload(pickLocation);
    };
    // Everything the card says describes the file Download saves.
    const shown = hidden.size > 0 && visible ? visible : result;
    const downloadRef = useRef(download);
    downloadRef.current = download;
    useEffect(() => {
        const onKey = (e) => {
            if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "s") {
                e.preventDefault();
                downloadRef.current(e.shiftKey);
            }
        };
        window.addEventListener("keydown", onKey);
        return () => window.removeEventListener("keydown", onKey);
    }, []);
    const onOrtho = (on) => {
        setOrtho(on);
        viewer.current?.setOrtho(on);
    };
    const panel = {
        report: shown.report,
        decisions,
        format: prefs.format,
        ms: shown.ms,
        timings: shown.timings,
        exporter: inspection?.hints.exporter ?? null,
        busy,
        downloaded,
        unitsStated: inspection?.hints.units_source === "exporter",
        houseUnits: prefs.houseUnits,
        showIn: prefs.showIn,
        includeName: prefs.includeName,
        curves: prefs.curves,
        edges,
        ortho,
        hiddenCount: hidden.size,
        layerCount,
        onUp,
        onUnits,
        onHouseUnits,
        onShowIn,
        onKeepLoose,
        onFormat,
        onIncludeName,
        onCurves,
        onEdges: setEdges,
        onView: (v) => viewer.current?.setView(v),
        onOrtho,
        onFit: () => viewer.current?.fit(),
        onDownload: download,
        onDownloadReport,
        onAddMtl,
        onAnother,
    };
    return (_jsxs("main", { className: "relative flex h-dvh flex-col overflow-hidden bg-viewport lg:block", children: [_jsxs("div", { className: "relative min-h-[200px] flex-1 lg:absolute lg:inset-0", children: [_jsx("div", { ref: host, className: "absolute inset-0" }), !available && (_jsx("div", { className: "absolute inset-0 grid place-items-center p-6", children: _jsx("div", { className: "panel px-5 py-4 text-[13.5px] font-semibold", children: "No preview for this model" }) })), _jsx(AnimatePresence, { children: busy && (_jsxs(motion.div, { initial: { opacity: 0, y: -6 }, animate: { opacity: 1, y: 0 }, exit: { opacity: 0 }, className: "panel absolute top-[72px] left-1/2 z-10 flex -translate-x-1/2 items-center gap-2.5 !rounded-[4px] px-4 py-2 text-[13px] text-fg-2 lg:top-[76px]", role: "status", children: [_jsx("span", { className: "size-3.5 animate-spin rounded-full border-2 border-line border-t-accent" }), busy] })) }), _jsx("div", { className: "pointer-events-none absolute bottom-4 left-4 hidden sm:block", children: _jsx(AxisGizmo, { axes: axes, unit: unitSymbol(shownUnit.unit) }) }), _jsx("div", { className: "absolute right-4 bottom-4 lg:top-[76px] lg:right-[396px] lg:bottom-auto", children: _jsx(ViewTools, { edges: edges, ortho: ortho, onEdges: setEdges, onFit: panel.onFit, onView: panel.onView, onOrtho: onOrtho }) })] }), _jsxs("div", { className: "flex max-h-[58%] shrink-0 flex-col gap-3 overflow-y-auto border-t border-line bg-bg p-3 lg:contents", children: [_jsx(motion.div, { initial: { opacity: 0, x: 16 }, animate: { opacity: 1, x: 0 }, transition: { delay: 0.1 }, className: "pointer-events-none lg:absolute lg:top-[76px] lg:right-4 lg:flex lg:max-h-[calc(100%-92px)] lg:w-[364px]", children: _jsx(ResultCard, { ...panel }) }), _jsx(motion.div, { initial: { opacity: 0, x: -16 }, animate: { opacity: 1, x: 0 }, transition: { delay: 0.05 }, className: "pointer-events-none lg:absolute lg:top-[76px] lg:left-4 lg:flex lg:max-h-[calc(100%-200px)] lg:w-[268px]", children: _jsx(LayersCard, { report: report, mode: prefs.layerMode, hidden: hidden, busy: busy !== null, onMode: onLayerMode, onHidden: changeHidden, selected: selected, onSelect: setSelected, onHover: setHovered, inspection: inspection }) })] })] }));
}
