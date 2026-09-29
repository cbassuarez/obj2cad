import { jsx as _jsx, jsxs as _jsxs } from "react/jsx-runtime";
const R = 26;
/** Live axis indicator: projects the camera's view of X/Y/Z, back-to-front. */
export function AxisGizmo({ axes, unit }) {
    const items = axes
        ? ["x", "y", "z"]
            .map((k) => ({ k, d: axes[k] }))
            .sort((a, b) => a.d[2] - b.d[2])
        : [];
    return (_jsxs("div", { className: "panel pointer-events-auto flex items-center gap-3 py-2 pr-4 pl-2", children: [_jsxs("svg", { viewBox: "-36 -36 72 72", className: "size-[60px]", role: "img", "aria-label": "Axes, Z up", children: [_jsx("circle", { r: "34", fill: "none", stroke: "var(--line)" }), items.map(({ k, d }) => {
                        const x = d[0] * R;
                        const y = -d[1] * R;
                        const color = `var(--axis-${k})`;
                        return (_jsxs("g", { opacity: d[2] < -0.2 ? 0.45 : 1, children: [_jsx("line", { x1: "0", y1: "0", x2: x, y2: y, stroke: color, strokeWidth: "2.2", strokeLinecap: "round" }), _jsx("circle", { cx: x * 1.18, cy: y * 1.18, r: "6.5", fill: "var(--panel-solid)", stroke: color }), _jsx("text", { x: x * 1.18, y: y * 1.18 + 3, textAnchor: "middle", fontSize: "8.5", fontFamily: "var(--font-mono)", fill: color, children: k.toUpperCase() })] }, k));
                    })] }), _jsxs("div", { className: "num text-[11.5px] leading-relaxed text-fg-3", children: ["Z up", _jsx("br", {}), unit || "no unit"] })] }));
}
