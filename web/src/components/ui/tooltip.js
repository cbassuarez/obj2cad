import { jsx as _jsx, jsxs as _jsxs } from "react/jsx-runtime";
import { Tooltip as TooltipPrimitive } from "radix-ui";
import { cn } from "@/lib/utils";
export const TooltipProvider = TooltipPrimitive.Provider;
/** Minimal shadcn-style tooltip: `<Tip label="…"><button/></Tip>`. */
export function Tip({ label, side = "top", children, className, }) {
    return (_jsxs(TooltipPrimitive.Root, { delayDuration: 300, children: [_jsx(TooltipPrimitive.Trigger, { asChild: true, children: children }), _jsx(TooltipPrimitive.Portal, { children: _jsx(TooltipPrimitive.Content, { side: side, sideOffset: 8, className: cn("z-50 max-w-72 rounded-[3px] border border-line bg-panel-solid px-2.5 py-1.5 text-[12.5px] leading-snug text-fg shadow-panel", className), children: label }) })] }));
}
