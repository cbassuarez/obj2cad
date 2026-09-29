import { describe, expect, it } from "vitest";
import { AUTO, DEFAULT_PREFS, engineSettings, geometryKey, loadPrefs, previewKey, savePrefs, type Prefs } from "@/lib/settings";

const store = (value: string | null) => ({ getItem: () => value });

describe("preferences", () => {
  it("round-trip through storage", () => {
    const saved: Record<string, string> = {};
    const prefs: Prefs = { format: "dwg", layerMode: "materials", houseUnits: "inches", includeName: false };
    savePrefs(prefs, { setItem: (k, v) => void (saved[k] = v) });
    expect(loadPrefs({ getItem: (k) => saved[k] ?? null })).toEqual(prefs);
  });

  it("fall back to defaults when missing, damaged or unavailable", () => {
    expect(loadPrefs(store(null))).toEqual(DEFAULT_PREFS);
    expect(loadPrefs(store("{not json"))).toEqual(DEFAULT_PREFS);
    expect(loadPrefs(null)).toEqual(DEFAULT_PREFS);
  });

  it("drop unknown values field by field", () => {
    const raw = JSON.stringify({ format: "stl", layerMode: "groups", houseUnits: "parsecs", includeName: "yes" });
    expect(loadPrefs(store(raw))).toEqual({ ...DEFAULT_PREFS, layerMode: "groups" });
    // "No units" is never a house unit: files without units stay without them.
    expect(loadPrefs(store(JSON.stringify({ houseUnits: "unitless" }))).houseUnits).toBeNull();
  });

  it("survive storage that throws", () => {
    expect(() =>
      savePrefs(DEFAULT_PREFS, {
        setItem: () => {
          throw new Error("quota");
        },
      }),
    ).not.toThrow();
  });
});

describe("engine settings", () => {
  it("take the file's modified time in whole seconds, like the command-line tool", () => {
    expect(engineSettings(DEFAULT_PREFS, AUTO, { lastModified: 1_700_000_000_999 }).created_unix).toBe(1_700_000_000);
    expect(engineSettings(DEFAULT_PREFS, AUTO, { lastModified: 0 }).created_unix).toBeNull();
    expect(engineSettings(DEFAULT_PREFS, AUTO, null).created_unix).toBeNull();
  });

  it("keep units and up direction undecided until the user chooses", () => {
    const s = engineSettings({ ...DEFAULT_PREFS, houseUnits: "feet" }, AUTO, null);
    expect(s.units).toBeNull();
    expect(s.up_axis).toBeNull();
    expect(s.default_units).toBe("feet");
  });

  it("need a new parity hash only when geometry moves", () => {
    const base = engineSettings(DEFAULT_PREFS, AUTO, null);
    expect(geometryKey(engineSettings({ ...DEFAULT_PREFS, format: "dwg" }, AUTO, null))).toBe(geometryKey(base));
    expect(geometryKey(engineSettings(DEFAULT_PREFS, { ...AUTO, units: "meters" }, null))).toBe(geometryKey(base));
    expect(geometryKey(engineSettings(DEFAULT_PREFS, { ...AUTO, up: "y_up_to_z_up" }, null))).not.toBe(geometryKey(base));
    expect(geometryKey(engineSettings(DEFAULT_PREFS, AUTO, null, ["Roof"]))).not.toBe(geometryKey(base));
  });

  it("rebuild the preview for layers and loose points, not for units, format or orientation", () => {
    const base = previewKey(engineSettings(DEFAULT_PREFS, AUTO, null));
    expect(previewKey(engineSettings(DEFAULT_PREFS, { ...AUTO, units: "feet", up: "y_up_to_z_up" }, null))).toBe(base);
    expect(previewKey(engineSettings({ ...DEFAULT_PREFS, format: "dxf-binary" }, AUTO, null))).toBe(base);
    expect(previewKey(engineSettings({ ...DEFAULT_PREFS, layerMode: "single" }, AUTO, null))).not.toBe(base);
    expect(previewKey(engineSettings(DEFAULT_PREFS, { ...AUTO, keepLoose: true }, null))).not.toBe(base);
  });
});
