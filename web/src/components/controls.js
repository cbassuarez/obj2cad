import { Fragment as _Fragment, jsx as _jsx, jsxs as _jsxs } from "react/jsx-runtime";
// The result card's controls and the viewport's view tools.
import { Box, Check, ChevronDown, CircleCheck, Download, Rotate3d, Scan } from "lucide-react";
import { motion } from "motion/react";
import { Menu } from "@mantine/core";
import { Button } from "@/components/ui/button";
import { Tip } from "@/components/ui/tooltip";
import { canPickSaveLocation } from "@/lib/files";
import { bytes, fmt, measure } from "@/lib/format";
import { UNITS, displayUnit, formatInfo, unitName, unitSymbol } from "@/lib/settings";
import { cn, shortcut } from "@/lib/utils";
export const menuStyles = { dropdown: "panel !p-1", item: "!rounded-[3px] !text-[13px]", label: "!text-[11px]" };
const VIEWS = [
    { value: "iso", label: "Isometric" },
    { value: "top", label: "Top" },
    { value: "front", label: "Front" },
    { value: "right", label: "Right" },
];
const UNITS_FROM = { chosen: undefined, file: "auto", default: "default", assumed: "assumed" };
export const shortFormat = (f) => formatInfo(f).label.replace(" (binary)", "");
export const upChanged = (d) => d.up_from === "chosen" && d.up_axis !== d.detected_up_axis;
export const unitsChanged = (d) => d.units_from === "chosen" && d.units !== d.detected_units;
export const unitsLabel = (d) => (d.units === "unitless" ? "None" : unitName(d.units));
/** "auto" / "default" beside a value the app decided, or a Reset link once the user changed it. */
export function Provenance({ tag, onReset, className }) {
    if (onReset)
        return (_jsx("button", { type: "button", className: cn("cursor-pointer text-[12px] font-semibold text-accent hover:underline", className), onClick: onReset, children: "Reset" }));
    return tag ? _jsx("span", { className: cn("text-[12px] text-fg-3", className), children: tag }) : null;
}
export const upTag = (d) => (d.up_from === "detected" ? "auto" : undefined);
export const unitsTag = (d) => UNITS_FROM[d.units_from];
/** The units, as an underlined value that opens the list. */
export function UnitsMenu({ decisions, unitsStated, houseUnits, onUnits, onHouseUnits, position = "bottom-start", children, }) {
    return (_jsxs(Menu, { position: position, offset: 6, width: 230, classNames: menuStyles, children: [_jsx(Menu.Target, { children: children ?? (_jsxs("button", { type: "button", className: "inline-flex cursor-pointer items-center gap-0.5 font-semibold text-fg underline decoration-line underline-offset-2 hover:decoration-fg", children: [unitsLabel(decisions), _jsx(ChevronDown, { className: "size-3.5" })] })) }), _jsxs(Menu.Dropdown, { children: [_jsx(Menu.Label, { children: "The file's numbers are in" }), UNITS.map((u) => (_jsx(Menu.Item, { onClick: () => onUnits(u.value), rightSection: decisions.units === u.value ? _jsx(Check, { className: "size-3.5" }) : null, children: u.name }, u.value))), !unitsStated && decisions.units !== "unitless" && (_jsxs(_Fragment, { children: [_jsx(Menu.Divider, {}), _jsx(Menu.Item, { onClick: () => onHouseUnits(houseUnits === decisions.units ? null : decisions.units), rightSection: houseUnits === decisions.units ? _jsx(Check, { className: "size-3.5" }) : null, children: "Use for files without units" })] }))] })] }));
}
/** "120 × 80 × 40 mm" in the unit lengths are shown in, or null when the drawing is empty. */
export function sizeText(report, decisions, showIn) {
    const b = report.output.bounds;
    if (!b)
        return null;
    const { unit, factor } = displayUnit(decisions.units, showIn);
    const symbol = unitSymbol(unit);
    return `${b[1]
        .map((hi, a) => (hi - b[0][a]) * factor)
        .map(measure)
        .join(" × ")}${symbol ? ` ${symbol}` : ""}`;
}
/** What the download holds, given the layers hidden in the viewer. */
export const layersShown = ({ hiddenCount, layerCount }) => (hiddenCount === 0 ? "all" : hiddenCount < layerCount ? "some" : "none");
/** The one filled button on the page. Hidden layers are left out. */
export function DownloadButton({ report, format, busy, hiddenCount, layerCount, onDownload, className, }) {
    const shown = layersShown({ hiddenCount, layerCount });
    return (_jsx(Tip, { label: `Download (${shortcut("S")})`, children: _jsxs(Button, { variant: "primary", size: "lg", className: cn("w-full justify-between px-5", className), onClick: () => onDownload(false), disabled: busy !== null || shown === "none", children: [_jsxs("span", { className: "flex items-center gap-2.5", children: [_jsx(Download, {}), "Download ", shortFormat(format)] }), _jsx("span", { className: "num text-[12px] font-normal opacity-80", children: busy !== null ? "…" : shown === "all" ? bytes(report.output.bytes) : `${fmt(layerCount - hiddenCount)} of ${fmt(layerCount)} layers` })] }) }));
}
/** "Save as…" and the downloaded confirmation, under the download button. */
export function DownloadAfter({ busy, downloaded, hiddenCount, layerCount, onDownload, onAnother, }) {
    const shown = layersShown({ hiddenCount, layerCount });
    if (downloaded && busy === null && shown !== "none")
        return (_jsxs(motion.div, { initial: { opacity: 0, height: 0 }, animate: { opacity: 1, height: "auto" }, className: "flex items-center gap-2 text-[13px]", children: [_jsx(CircleCheck, { className: "size-4 shrink-0 text-exact" }), _jsxs("span", { className: "min-w-0 truncate", children: ["Downloaded ", _jsx("span", { className: "num", children: downloaded })] }), _jsx(Button, { variant: "link", className: "ml-auto h-auto px-0 text-[13px]", onClick: onAnother, children: "Convert another" })] }));
    return (_jsxs("div", { className: "flex min-h-5 items-center gap-3 text-[12.5px] text-fg-3", children: [_jsx("span", { children: shown === "all" ? "Everything in the viewer is included" : shown === "some" ? "Unticked layers are left out" : "No layer is ticked: tick one to download" }), canPickSaveLocation() && shown === "all" && (_jsx(Button, { variant: "link", className: "ml-auto h-auto px-0 text-[12.5px]", onClick: () => onDownload(true), disabled: busy !== null, children: "Save as\u2026" }))] }));
}
/** Edges, fit, named views and projection: a vertical icon strip beside the result card. */
export function ViewTools({ edges, ortho, onEdges, onFit, onView, onOrtho }) {
    return (_jsx("div", { className: "panel pointer-events-auto p-1", children: _jsxs("div", { className: "flex flex-col gap-0.5 rounded-[4px] bg-panel-2 p-[3px]", children: [_jsx(Tip, { label: "Edges", side: "left", children: _jsx(Button, { variant: "ghost", size: "icon-sm", className: "data-[on=true]:bg-panel-solid data-[on=true]:text-fg data-[on=true]:shadow-[inset_0_0_0_1px_var(--line)]", "data-on": edges, "aria-pressed": edges, "aria-label": "Edges", onClick: () => onEdges(!edges), children: _jsx(Box, {}) }) }), _jsx(Tip, { label: "Fit to view", side: "left", children: _jsx(Button, { variant: "ghost", size: "icon-sm", "aria-label": "Fit to view", onClick: onFit, children: _jsx(Scan, {}) }) }), _jsxs(Menu, { position: "left-start", offset: 8, width: 180, classNames: menuStyles, children: [_jsx(Menu.Target, { children: _jsx(Button, { variant: "ghost", size: "icon-sm", "aria-label": "Views", children: _jsx(Rotate3d, {}) }) }), _jsxs(Menu.Dropdown, { children: [_jsx(Menu.Label, { children: "Views" }), VIEWS.map((v) => (_jsx(Menu.Item, { onClick: () => onView(v.value), children: v.label }, v.value))), _jsx(Menu.Divider, {}), _jsx(Menu.Item, { onClick: () => onOrtho(!ortho), rightSection: ortho ? _jsx(Check, { className: "size-3.5" }) : null, children: "Orthographic" })] })] })] }) }));
}
