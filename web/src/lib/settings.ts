export type Units = "mm" | "cm" | "m" | "in" | "ft" | "unitless";
export type Up = "as-is" | "y-to-z";
export type Theme = "dark" | "light";

export interface Settings {
  units: Units;
  up: Up;
}

export const UNITS: { value: Units; label: string; name: string }[] = [
  { value: "mm", label: "mm", name: "millimeters" },
  { value: "cm", label: "cm", name: "centimeters" },
  { value: "m", label: "m", name: "meters" },
  { value: "in", label: "in", name: "inches" },
  { value: "ft", label: "ft", name: "feet" },
  { value: "unitless", label: "none", name: "no unit" },
];

export const UPS: { value: Up; label: string; short: string; detail: string }[] = [
  { value: "as-is", label: "As exported", short: "As exported", detail: "Coordinates exactly as in the file" },
  { value: "y-to-z", label: "Y-up → Z-up", short: "Y-up → Z-up", detail: "Stands Y-up models upright for CAD. Still exact." },
];

/** The core reports hints with its own enum names. */
export const HINT_UNITS: Record<string, Units> = {
  millimeters: "mm",
  centimeters: "cm",
  meters: "m",
  inches: "in",
  feet: "ft",
  unitless: "unitless",
};
export const HINT_UP: Record<string, Up> = { as_is: "as-is", y_up_to_z_up: "y-to-z" };

const SETTINGS_KEY = "obj2cad.settings.v1";
const THEME_KEY = "obj2cad.theme";

// Storage can be unavailable (private windows, blocked site data): everything falls back
// to in-memory defaults and the app keeps working.
export function loadSettings(): Settings | null {
  try {
    const s = JSON.parse(localStorage.getItem(SETTINGS_KEY) ?? "null");
    return s && UNITS.some((u) => u.value === s.units) && UPS.some((u) => u.value === s.up) ? s : null;
  } catch {
    return null;
  }
}

export function saveSettings(s: Settings): void {
  try {
    localStorage.setItem(SETTINGS_KEY, JSON.stringify(s));
  } catch {
    /* session-only */
  }
}

export function loadTheme(): Theme {
  return document.documentElement.classList.contains("light") ? "light" : "dark";
}

export function applyTheme(t: Theme): void {
  document.documentElement.classList.remove("light", "dark");
  document.documentElement.classList.add(t);
  document.querySelector('meta[name="theme-color"]')?.setAttribute("content", t === "dark" ? "#0e0f11" : "#f4f2ee");
  try {
    localStorage.setItem(THEME_KEY, t);
  } catch {
    /* session-only */
  }
}
