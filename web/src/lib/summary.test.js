import { describe, expect, it } from "vitest";
import { summarize } from "@/lib/summary";
const omissions = {
    freeform_surfaces: 0,
    freeform_curves: 0,
    broken_faces: 0,
    loose_points: 0,
    excluded_layers: [],
    excluded_faces: 0,
    excluded_lines: 0,
    excluded_points: 0,
};
const report = (patch) => ({
    rotated: patch.rotated ?? false,
    point_cloud: patch.point_cloud ?? false,
    omissions: { ...omissions, ...patch.omissions },
    output: { faces: 12, polylines: patch.polylines ?? 0, points: patch.points ?? 0 },
    diagnostics: (patch.diagnostics ?? []).map(([code, message]) => ({ severity: "info", code, line: 0, count: 1, message })),
});
describe("result summary", () => {
    it("is an exact copy when nothing is left out", () => {
        const s = summarize(report({}));
        expect([s.status, s.title]).toEqual(["exact", "Exact copy"]);
        expect(summarize(report({ rotated: true })).title).toBe("Exact copy, stood upright");
    });
    it("never claims exact when geometry is left out", () => {
        const s = summarize(report({ rotated: true, omissions: { freeform_surfaces: 2, broken_faces: 1 } }));
        expect([s.status, s.title]).toEqual(["partial", "Converted, with parts left out"]);
        expect(s.leftOut).toEqual(["2 curved surfaces", "1 broken face"]);
    });
    it("lists loose points either way", () => {
        expect(summarize(report({ omissions: { loose_points: 3 } })).loosePoints).toEqual({ count: 3, included: false });
        const kept = summarize(report({ points: 3, diagnostics: [["loose_vertices_as_points", "3 unused vertices written as points"]] }));
        expect(kept.loosePoints).toEqual({ count: 3, included: true });
        // A point cloud is the model itself, not loose points.
        expect(summarize(report({ point_cloud: true, points: 3, diagnostics: [["loose_vertices_as_points", "3"]] })).loosePoints).toBeNull();
    });
    it("names what the drawing doesn't carry", () => {
        const s = summarize(report({ diagnostics: [["texcoords_dropped", ""], ["smoothing_groups_ignored", ""], ["material_colors_unavailable", ""]] }));
        expect(s.notIncluded).toEqual(["textures", "smooth shading"]);
        expect(s.needsMtl).toBe(true);
        expect(s.status).toBe("exact");
    });
    it("counts faces, or shapes when there are lines or points", () => {
        expect(summarize(report({})).shapes).toEqual({ count: 12, noun: "faces" });
        expect(summarize(report({ polylines: 2, points: 1 })).shapes).toEqual({ count: 15, noun: "shapes" });
    });
});
