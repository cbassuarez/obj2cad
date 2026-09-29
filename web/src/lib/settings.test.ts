import { describe, expect, it } from "vitest";
import { AUTO, DEFAULT_PREFS, displayUnit, engineSettings, geometryKey, loadPrefs, previewKey, savePrefs } from "@/lib/settings";

const store = (value: string | null) => ({ getItem: () => value });

describe("preferences", () => {
  it("fall back to defaults when nothing is stored or storage is damaged", () => {
    expect(loadPrefs(null)).toEqual(DEFAULT_PREFS);
    expect(loadPrefs(store(null))).toEqual(DEFAULT_PREFS);
    expect(loadPrefs(store("{not json"))).toEqual(DEFAULT_PREFS);
  });

  it("keep valid values and drop unknown ones", () => {
    const p = loadPrefs(store(JSON.stringify({ format: "dwg", layerMode: "nope", houseUnits: "meters", includeName: false, curves: "yes" })));
    expect(p).toEqual({ format: "dwg", layerMode: "objects", houseUnits: "meters", includeName: false, curves: false, showIn: null });
    expect(loadPrefs(store(JSON.stringify({ showIn: "feet" }))).showIn).toBe("feet");
    expect(loadPrefs(store(JSON.stringify({ showIn: "cubits" }))).showIn).toBeNull();
    expect(loadPrefs(store(JSON.stringify({ curves: true }))).curves).toBe(true);
    expect(loadPrefs(store(JSON.stringify({ houseUnits: "unitless" }))).houseUnits).toBeNull();
    expect(loadPrefs(store(JSON.stringify({ houseUnits: "parsecs" }))).houseUnits).toBeNull();
  });

  it("round-trip through storage", () => {
    let saved = "";
    const p = { ...DEFAULT_PREFS, format: "dxf-binary" as const, houseUnits: "inches" as const };
    savePrefs(p, { setItem: (_, v) => (saved = v) });
    expect(loadPrefs(store(saved))).toEqual(p);
  });
});

describe("engine settings", () => {
  it("pass the house unit as the default, not as a choice", () => {
    const s = engineSettings({ ...DEFAULT_PREFS, houseUnits: "feet" }, AUTO);
    expect([s.units, s.default_units]).toEqual([null, "feet"]);
  });

  it("rebuild the preview only when what it shows changes", () => {
    const base = engineSettings(DEFAULT_PREFS, AUTO);
    const upright = engineSettings(DEFAULT_PREFS, { ...AUTO, up: "y_up_to_z_up" });
    const units = engineSettings(DEFAULT_PREFS, { ...AUTO, units: "meters" });
    const layers = engineSettings({ ...DEFAULT_PREFS, layerMode: "materials" }, AUTO);
    const curves = engineSettings({ ...DEFAULT_PREFS, curves: true }, AUTO);
    // Orientation turns the existing preview; units only relabel it.
    expect(previewKey(upright)).toBe(previewKey(base));
    expect(previewKey(units)).toBe(previewKey(base));
    expect(previewKey(layers)).not.toBe(previewKey(base));
    expect(previewKey(curves)).not.toBe(previewKey(base));
    // The parity hash follows the mesh, never labels (curved surfaces sit next to it).
    expect(geometryKey(upright)).not.toBe(geometryKey(base));
    expect(geometryKey(units)).toBe(geometryKey(base));
    expect(geometryKey(layers)).toBe(geometryKey(base));
    expect(geometryKey(curves)).toBe(geometryKey(base));
  });
});

describe("display unit", () => {
  it("converts for display only, and only between real units", () => {
    expect(displayUnit("meters", "millimeters")).toEqual({ unit: "millimeters", factor: 1000 });
    expect(displayUnit("inches", "feet").factor).toBeCloseTo(1 / 12, 12);
    expect(displayUnit("meters", null)).toEqual({ unit: "meters", factor: 1 });
    // A file without a unit has nothing to convert from.
    expect(displayUnit("unitless", "millimeters")).toEqual({ unit: "unitless", factor: 1 });
  });
});
