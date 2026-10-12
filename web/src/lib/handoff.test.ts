import { describe, expect, it } from "vitest";
import { downloadsFor, handoffItem, handoffLink } from "@/lib/handoff";
import type { Job } from "@/lib/files";
import { DEFAULT_PREFS } from "@/lib/settings";

(globalThis as Record<string, unknown>).__APP_VERSION__ = "1.2.3";

const file = (name: string, size: number) => new File([new Uint8Array(size)], name);

describe("handoff", () => {
  it("names the .zip, the folder or the model, as they are on disk", () => {
    const zip = file("scan (2).zip", 7);
    const entry = (name: string, size: number) => ({ name, size }) as never;
    const zipped: Job = { name: "scan (2).zip", sources: [{ file: zip, path: "a.obj", zip: entry("a.obj", 1) }, { file: zip, path: "cloud.xyz", zip: entry("cloud.xyz", 2) }] };
    expect(handoffItem(zipped)).toEqual({ name: "scan (2).zip", size: 7 });
    const folder: Job = { name: "site", sources: [{ file: file("a.obj", 3), path: "site/a.obj" }, { file: file("a.mtl", 1), path: "site/a.mtl" }] };
    expect(handoffItem(folder)).toEqual({ name: "site" });
    const loose: Job = { name: "", sources: [{ file: file("a.mtl", 1), path: "a.mtl" }, { file: file("a.obj", 3), path: "a.obj" }] };
    expect(handoffItem(loose)).toEqual({ name: "a.obj", size: 3 });
  });

  it("carries the remembered settings in the command-line version's words", () => {
    const job: Job = { name: "", sources: [{ file: file("scan & co.xyz", 4), path: "scan & co.xyz" }] };
    const url = new URL(handoffLink(job, { ...DEFAULT_PREFS, format: "dwg", layerMode: "materials", houseUnits: "millimeters", curves: true }));
    expect(url.protocol).toBe("obj2cad:");
    expect(Object.fromEntries(url.searchParams)).toEqual({ v: "1", name: "scan & co.xyz", size: "4", format: "dwg", layers: "materials", "default-units": "mm", curves: "1" });
  });

  it("offers the build for this computer", () => {
    expect(downloadsFor("Mozilla/5.0 (Windows NT 10.0; Win64; x64)")[0].url).toBe("https://github.com/cbassuarez/obj2cad/releases/download/obj2cad-v1.2.3/obj2cad-cli-1.2.3-x86_64-pc-windows-msvc.zip");
    expect(downloadsFor("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)").map((d) => d.label)).toEqual(["Mac (Apple silicon)", "Mac (Intel)"]);
    expect(downloadsFor("Mozilla/5.0 (X11; Linux x86_64)")[0].url).toMatch(/linux-gnu\.tar\.gz$/);
    expect(downloadsFor("Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X)")).toEqual([]);
  });
});
