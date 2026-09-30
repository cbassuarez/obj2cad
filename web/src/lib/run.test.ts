import { describe, expect, it } from "vitest";
import { finishRun, onHash, onProgress, onShown, runLabel, startRun, type Run, type StationKey } from "@/lib/run";
import type { Progress } from "@/lib/engine";

const st = (r: Run, k: StationKey) => r.stations.find((s) => s.key === k)!;
const p = (stage: Progress["stage"], done = 0, total = 0, extra: Partial<Progress> = {}): Progress => ({ stage, done, total, ...extra });

describe("the station rail", () => {
  it("has a station per step, in order", () => {
    expect(startRun({ total: 10, geometry: 10, format: "DXF", curves: false }).stations.map((s) => s.label)).toEqual(["Read", "Model", "View", "Fingerprint", "Write DXF"]);
    expect(startRun({ total: 10, geometry: 10, format: "DWG", curves: true }).stations.map((s) => s.key)).toEqual(["engine", "read", "model", "curves", "view", "hash", "write"]);
  });

  it("follows one file through, with bars only for counted steps", () => {
    let r = startRun({ total: 1000, geometry: 1000, format: "DXF", curves: false });
    r = onProgress(r, p("read", 400, 1000));
    expect(st(r, "read")).toMatchObject({ state: "active", frac: 0.4 });
    r = onProgress(r, p("read", 1000, 1000));
    r = onProgress(r, p("parse", 500, 1000, { file: "a.obj", count: 1234, cloud: false }));
    expect(st(r, "read")).toMatchObject({ state: "done", frac: 1 });
    expect(st(r, "model")).toMatchObject({ state: "active", frac: 0.5, value: "1,234 vertices" });
    r = onProgress(r, p("preview"));
    expect(st(r, "model").state).toBe("done");
    expect(st(r, "view")).toMatchObject({ state: "active", frac: null });
    r = onShown(r, null);
    expect(st(r, "view").state).toBe("done");
    r = onProgress(r, p("hash"));
    expect(st(r, "hash")).toMatchObject({ state: "active", frac: null, value: "computing" });
    r = onProgress(r, p("write"));
    expect(st(r, "hash")).toMatchObject({ state: "done", value: "" });
    expect(st(r, "write")).toMatchObject({ state: "active", frac: null });
    r = onProgress(r, p("write", 25, 100));
    expect(st(r, "write")).toMatchObject({ frac: 0.25, value: "25%" });
    expect(runLabel(r)).toBe("Write DXF 25%");
    r = finishRun(onHash(r, "a3f904c1d2e37e2dc21e"));
    expect(r.stations.every((s) => s.state === "done")).toBe(true);
    expect(st(r, "hash").value).toBe("a3f9…c21e");
  });

  it("never goes backwards while a bundle's large file loads after its model", () => {
    // building.obj (100 bytes) is read, parsed and shown first; scan.xyz (900) after.
    let r = startRun({ total: 1000, geometry: 1000, format: "DXF", curves: false });
    r = onProgress(r, p("read", 100, 100));
    r = onProgress(r, p("parse", 100, 100, { file: "building.obj", count: 8, cloud: false }));
    r = onShown(r, { names: ["scan.xyz"] });
    expect(st(r, "read")).toMatchObject({ state: "paused", frac: 0.1 });
    expect(st(r, "model")).toMatchObject({ state: "paused", frac: 0.1 });
    expect(st(r, "view")).toMatchObject({ state: "paused", value: "in part" });
    // Everything is read again, the model first: the bars hold, then grow.
    r = onProgress(r, p("read", 50, 1000));
    expect(st(r, "read")).toMatchObject({ state: "active", frac: 0.1 });
    r = onProgress(r, p("read", 1000, 1000));
    r = onProgress(r, p("parse", 60, 1000, { file: "building.obj", count: 4, cloud: false }));
    expect(st(r, "model").frac).toBe(0.1);
    expect(st(r, "read").state).toBe("done");
    r = onProgress(r, p("parse", 700, 1000, { file: "scan.xyz", count: 2_000_000, cloud: true }));
    expect(st(r, "model")).toMatchObject({ frac: 0.7, value: "2,000,000 points" });
    expect(st(r, "view").state).toBe("paused");
    r = onProgress(r, p("preview"));
    r = onShown(r, null);
    expect(st(r, "view").state).toBe("done");
    expect(st(r, "model")).toMatchObject({ state: "done", frac: 1 });
  });

  it("counts bars in whole steps and caps them at the total", () => {
    let r = startRun({ total: 100, geometry: 100, format: "DXF", curves: false });
    r = onProgress(r, p("read", 150, 100));
    expect(st(r, "read").frac).toBe(1);
    r = onProgress(r, p("write", 999, 1000));
    expect(st(r, "write").value).toBe("99%");
  });

  it("a DWG is written without a count", () => {
    let r = startRun({ total: 10, geometry: 10, format: "DWG", curves: false });
    r = onProgress(r, p("engine"));
    expect(st(r, "engine").state).toBe("active");
    r = onProgress(r, p("read", 10, 10));
    expect(st(r, "engine").state).toBe("done");
    r = onProgress(r, p("write"));
    expect(st(r, "write")).toMatchObject({ frac: null, value: "writing" });
  });
});
