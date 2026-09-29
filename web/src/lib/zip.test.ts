import { describe, expect, it } from "vitest";
import { zipSync } from "fflate";
import { extract, listZip } from "@/lib/zip";

const text = (s: string) => new TextEncoder().encode(s);

describe("zip", () => {
  it("lists an archive's files and takes each out exactly", async () => {
    const big = text("1.000 2.000 3.000 10 20 30\n".repeat(20000));
    const archive = new Blob([
      zipSync({
        "site/model.obj": [text("v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n"), { level: 0 }],
        "site/scan.xyz": [big, { level: 6 }],
        "site/empty/": new Uint8Array(0),
        "site/café.mtl": [text("newmtl a\nKd 1 0 0\n"), { level: 9 }],
      }),
    ]);
    const entries = await listZip(archive, "site.zip");
    expect(entries.map((e) => [e.name, e.method])).toEqual([
      ["site/model.obj", 0],
      ["site/scan.xyz", 8],
      ["site/café.mtl", 8],
    ]);
    const scan = entries[1];
    let seen = 0;
    const got = await extract(archive, scan, (done) => (seen = done));
    expect(got).toEqual(big);
    expect(seen).toBe(big.length);
    expect(new TextDecoder().decode(await extract(archive, entries[2]))).toBe("newmtl a\nKd 1 0 0\n");
  });

  it("says what is wrong with a file that isn't a zip", async () => {
    await expect(listZip(new Blob([text("not a zip")]), "x.zip")).rejects.toThrow("x.zip: not a .zip file");
  });
});
