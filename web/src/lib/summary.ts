// What the result panel says about a conversion: plain facts, no explanations.
import type { Report } from "@/lib/engine";
import { fmt } from "@/lib/format";

export type Status = "exact" | "partial";

export interface Summary {
  status: Status;
  /** "Exact copy", "Exact copy, stood upright", or "Converted, with parts left out". */
  title: string;
  /** Geometry in the file that is not in the drawing (only when partial). */
  leftOut: string[];
  /** Vertices no face or line uses. `null` when there are none. */
  loosePoints: { count: number; included: boolean } | null;
  /** Non-geometry data the drawing doesn't carry (textures, smooth shading…). */
  notIncluded: string[];
  /** Materials are used but no .mtl was given. */
  needsMtl: boolean;
  /** Files the drawing's files name that weren't there (a texture, a library). */
  missing: string[];
  /** Files that came along but weren't needed, or couldn't be read. */
  notUsed: string[];
  unreadable: string[];
  /** Some faces are colored from textures (approximate colors). */
  textureColors: boolean;
  /** Count and noun for the main stat. */
  shapes: { count: number; noun: string };
}

const plural = (n: number, one: string, many = `${one}s`) => `${fmt(n)} ${n === 1 ? one : many}`;

export function summarize(r: Report): Summary {
  const o = r.omissions;
  const leftOut = [
    o.freeform_surfaces > 0 && plural(o.freeform_surfaces, "curved surface"),
    o.freeform_curves > 0 && plural(o.freeform_curves, "curve"),
    o.broken_faces > 0 && plural(o.broken_faces, "broken face"),
  ].filter((x): x is string => Boolean(x));
  const status: Status = leftOut.length ? "partial" : "exact";
  const title = status === "partial" ? "Converted, with parts left out" : r.rotated ? "Exact copy, stood upright" : "Exact copy";

  const has = (code: string) => r.diagnostics.some((d) => d.code === code);
  const kept = r.diagnostics.find((d) => d.code === "loose_vertices_as_points");
  const loosePoints =
    o.loose_points > 0
      ? { count: o.loose_points, included: false }
      : kept && !r.point_cloud
        ? { count: Number(/(\d+)/.exec(kept.message)?.[1] ?? kept.count), included: true }
        : null;

  const notIncluded = [
    has("texcoords_dropped") && !(r.texture_colored_faces > 0) && "textures",
    (has("normals_dropped") || has("smoothing_groups_ignored")) && "smooth shading",
    has("vertex_colors_dropped") && "vertex colors",
  ].filter((x): x is string => Boolean(x));

  const other = r.output.polylines + r.output.points;
  const shapes = { count: r.output.faces + other, noun: other ? "shapes" : "faces" };
  const files = r.files ?? [];
  const named = (role: string) => files.filter((f) => f.role === role).map((f) => f.name);
  return {
    status,
    title,
    leftOut,
    loosePoints,
    notIncluded,
    needsMtl: has("material_colors_unavailable"),
    missing: named("missing"),
    notUsed: named("not_used"),
    unreadable: named("unreadable"),
    textureColors: (r.texture_colored_faces ?? 0) > 0,
    shapes,
  };
}
