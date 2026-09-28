import { describe, expect, it } from "vitest";
import type { Report } from "@/lib/engine";
import { designerNotes } from "@/lib/notes";

const report = (diagnostics: Report["diagnostics"]) => ({ diagnostics }) as Report;
const diag = (code: string, message: string, count = 1): Report["diagnostics"][number] => ({
  severity: "warning",
  code,
  line: 0,
  count,
  message,
});

describe("designerNotes", () => {
  it("says nothing when the file is fully included", () => {
    expect(designerNotes(report([]))).toEqual([]);
  });

  it("hides engine bookkeeping from designers", () => {
    const notes = designerNotes(
      report([
        diag("shared_vertices_repeated", "faces are grouped into 2 mesh entities…"),
        diag("mesh_split", "large meshes were split…"),
        diag("face_repeated_vertex", "face uses the same vertex more than once"),
        diag("multiple_groups", "elements belong to several groups"),
      ]),
    );
    expect(notes).toEqual([]);
  });

  it("merges normals and smoothing groups into one plain note", () => {
    const notes = designerNotes(
      report([diag("normals_dropped", "vertex normals (vn) … (12 in source)"), diag("smoothing_groups_ignored", "smoothing groups …")]),
    );
    expect(notes).toHaveLength(1);
    expect(notes[0].text).toMatch(/smooth shading/i);
  });

  it("offers an action when material colors are missing", () => {
    const [note] = designerNotes(report([diag("material_colors_unavailable", "materials are referenced but no MTL…")]));
    expect(note.action).toBe("add-mtl");
  });

  it("uses the count from the engine message for loose points", () => {
    const [note] = designerNotes(report([diag("unreferenced_vertices", "1234 vertices are not used by any face…")]));
    expect(note.text).toContain("1,234 loose points");
  });

  it("pluralizes skipped faces", () => {
    expect(designerNotes(report([diag("face_too_small", "…", 1)]))[0].text).toMatch(/1 broken face \(/);
    expect(designerNotes(report([diag("face_too_small", "…", 3)]))[0].text).toMatch(/3 broken faces/);
  });
});
