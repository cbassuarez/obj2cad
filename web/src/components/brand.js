import { jsx as _jsx, jsxs as _jsxs } from "react/jsx-runtime";
/** The obj2cad mark: an isometric cell whose top face is the exact, highlighted part. */
export function Logo({ className = "size-5" }) {
    return (_jsxs("svg", { viewBox: "0 0 24 24", fill: "none", stroke: "currentColor", strokeWidth: 1.8, strokeLinejoin: "round", className: className, "aria-hidden": "true", children: [_jsx("path", { d: "M12 2.8 20 7.4v9.2L12 21.2 4 16.6V7.4z" }), _jsx("path", { d: "M4 7.4 12 12l8-4.6M12 12v9.2" })] }));
}
/** Larger drafting illustration for the drop target: vertices marked as in a CAD snap. */
export function CubeArt({ className = "size-16" }) {
    return (_jsxs("svg", { viewBox: "0 0 96 96", fill: "none", stroke: "currentColor", strokeWidth: 2, strokeLinejoin: "round", className: className, "aria-hidden": "true", children: [_jsx("path", { d: "M48 12 80 30v36L48 84 16 66V30z", opacity: ".35" }), _jsx("path", { d: "M16 30l32 18 32-18M48 48v36", opacity: ".35" }), _jsx("path", { d: "M48 12 80 30 48 48 16 30z" }), [
                [48, 12],
                [80, 30],
                [16, 30],
                [48, 48],
            ].map(([x, y]) => (_jsx("rect", { x: x - 3, y: y - 3, width: 6, height: 6, fill: "currentColor", stroke: "none" }, `${x}-${y}`)))] }));
}
