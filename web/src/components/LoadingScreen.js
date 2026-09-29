import { jsx as _jsx, jsxs as _jsxs } from "react/jsx-runtime";
import { motion } from "motion/react";
import { CubeArt } from "@/components/brand";
import { Button } from "@/components/ui/button";
import { bytes } from "@/lib/format";
const STAGE = { read: "Reading", parse: "Reading" };
/** Opening a file: reading it, then parsing it, with progress for large files. */
export function LoadingScreen({ name, progress }) {
    // Reading is the first half of the bar, parsing the second.
    const frac = progress ? (progress.stage === "read" ? 0 : 0.5) + (progress.done / Math.max(progress.total, 1)) * 0.5 : null;
    return (_jsx("main", { className: "paper flex h-dvh items-center justify-center overflow-hidden px-4", role: "status", "aria-live": "polite", children: _jsxs("div", { className: "panel flex w-full max-w-[420px] flex-col items-center gap-4 px-10 py-8", children: [_jsx(motion.div, { animate: { rotate: [0, 0, 120, 120] }, transition: { duration: 1.8, repeat: Infinity, times: [0, 0.3, 0.7, 1] }, children: _jsx(CubeArt, { className: "size-12 text-accent" }) }), _jsxs("div", { className: "max-w-full truncate text-[14px] text-fg-2", children: [progress ? STAGE[progress.stage] : "Opening", " ", _jsx("span", { className: "num text-fg", children: name })] }), _jsx("div", { className: "h-1.5 w-full overflow-hidden rounded-[2px] bg-panel-2", children: frac === null ? (_jsx(motion.div, { className: "h-full w-1/3 bg-accent", animate: { x: ["-100%", "300%"] }, transition: { duration: 1.4, repeat: Infinity, ease: "easeInOut" } })) : (_jsx("div", { className: "h-full bg-accent transition-[width] duration-200", style: { width: `${Math.round(frac * 100)}%` } })) }), progress && (_jsxs("div", { className: "num text-[12px] text-fg-3", children: [bytes(progress.done), " / ", bytes(progress.total)] }))] }) }));
}
/** A file too large to be comfortable in a browser tab. */
export function PreflightScreen({ name, size, onContinue, onCancel, cliUrl }) {
    return (_jsx("main", { className: "paper flex h-dvh items-center justify-center overflow-hidden px-4", role: "alertdialog", "aria-labelledby": "preflight-title", children: _jsxs("div", { className: "panel flex w-full max-w-[520px] flex-col gap-4 p-6", children: [_jsxs("div", { id: "preflight-title", className: "text-[16px] font-semibold", children: [name, " is ", bytes(size)] }), _jsxs("div", { className: "flex flex-wrap gap-3", children: [_jsx(Button, { variant: "primary", asChild: true, children: _jsx("a", { href: cliUrl, target: "_blank", rel: "noreferrer", children: "Command-line version" }) }), _jsx(Button, { onClick: onContinue, children: "Convert here anyway" }), _jsx(Button, { variant: "ghost", onClick: onCancel, children: "Cancel" })] })] }) }));
}
