// Everything the user can choose, in the engine's own vocabulary (the serde names in
// crates/obj2cad-core/src/convert.rs and crates/obj2cad-wasm/src/lib.rs), and the
// preferences the app remembers between files and visits.
export const UNITS = [
    { value: "millimeters", symbol: "mm", name: "Millimeters", meters: 0.001 },
    { value: "centimeters", symbol: "cm", name: "Centimeters", meters: 0.01 },
    { value: "meters", symbol: "m", name: "Meters", meters: 1 },
    { value: "inches", symbol: "in", name: "Inches", meters: 0.0254 },
    { value: "feet", symbol: "ft", name: "Feet", meters: 0.3048 },
    { value: "unitless", symbol: "", name: "None", meters: null },
];
export const UPS = [
    { value: "as_is", label: "As exported" },
    { value: "y_up_to_z_up", label: "Stand upright" },
];
export const LAYER_MODES = [
    { value: "objects", label: "Objects" },
    { value: "groups", label: "Groups" },
    { value: "materials", label: "Materials" },
    { value: "single", label: "One layer" },
];
export const FORMATS = [
    { value: "dxf", label: "DXF", ext: "dxf", mime: "application/dxf" },
    { value: "dxf-binary", label: "DXF (binary)", ext: "dxf", mime: "application/dxf" },
    { value: "dwg", label: "DWG", ext: "dwg", mime: "application/acad", beta: true },
];
export const unitSymbol = (u) => UNITS.find((x) => x.value === u)?.symbol ?? "";
/** What lengths are shown in: `show` when set and the file has a unit, else the file's
 *  unit. Display only: the drawing always keeps its own unit and coordinates. */
export function displayUnit(file, show) {
    const from = UNITS.find((x) => x.value === file)?.meters ?? null;
    const to = show ? (UNITS.find((x) => x.value === show)?.meters ?? null) : null;
    return from !== null && to !== null && show ? { unit: show, factor: from / to } : { unit: file, factor: 1 };
}
export const unitName = (u) => UNITS.find((x) => x.value === u)?.name ?? u;
export const formatInfo = (f) => FORMATS.find((x) => x.value === f);
export const DEFAULT_PREFS = { format: "dxf", layerMode: "objects", houseUnits: null, includeName: true, curves: false, showIn: null };
const PREFS_KEY = "obj2cad.prefs.v1";
const oneOf = (values, v, fallback) => values.some((x) => x.value === v) ? v : fallback;
/** Stored preferences, validated; defaults when storage is unavailable or damaged. */
export function loadPrefs(storage = safeStorage()) {
    try {
        const raw = JSON.parse(storage?.getItem(PREFS_KEY) ?? "{}");
        return {
            format: oneOf(FORMATS, raw.format, DEFAULT_PREFS.format),
            layerMode: oneOf(LAYER_MODES, raw.layerMode, DEFAULT_PREFS.layerMode),
            houseUnits: raw.houseUnits == null ? null : oneOf(UNITS, raw.houseUnits, "unitless") === "unitless" ? null : raw.houseUnits,
            includeName: typeof raw.includeName === "boolean" ? raw.includeName : DEFAULT_PREFS.includeName,
            curves: typeof raw.curves === "boolean" ? raw.curves : DEFAULT_PREFS.curves,
            showIn: raw.showIn == null || raw.showIn === "unitless" ? null : oneOf(UNITS, raw.showIn, "unitless") === "unitless" ? null : raw.showIn,
        };
    }
    catch {
        return DEFAULT_PREFS;
    }
}
export function savePrefs(p, storage = safeStorage()) {
    try {
        storage?.setItem(PREFS_KEY, JSON.stringify(p));
    }
    catch {
        /* private window or storage full: preferences last for this visit only */
    }
}
function safeStorage() {
    try {
        return typeof localStorage === "undefined" ? null : localStorage;
    }
    catch {
        return null;
    }
}
export const AUTO = { units: null, up: null, keepLoose: false };
/** The drawing's date comes from its files (the newest one used), set by the engine. */
export function engineSettings(prefs, choices, exclude = []) {
    return {
        units: choices.units,
        default_units: prefs.houseUnits,
        up_axis: choices.up,
        layer_mode: prefs.layerMode,
        keep_loose_points: choices.keepLoose,
        exclude_layers: exclude,
        format: prefs.format,
        include_name: prefs.includeName,
        curves: prefs.curves,
    };
}
/** Settings that move geometry: a change needs a new parity hash. (Curved surfaces are
 *  written next to the mesh; the parity hash covers the mesh.) */
export const geometryKey = (s) => JSON.stringify([s.up_axis, s.keep_loose_points, s.exclude_layers]);
/** Settings that change what the preview shows. Orientation is applied by rotating it. */
export const previewKey = (s) => JSON.stringify([s.layer_mode, s.keep_loose_points, s.exclude_layers, s.curves]);
