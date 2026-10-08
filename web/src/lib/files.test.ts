import { describe, expect, it } from "vitest";
import { zipSync } from "fflate";
import { baseName, jobName, plan, stem } from "@/lib/files";

const file = (name: string, text = "") => new File([text], name, { lastModified: 1_700_000_000_000 });
const paths = (sources: { path: string }[]) => sources.map((s) => s.path);

describe("names", () => {
  it("ignore folders", () => {
    expect(baseName("scan/textures/a.mtl")).toBe("a.mtl");
    expect(baseName("C:\\models\\ring.obj")).toBe("ring.obj");
    expect(stem("dir/ring.v2.obj")).toBe("ring.v2");
  });
});

describe("plan", () => {
  it("makes one drawing of a model and its material library, and lists the rest", async () => {
    const p = await plan([file("a.obj"), file("a.mtl"), file("notes.txt")]);
    expect(p.kind).toBe("one");
    if (p.kind === "one") expect(paths(p.job.sources)).toEqual(["a.obj", "a.mtl"]);
  });

  it("reads extensions as the engine does: a name without a dot has none", async () => {
    const p = await plan([file("obj"), file("png"), file("README")]);
    expect(p).toEqual({ kind: "none", ignored: ["obj", "png", "README"] });
  });

  it("offers loose models together or apart, each with the library it names", async () => {
    const p = await plan([file("a.obj", "mtllib shared.mtl\nv 0 0 0\n"), file("b.obj"), file("shared.mtl")]);
    expect(p.kind).toBe("choose");
    if (p.kind === "choose") {
      expect(paths(p.combined.sources)).toEqual(["a.obj", "b.obj", "shared.mtl"]);
      expect(p.separate.map((j) => paths(j.sources))).toEqual([["a.obj", "shared.mtl"], ["b.obj"]]);
    }
  });

  it("uses a .zip's models and materials, skipping hidden and unused entries", async () => {
    const zip = zipSync({
      "scan/model.obj": new TextEncoder().encode("v 0 0 0\n"),
      "scan/model.mtl": new TextEncoder().encode("newmtl a\n"),
      "scan/readme.txt": new TextEncoder().encode("hi"),
      "__MACOSX/scan/._model.obj": new Uint8Array([0]),
    });
    const p = await plan([new File([zip], "bundle.zip")]);
    expect(p.kind).toBe("one");
    if (p.kind === "one") {
      expect(p.job.name).toBe("bundle.zip");
      expect(paths(p.job.sources)).toEqual(["scan/model.obj", "scan/model.mtl"]);
      expect(jobName(p.job)).toBe("bundle.zip");
    }
  });
});
