import { jsx as _jsx, jsxs as _jsxs } from "react/jsx-runtime";
import { Check, ChevronDown, CircleAlert, Download, TriangleAlert } from "lucide-react";
import { motion } from "motion/react";
import { Menu } from "@mantine/core";
import { Button } from "@/components/ui/button";
import { bytes, fmt } from "@/lib/format";
import { FORMATS, formatInfo } from "@/lib/settings";
import { summarize } from "@/lib/summary";
import { cn } from "@/lib/utils";
function Status({ item }) {
    if (item.status === "waiting")
        return _jsx("span", { className: "text-fg-3", children: "Waiting" });
    if (item.status === "converting")
        return (_jsxs("span", { className: "flex items-center gap-2 text-fg-2", children: [_jsx("span", { className: "size-3 animate-spin rounded-full border-2 border-line border-t-accent" }), "Converting"] }));
    if (item.status === "failed")
        return (_jsxs("span", { className: "flex min-w-0 items-center gap-1.5 text-danger", title: item.error?.lines.join("\n"), children: [_jsx(CircleAlert, { className: "size-4 shrink-0" }), _jsx("span", { className: "truncate", children: item.error?.title ?? "Failed" })] }));
    const s = summarize(item.result.report);
    return (_jsxs("span", { className: cn("flex min-w-0 items-center gap-1.5", s.status === "exact" ? "text-exact" : "text-warn"), children: [s.status === "exact" ? _jsx(Check, { className: "size-4 shrink-0" }) : _jsx(TriangleAlert, { className: "size-4 shrink-0" }), _jsx("span", { className: "truncate", children: s.title })] }));
}
/** Several drawings at once: each converted with its own detected settings. */
export function BatchScreen({ items, format, onFormat, onOpen, onDownload, onDownloadAll, zipping, }) {
    const done = items.filter((i) => i.status === "done");
    const running = items.some((i) => i.status === "waiting" || i.status === "converting");
    return (_jsx("main", { className: "paper flex h-dvh justify-center overflow-hidden px-4 pt-[76px] pb-4", children: _jsxs(motion.section, { initial: { opacity: 0, y: 10 }, animate: { opacity: 1, y: 0 }, className: "panel flex h-fit max-h-full min-h-0 w-full max-w-[860px] flex-col", "aria-label": "Files", children: [_jsxs("div", { className: "flex flex-wrap items-center gap-3 border-b border-line-soft px-5 py-4", children: [_jsxs("h1", { className: "m-0 text-[16px] font-semibold", children: [fmt(items.length), " drawings", _jsxs("span", { className: "ml-2 text-[13px] font-normal text-fg-3", children: [fmt(done.length), " converted", items.some((i) => i.status === "failed") && `, ${fmt(items.filter((i) => i.status === "failed").length)} failed`] })] }), _jsx("div", { className: "flex-1" }), _jsxs(Menu, { position: "bottom-end", offset: 6, width: 200, classNames: { dropdown: "panel !p-1", item: "!rounded-[3px] !text-[13px]" }, children: [_jsx(Menu.Target, { children: _jsxs(Button, { size: "sm", disabled: running, children: [formatInfo(format).label, _jsx(ChevronDown, {})] }) }), _jsx(Menu.Dropdown, { children: FORMATS.map((f) => (_jsxs(Menu.Item, { onClick: () => onFormat(f.value), rightSection: format === f.value ? _jsx(Check, { className: "size-3.5" }) : null, children: [f.label, f.beta && _jsx("span", { className: "ml-1.5 rounded-[2px] bg-warn-soft px-1 py-px text-[10.5px] font-medium text-warn", children: "beta" })] }, f.value))) })] }), _jsxs(Button, { variant: "primary", size: "sm", onClick: onDownloadAll, disabled: running || done.length === 0 || zipping, children: [_jsx(Download, {}), zipping ? "Zipping…" : "Download all (.zip)"] })] }), _jsx("ul", { className: "m-0 min-h-0 flex-1 list-none overflow-y-auto p-2", children: items.map((item) => (_jsxs("li", { className: "grid grid-cols-[minmax(0,1.4fr)_minmax(0,1.3fr)_auto] items-center gap-3 rounded-[3px] px-3 py-2 hover:bg-panel-2 sm:grid-cols-[minmax(0,1.4fr)_minmax(0,1.3fr)_90px_auto]", children: [_jsx("button", { type: "button", className: "min-w-0 cursor-pointer truncate text-left text-[14px] font-medium hover:text-accent disabled:cursor-default disabled:hover:text-fg", onClick: () => onOpen(item), disabled: item.status !== "done", children: item.name }), _jsx("span", { className: "min-w-0 text-[13px]", children: _jsx(Status, { item: item }) }), _jsx("span", { className: "num hidden text-right text-[12px] text-fg-3 sm:block", children: item.result ? bytes(item.result.report.output.bytes) : bytes(item.size) }), _jsx(Button, { variant: "ghost", size: "icon-sm", onClick: () => onDownload(item), disabled: item.status !== "done", "aria-label": `Download ${item.name}`, children: _jsx(Download, {}) })] }, item.id))) })] }) }));
}
