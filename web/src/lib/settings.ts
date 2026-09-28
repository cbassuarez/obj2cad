export type Units = "mm" | "cm" | "m" | "in" | "ft" | "unitless";
export type Up = "as-is" | "y-to-z";

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

export const UPS: { value: Up; label: string; short: string }[] = [
  { value: "as-is", label: "Keep as exported", short: "As exported" },
  { value: "y-to-z", label: "Stand upright (Y-up → Z-up)", short: "Stand upright" },
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
