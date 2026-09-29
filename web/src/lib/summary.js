import { fmt } from "@/lib/format";
const plural = (n, one, many = `${one}s`) => `${fmt(n)} ${n === 1 ? one : many}`;
export function summarize(r) {
    const o = r.omissions;
    const leftOut = [
        o.freeform_surfaces > 0 && plural(o.freeform_surfaces, "curved surface"),
        o.freeform_curves > 0 && plural(o.freeform_curves, "curve"),
        o.broken_faces > 0 && plural(o.broken_faces, "broken face"),
    ].filter((x) => Boolean(x));
    const status = leftOut.length ? "partial" : "exact";
    const title = status === "partial" ? "Converted, with parts left out" : r.rotated ? "Exact copy, stood upright" : "Exact copy";
    const has = (code) => r.diagnostics.some((d) => d.code === code);
    const kept = r.diagnostics.find((d) => d.code === "loose_vertices_as_points");
    const loosePoints = o.loose_points > 0
        ? { count: o.loose_points, included: false }
        : kept && !r.point_cloud
            ? { count: Number(/(\d+)/.exec(kept.message)?.[1] ?? kept.count), included: true }
            : null;
    const notIncluded = [
        has("texcoords_dropped") && !(r.texture_colored_faces > 0) && "textures",
        (has("normals_dropped") || has("smoothing_groups_ignored")) && "smooth shading",
        has("vertex_colors_dropped") && "vertex colors",
    ].filter((x) => Boolean(x));
    const other = r.output.polylines + r.output.points + (r.output.splines ?? 0);
    const shapes = { count: r.output.faces + other, noun: other ? "shapes" : "faces" };
    const files = r.files ?? [];
    const named = (role) => files.filter((f) => f.role === role).map((f) => f.name);
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
        curves: ["cylinder", "cone", "sphere", "torus"]
            .map((k) => [k, (r.curves ?? []).filter((c) => c.kind === k).length])
            .filter(([, n]) => n > 0)
            .map(([k, n]) => plural(n, k, k === "torus" ? "tori" : `${k}s`)),
        shapes,
    };
}
