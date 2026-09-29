import { jsx as _jsx, jsxs as _jsxs } from "react/jsx-runtime";
import { Layers, Files } from "lucide-react";
import { motion } from "motion/react";
import { Button } from "@/components/ui/button";
import { baseName } from "@/lib/files";
/** Several loose models: one drawing with all of them, or one drawing each. */
export function ChoiceScreen({ separate, onCombine, onSeparate }) {
    const names = separate.map((j) => baseName(j.sources[0].path));
    return (_jsx("main", { className: "paper flex h-dvh items-center justify-center overflow-hidden px-4 pt-[76px] pb-4", children: _jsxs(motion.div, { initial: { opacity: 0, y: 10 }, animate: { opacity: 1, y: 0 }, className: "panel flex max-h-full w-full max-w-[520px] flex-col gap-5 overflow-y-auto p-6", role: "dialog", "aria-labelledby": "choice-title", children: [_jsxs("div", { children: [_jsxs("h1", { id: "choice-title", className: "m-0 text-[17px] font-semibold", children: [names.length, " models"] }), _jsx("p", { className: "m-0 mt-1 truncate text-[13.5px] text-fg-3", title: names.join(", "), children: names.join(", ") })] }), _jsxs("div", { className: "flex flex-wrap gap-3", children: [_jsxs(Button, { variant: "primary", onClick: onCombine, children: [_jsx(Layers, {}), "Combine into one drawing"] }), _jsxs(Button, { onClick: onSeparate, children: [_jsx(Files, {}), "Convert separately"] })] })] }) }));
}
