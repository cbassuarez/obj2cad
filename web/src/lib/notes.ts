// Engine diagnostics → what a designer needs to know. Codes without an entry are
// bookkeeping (how entities were organized, spec trivia): they stay in Technical details
// and the downloadable report, but never compete for attention in the main panel.
import type { Report } from "@/lib/engine";
import { fmt } from "@/lib/format";

export interface DesignerNote {
  code: string;
  text: string;
  /** Something the user can fix right now (e.g. add the .mtl). */
  action?: "add-mtl";
}

type Diag = Report["diagnostics"][number];

const COPY: Record<string, (d: Diag, n: number) => string> = {
  texcoords_dropped: () => "Texture mapping (UVs) isn't included. DXF has no textures; the shapes are exact.",
  normals_dropped: () => "Smooth shading isn't included. Faces show flat in CAD; the geometry is exact.",
  vertex_colors_dropped: () => "Per-point colors aren't included.",
  material_colors_unavailable: () => "Add the .mtl file to keep material colors.",
  unreferenced_vertices: (_, n) => `${fmt(n)} loose points that weren't part of any face or line were left out.`,
  freeform_not_converted: () => "This file has curved NURBS surfaces, which aren't converted yet.",
  face_too_small: (d) => `${fmt(d.count)} broken face${d.count === 1 ? "" : "s"} (fewer than 3 corners) ${d.count === 1 ? "was" : "were"} skipped.`,
  layer_renamed: () => "Some object names were adjusted to be valid AutoCAD layer names.",
};

/** Normals and smoothing groups are the same thing to a designer: show one note. */
const SAME_AS: Record<string, string> = { smoothing_groups_ignored: "normals_dropped" };

export function designerNotes(report: Report): DesignerNote[] {
  const seen = new Set<string>();
  const out: DesignerNote[] = [];
  for (const d of report.diagnostics) {
    const code = SAME_AS[d.code] ?? d.code;
    const copy = COPY[code];
    if (!copy || seen.has(code)) continue;
    seen.add(code);
    const n = Number(/^(\d+)/.exec(d.message)?.[1] ?? d.count);
    out.push({ code, text: copy(d, n), action: code === "material_colors_unavailable" ? "add-mtl" : undefined });
  }
  return out;
}
