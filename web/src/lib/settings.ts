// Everything the user can choose, in the engine's own vocabulary (the serde names in
// crates/obj2cad-core/src/convert.rs and crates/obj2cad-wasm/src/lib.rs), and the
// preferences the app remembers between files and visits.

export type Units = "millimeters" | "centimeters" | "meters" | "inches" | "feet" | "unitless";
export type UpAxis = "as_is" | "y_up_to_z_up";
export type LayerMode = "objects" | "groups" | "materials" | "single";
export type Format = "dxf" | "dxf-binary" | "dwg";

export const UNITS: { value: Units; symbol: string; name: string }[] = [
  { value: "millimeters", symbol: "mm", name: "Millimeters" },
  { value: "centimeters", symbol: "cm", name: "Centimeters" },
  { value: "meters", symbol: "m", name: "Meters" },
  { value: "inches", symbol: "in", name: "Inches" },
  { value: "feet", symbol: "ft", name: "Feet" },
  { value: "unitless", symbol: "", name: "None" },
];

export const UPS: { value: UpAxis; label: string }[] = [
  { value: "as_is", label: "As exported" },
  { value: "y_up_to_z_up", label: "Stand upright" },
];

export const LAYER_MODES: { value: LayerMode; label: string }[] = [
  { value: "objects", label: "Objects" },
  { value: "groups", label: "Groups" },
  { value: "materials", label: "Materials" },
  { value: "single", label: "One layer" },
];

export const FORMATS: { value: Format; label: string; ext: string; mime: string; beta?: boolean }[] = [
  { value: "dxf", label: "DXF", ext: "dxf", mime: "application/dxf" },
  { value: "dxf-binary", label: "DXF (binary)", ext: "dxf", mime: "application/dxf" },
  { value: "dwg", label: "DWG", ext: "dwg", mime: "application/acad", beta: true },
];

export const unitSymbol = (u: Units) => UNITS.find((x) => x.value === u)?.symbol ?? "";
export const unitName = (u: Units) => UNITS.find((x) => x.value === u)?.name ?? u;
export const formatInfo = (f: Format) => FORMATS.find((x) => x.value === f)!;

/** Remembered across files and visits (this browser only). */
export interface Prefs {
  format: Format;
  layerMode: LayerMode;
  /** Unit for files that don't state one. */
  houseUnits: Units | null;
  /** Record the source file name in the drawing's properties. */
  includeName: boolean;
  /** Also write curved surfaces recognized in the mesh. */
  curves: boolean;
}

export const DEFAULT_PREFS: Prefs = { format: "dxf", layerMode: "objects", houseUnits: null, includeName: true, curves: false };
const PREFS_KEY = "obj2cad.prefs.v1";

const oneOf = <T extends string>(values: readonly { value: T }[], v: unknown, fallback: T): T =>
  values.some((x) => x.value === v) ? (v as T) : fallback;

/** Stored preferences, validated; defaults when storage is unavailable or damaged. */
export function loadPrefs(storage: Pick<Storage, "getItem"> | null = safeStorage()): Prefs {
  try {
    const raw = JSON.parse(storage?.getItem(PREFS_KEY) ?? "{}") as Partial<Record<keyof Prefs, unknown>>;
    return {
      format: oneOf(FORMATS, raw.format, DEFAULT_PREFS.format),
      layerMode: oneOf(LAYER_MODES, raw.layerMode, DEFAULT_PREFS.layerMode),
      houseUnits: raw.houseUnits == null ? null : oneOf(UNITS, raw.houseUnits, "unitless") === "unitless" ? null : (raw.houseUnits as Units),
      includeName: typeof raw.includeName === "boolean" ? raw.includeName : DEFAULT_PREFS.includeName,
      curves: typeof raw.curves === "boolean" ? raw.curves : DEFAULT_PREFS.curves,
    };
  } catch {
    return DEFAULT_PREFS;
  }
}

export function savePrefs(p: Prefs, storage: Pick<Storage, "setItem"> | null = safeStorage()): void {
  try {
    storage?.setItem(PREFS_KEY, JSON.stringify(p));
  } catch {
    /* private window or storage full: preferences last for this visit only */
  }
}

function safeStorage(): Storage | null {
  try {
    return typeof localStorage === "undefined" ? null : localStorage;
  } catch {
    return null;
  }
}

/** Choices for one file. `null` means "decided from the file". */
export interface FileChoices {
  units: Units | null;
  up: UpAxis | null;
  keepLoose: boolean;
}

export const AUTO: FileChoices = { units: null, up: null, keepLoose: false };

/** What the engine takes (`Settings` in crates/obj2cad-wasm/src/lib.rs). */
export interface EngineSettings {
  units: Units | null;
  default_units: Units | null;
  up_axis: UpAxis | null;
  layer_mode: LayerMode;
  keep_loose_points: boolean;
  exclude_layers: string[];
  format: Format;
  include_name: boolean;
  curves: boolean;
}

/** The drawing's date comes from its files (the newest one used), set by the engine. */
export function engineSettings(prefs: Prefs, choices: FileChoices, exclude: string[] = []): EngineSettings {
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
export const geometryKey = (s: EngineSettings) => JSON.stringify([s.up_axis, s.keep_loose_points, s.exclude_layers]);

/** Settings that change what the preview shows. Orientation is applied by rotating it. */
export const previewKey = (s: EngineSettings) => JSON.stringify([s.layer_mode, s.keep_loose_points, s.exclude_layers, s.curves]);
