import { describe, expect, it } from "vitest";
import { AUTO, DEFAULT_PREFS, engineSettings, geometryKey, loadPrefs, previewKey, savePrefs } from "@/lib/settings";

const store = (value: string | null) => ({ getItem: () => value });

describe("preferences", () => {
  it("fall back to defaults when nothing is stored or storage is damaged", () => {
    expect(loadPrefs(null)).toEqual(DEFAULT_PREFS);
    expect(loadPrefs(store(null))).toEqual(DEFAULT_PREFS);
    expect(loadPrefs(store("{not json"))).toEqual(DEFAULT_PREFS);
  });

  it("keep valid values and drop unknown ones", () => {
    const p = loadPrefs(store(JSON.stringify({ format: "dwg", layerMode: "nope", houseUnits: "meters", includeName: false })));
    expect(p).toEqual({ format: "dwg", layerMode: "objects", houseUnits: "meters", includeName: false });
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
  it("use whole seconds of the file's time, like the command-line tool", () => {
    expect(engineSettings(DEFAULT_PREFS, AUTO, { lastModified: 1_700_000_000_999 }).created_unix).toBe(1_700_000_000);
    expect(engineSettings(DEFAULT_PREFS, AUTO, { lastModified: 0 }).created_unix).toBeNull();
    expect(engineSettings(DEFAULT_PREFS, AUTO, null).created_unix).toBeNull();
  });

  it("pass the house unit as the default, not as a choice", () => {
    const s = engineSettings({ ...DEFAULT_PREFS, houseUnits: "feet" }, AUTO, null);
    expect([s.units, s.default_units]).toEqual([null, "feet"]);
  });

  it("rebuild the preview only when what it shows changes", () => {
    const base = engineSettings(DEFAULT_PREFS, AUTO, null);
    const upright = engineSettings(DEFAULT_PREFS, { ...AUTO, up: "y_up_to_z_up" }, null);
    const units = engineSettings(DEFAULT_PREFS, { ...AUTO, units: "meters" }, null);
    const layers = engineSettings({ ...DEFAULT_PREFS, layerMode: "materials" }, AUTO, null);
    // Orientation turns the existing preview; units only relabel it.
    expect(previewKey(upright)).toBe(previewKey(base));
    expect(previewKey(units)).toBe(previewKey(base));
    expect(previewKey(layers)).not.toBe(previewKey(base));
    // The parity hash follows geometry, never labels.
    expect(geometryKey(upright)).not.toBe(geometryKey(base));
    expect(geometryKey(units)).toBe(geometryKey(base));
    expect(geometryKey(layers)).toBe(geometryKey(base));
  });
});
