import { describe, expect, it } from "vitest";
import { HINT_UNITS, HINT_UP, UNITS } from "@/lib/settings";

describe("engine hint names", () => {
  it("map every engine unit to a UI unit", () => {
    for (const u of Object.values(HINT_UNITS)) expect(UNITS.some((x) => x.value === u)).toBe(true);
    // The Rust enum's serde names (crates/obj2cad-core/src/convert.rs).
    expect(Object.keys(HINT_UNITS).sort()).toEqual(["centimeters", "feet", "inches", "meters", "millimeters", "unitless"]);
    expect(Object.keys(HINT_UP).sort()).toEqual(["as_is", "y_up_to_z_up"]);
  });
});
