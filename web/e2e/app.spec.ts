// The web app end to end: real files in, real downloads out. Every download must be
// byte-identical to what the command-line tool writes for the same file, so everything
// tests/harness/parity.py verifies for the CLI holds for the web app too.
import { expect, test, type Page } from "@playwright/test";
import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdtempSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { zipSync } from "fflate";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
const fixtures = path.join(root, "tests", "fixtures");
const cli = process.env.OBJ2CAD_BIN ?? path.join(root, "target", "release", process.platform === "win32" ? "obj2cad.exe" : "obj2cad");
const objs = (dir: string) => readdirSync(path.join(fixtures, dir)).filter((f) => /\.(obj|xyz)$/.test(f));
const input = (page: Page) => page.locator('input[accept^=".obj"]');

type Format = "dxf" | "dxf-binary" | "dwg";

async function open(page: Page, files: string[], format: Format = "dxf", curves = false) {
  await page.addInitScript(([f, c]) => localStorage.setItem("obj2cad.prefs.v1", JSON.stringify({ format: f, curves: c })), [format, curves] as const);
  await page.goto("/");
  await input(page).setInputFiles(files);
}

const downloadButton = (page: Page) => page.getByRole("button", { name: /^Download (DXF|DWG)/ });
const card = (page: Page) => page.getByRole("complementary", { name: "Result" });

async function download(page: Page): Promise<{ name: string; bytes: Buffer }> {
  await expect(downloadButton(page)).toBeEnabled();
  const [d] = await Promise.all([page.waitForEvent("download"), downloadButton(page).click()]);
  return { name: d.suggestedFilename(), bytes: readFileSync((await d.path())!) };
}

function cliRun(inputs: string | string[], format: Format, extra: string[] = []): { bytes: Buffer; report: Record<string, unknown> } {
  const dir = mkdtempSync(path.join(tmpdir(), "obj2cad-e2e-"));
  const out = path.join(dir, `out.${format === "dwg" ? "dwg" : "dxf"}`);
  const report = path.join(dir, "out.report.json");
  execFileSync(cli, ["convert", ...[inputs].flat(), "-o", out, "--report", report, "--format", format, "--quiet", ...extra]);
  return { bytes: readFileSync(out), report: JSON.parse(readFileSync(report, "utf8")) };
}
const cliOutput = (inputs: string | string[], format: Format, extra: string[] = []) => cliRun(inputs, format, extra).bytes;

/** Save the report from the result card's "⋯" menu. */
async function saveReport(page: Page): Promise<{ name: string; report: Record<string, unknown> }> {
  await card(page).getByRole("button", { name: "More" }).click();
  const [d] = await Promise.all([page.waitForEvent("download"), page.getByRole("menuitem", { name: /conversion report/ }).click()]);
  return { name: d.suggestedFilename(), report: JSON.parse(readFileSync((await d.path())!, "utf8")) };
}

/** A report with the left-out layers in one order (the app lists them in layer order, the CLI in flag order). */
const normalized = (r: Record<string, unknown>) => {
  const c = structuredClone(r) as { options: { exclude_layers: string[] } };
  c.options.exclude_layers.sort();
  return c;
};
const sha256 = (b: Buffer) => createHash("sha256").update(b).digest("hex");

/** A bundle folder as a .zip on disk (its date is the CLI's and the browser's). */
function zipFolder(dir: string): string {
  const files: Record<string, Uint8Array> = {};
  const walk = (d: string) => {
    for (const e of readdirSync(d, { withFileTypes: true })) {
      const p = path.join(d, e.name);
      if (e.isDirectory()) walk(p);
      else files[path.relative(dir, p).split(path.sep).join("/")] = new Uint8Array(readFileSync(p));
    }
  };
  walk(dir);
  const out = path.join(mkdtempSync(path.join(tmpdir(), "obj2cad-e2e-")), `${path.basename(dir)}.zip`);
  writeFileSync(out, zipSync(files));
  return out;
}

for (const format of ["dxf", "dxf-binary", "dwg"] as const) {
  for (const name of objs("edge")) {
    test(`${name} (${format}) matches the command-line tool`, async ({ page }) => {
      const obj = path.join(fixtures, "edge", name);
      const mtl = obj.replace(/\.obj$/, ".mtl");
      await open(page, mtl !== obj && readdirSync(path.dirname(obj)).includes(path.basename(mtl)) ? [obj, mtl] : [obj], format);
      const got = await download(page);
      expect(got.name).toBe(name.replace(/\.(obj|xyz)$/, format === "dwg" ? ".dwg" : ".dxf"));
      expect(got.bytes.equals(cliOutput(obj, format)), "web and CLI outputs differ").toBe(true);
      await expect(page.getByText(`Downloaded ${got.name}`)).toBeVisible();
    });
  }
}

for (const format of ["dxf", "dwg"] as const) {
  for (const bundle of readdirSync(path.join(fixtures, "bundle"))) {
    test(`bundle ${bundle}.zip (${format}) matches the command-line tool`, async ({ page }) => {
      const zip = zipFolder(path.join(fixtures, "bundle", bundle));
      await open(page, [zip], format);
      const got = await download(page);
      expect(got.name).toBe(`${bundle}.${format}`);
      expect(got.bytes.equals(cliOutput(zip, format)), "web and CLI outputs differ").toBe(true);
    });
  }
}

test("opening a large file shows each step on the rail, and can be cancelled; the next file still converts", async ({ page }) => {
  const big = path.join(mkdtempSync(path.join(tmpdir(), "obj2cad-e2e-")), "big.obj");
  execFileSync(cli, ["synth", "1400", big]); // about 3.9 million faces
  await open(page, [big]);
  // While the model is read the rail counts its vertices; Cancel is clicked in that same
  // moment (the page is busy building the scene, and a round trip could miss it).
  const stations = await page.waitForFunction(
    () => {
      const rail = document.querySelector('[role="status"][aria-label="Opening big.obj"]');
      if (!rail || !/Model: active, [\d,]+ vertices/.test(rail.textContent ?? "")) return null;
      rail.querySelector("button")!.click();
      return [...rail.querySelectorAll("li .sr-only")].map((e) => (e.textContent ?? "").split(":")[0]);
    },
    null,
    { timeout: 30_000 },
  );
  expect(await stations.jsonValue()).toEqual(["Read", "Model", "View", "Fingerprint", "Write DXF"]);
  await expect(page.getByRole("heading", { name: "OBJ to DWG / DXF" })).toBeVisible();
  const obj = path.join(fixtures, "edge", "cube_materials.obj");
  const mtl = path.join(fixtures, "edge", "cube_materials.mtl");
  await input(page).setInputFiles([obj, mtl]);
  const got = await download(page);
  expect(got.bytes.equals(cliOutput(obj, "dxf")), "web and CLI outputs differ").toBe(true);
});

test("opening another file doesn't wait for the one still loading", async ({ page }) => {
  const big = path.join(mkdtempSync(path.join(tmpdir(), "obj2cad-e2e-")), "big.obj");
  execFileSync(cli, ["synth", "900", big]);
  await open(page, [big]);
  await expect(page.getByRole("status", { name: "Opening big.obj" })).toBeVisible();
  const obj = path.join(fixtures, "edge", "cube_materials.obj");
  const mtl = path.join(fixtures, "edge", "cube_materials.mtl");
  await input(page).setInputFiles([obj, mtl]);
  const got = await download(page);
  expect(got.name).toBe("cube_materials.dxf");
  expect(got.bytes.equals(cliOutput(obj, "dxf")), "web and CLI outputs differ").toBe(true);
  await expect(page.getByText("big.obj")).toHaveCount(0);
});

test("a large scan loads after the model it comes with; the file is written once, from both", async ({ page }) => {
  const dir = mkdtempSync(path.join(tmpdir(), "obj2cad-e2e-"));
  const obj = path.join(dir, "building.obj");
  const scan = path.join(dir, "scan.xyz");
  writeFileSync(obj, "o Building\nv 0 0 0\nv 20 0 0\nv 20 12 0\nv 0 12 0\nf 1 2 3 4\n");
  // About 20 MB of colored points.
  const rows: string[] = [];
  for (let i = 0; i < 750_000; i++) rows.push(`${(i % 1000) - 500}.123 ${Math.floor(i / 1000)}.456 0.789 ${i % 256} 120 ${255 - (i % 256)}\n`);
  writeFileSync(scan, rows.join(""));
  await open(page, [obj, scan]);
  // The model is shown while the scan is still loading, and the layers say so; Download
  // waits, and nothing is called an exact copy before the file is written. (Read in one
  // moment: the scan may finish loading between two checks.)
  const loading = await page.waitForFunction(() => {
    if (!document.querySelector('[aria-label="Loading scan.xyz"]')) return null;
    const card = document.querySelector('aside[aria-label="Result"]');
    const download = [...document.querySelectorAll("button")].find((b) => /^Download (DXF|DWG)/.test(b.textContent ?? ""));
    return { card: card?.textContent ?? "", disabled: !!download?.disabled };
  });
  const seen = await loading.jsonValue();
  expect(seen.card).toContain("1 face on 1 layer");
  expect(seen.card).not.toContain("Exact copy");
  expect(seen.disabled).toBe(true);
  await expect(page.getByRole("status", { name: "Loading scan.xyz" })).toHaveCount(0, { timeout: 60_000 });
  await expect(downloadButton(page)).toBeEnabled({ timeout: 60_000 });
  await expect(card(page).getByText("750,001 shapes on 2 layers")).toBeVisible();
  const got = await download(page);
  expect(got.bytes.equals(cliOutput([obj, scan], "dxf")), "web and CLI outputs differ").toBe(true);
});

test("loose files with a point cloud make one drawing, like the command-line tool", async ({ page }) => {
  const site = path.join(fixtures, "bundle", "site");
  const files = ["site.obj", "Site.MTL", "scan.xyz", "ground.png"].map((f) => path.join(site, f));
  await open(page, files);
  const got = await download(page);
  expect(got.name).toBe("scan.dxf");
  expect(got.bytes.equals(cliOutput(files, "dxf")), "web and CLI outputs differ").toBe(true);
  await expect(page.getByText("4 files")).toBeVisible();
  await expect(page.getByText("Missing: Facade.JPG")).toBeVisible();
});

test("a bundle lists what's missing, unused and approximate", async ({ page }) => {
  await open(page, [zipFolder(path.join(fixtures, "bundle", "site"))]);
  await expect(downloadButton(page)).toBeEnabled();
  await expect(page.getByText("Colors from textures, averaged per face")).toBeVisible();
  await expect(page.getByText("Not used: unused.png")).toBeVisible();
  await open(page, [zipFolder(path.join(fixtures, "bundle", "two_models"))]);
  await expect(page.getByText("Missing: missing.mtl")).toBeVisible();
  await open(page, [zipFolder(path.join(fixtures, "bundle", "textured"))]);
  await expect(page.getByText("Colors from textures and vertex colors, averaged per face")).toBeVisible();
});

test("several loose models: combine into one drawing", async ({ page }) => {
  const files = ["a.obj", "b.obj", "a.mtl", "b.mtl"].map((f) => path.join(fixtures, "bundle", "two_models", f));
  await open(page, files);
  await page.getByRole("button", { name: "Combine into one drawing" }).click();
  const got = await download(page);
  expect(got.bytes.equals(cliOutput(files, "dxf")), "web and CLI outputs differ").toBe(true);
});

for (const format of ["dxf", "dwg"] as const) {
  for (const name of objs("curves")) {
    test(`${name} with curved surfaces (${format}) matches the command-line tool`, async ({ page }) => {
      const obj = path.join(fixtures, "curves", name);
      await open(page, [obj], format, true);
      const got = await download(page);
      expect(got.bytes.equals(cliOutput(obj, format, ["--curves"])), "web and CLI outputs differ").toBe(true);
    });
  }
}

test("curved surfaces are listed and get their own layer", async ({ page }) => {
  await open(page, [path.join(fixtures, "curves", "capsule.obj")], "dxf", true);
  await expect(downloadButton(page)).toBeEnabled();
  await expect(page.getByText("Curved surfaces: 1 cylinder, 2 spheres")).toBeVisible();
  await expect(page.getByRole("checkbox", { name: "Curves in the drawing" })).toBeVisible();
  // Off by default, from the Format menu.
  await open(page, [path.join(fixtures, "curves", "capsule.obj")]);
  await expect(downloadButton(page)).toBeEnabled();
  await expect(page.getByText(/^Curved surfaces:/)).toHaveCount(0);
  await card(page).getByRole("button", { name: /^DXF/ }).click();
  await page.getByRole("menuitem", { name: "Curved surfaces" }).click();
  await expect(page.getByText("Curved surfaces: 1 cylinder, 2 spheres")).toBeVisible();
});

for (const name of objs("invalid")) {
  test(`${name} is rejected with a line number`, async ({ page }) => {
    await open(page, [path.join(fixtures, "invalid", name)]);
    const alert = page.getByRole("alert");
    await expect(alert).toBeVisible();
    await expect(alert.getByText(name)).toBeVisible();
    if (name !== "utf16.obj" && name !== "ambiguous_columns.xyz") await expect(alert.getByText(/^Line \d/).first()).toBeVisible();
    await expect(page.getByRole("button", { name: "Choose a file…" })).toBeVisible();
  });
}

test("a byte-order mark is not content", async ({ page }) => {
  await open(page, [path.join(fixtures, "edge", "bom.obj")]);
  await expect(page.getByText("Exact copy")).toBeVisible();
  await expect(card(page).getByText(/^10 × 10 × 10/)).toBeVisible();
});

test("free-form surfaces are never called exact", async ({ page }) => {
  await open(page, [path.join(fixtures, "edge", "freeform_mixed.obj")]);
  await expect(page.getByText("Converted, with parts left out")).toBeVisible();
  await expect(page.getByText(/^Left out: /)).toBeVisible();
});

test("a point cloud converts to points", async ({ page }) => {
  await open(page, [path.join(fixtures, "edge", "point_cloud.obj")]);
  await expect(downloadButton(page)).toBeEnabled();
  await expect(page.getByText("Exact copy")).toBeVisible();
});

test("Back returns to the start, and the Open button stays available", async ({ page }) => {
  await open(page, [path.join(fixtures, "edge", "names_layers.obj")]);
  await expect(downloadButton(page)).toBeEnabled();
  await expect(page.getByRole("button", { name: "Open files" })).toBeVisible();
  await page.goBack();
  await expect(page.getByRole("button", { name: "Choose files…" })).toBeVisible();
  await page.goForward();
  await expect(downloadButton(page)).toBeEnabled();
});

test("changing the file's unit relabels the drawing without moving geometry", async ({ page }) => {
  await open(page, [path.join(fixtures, "edge", "names_layers.obj")]);
  await expect(downloadButton(page)).toBeEnabled();
  const hash = async () => {
    await card(page).getByRole("button", { name: "More" }).click();
    await page.getByRole("menuitem", { name: /Technical details/ }).click();
    const h = await page.getByRole("dialog").locator("code[title]").first().getAttribute("title");
    await page.keyboard.press("Escape");
    await expect(page.getByRole("dialog")).toHaveCount(0);
    return h;
  };
  const before = await hash();
  await expect(card(page).getByText("File in meters · assumed")).toBeVisible();
  await card(page).getByRole("button", { name: "Change the file's unit" }).click();
  await page.getByRole("menuitem", { name: "Feet" }).click();
  await expect(card(page).getByText(/ ft$/)).toBeVisible();
  await expect(downloadButton(page)).toBeEnabled();
  expect(await hash()).toBe(before);
  const got = await download(page);
  expect(got.bytes.equals(cliOutput(path.join(fixtures, "edge", "names_layers.obj"), "dxf", ["--units", "ft"])), "web and CLI outputs differ").toBe(true);
});

test("Show in changes only what the app displays, never the file", async ({ page }) => {
  const obj = path.join(fixtures, "curves", "capsule.obj");
  await open(page, [obj]);
  const before = await download(page);
  // No exporter named: meters, assumed.
  await expect(card(page).getByText("File in meters · assumed")).toBeVisible();
  const size = card(page).getByText(/ m$/);
  await expect(size).toHaveText("10 × 30 × 10 m");
  await card(page).getByRole("button", { name: /^Meters/ }).click();
  await page.getByRole("menuitem", { name: "Millimeters" }).click();
  await expect(card(page).getByText("10,000 × 30,000 × 10,000 mm")).toBeVisible();
  await expect(page.locator(".dim-label").first()).toHaveText(/ mm$/);
  await expect(card(page).getByText("File in meters · assumed")).toBeVisible();
  const after = await download(page);
  expect(after.bytes.equals(before.bytes), "the display unit must not change the file").toBe(true);
  // Remembered: saved with the preferences, and kept for the next file.
  expect(await page.evaluate(() => JSON.parse(localStorage.getItem("obj2cad.prefs.v1") ?? "{}").showIn)).toBe("millimeters");
  await input(page).setInputFiles([path.join(fixtures, "edge", "names_layers.obj")]);
  await expect(card(page).getByText("names_layers.obj")).toHaveCount(0);
  await expect(downloadButton(page)).toBeEnabled();
  await expect(card(page).getByRole("button", { name: /^Millimeters/ })).toBeVisible();
});

test("a chosen folder is one drawing, like the command-line tool", async ({ page }) => {
  const dir = path.join(fixtures, "bundle", "site");
  await page.goto("/");
  await page.locator('input[aria-label="Choose a folder"]').setInputFiles(dir);
  const got = await download(page);
  expect(got.name).toBe("site.dxf");
  expect(got.bytes.equals(cliOutput(dir, "dxf")), "web and CLI outputs differ").toBe(true);
});

test("a bundle lists layers under their file; a file's checkbox leaves out all of its layers", async ({ page }) => {
  const dir = path.join(fixtures, "bundle", "same_names");
  await page.goto("/");
  await page.locator('input[aria-label="Choose a folder"]').setInputFiles(dir);
  await expect(downloadButton(page)).toBeEnabled();
  const layers = page.getByRole("region", { name: "Layers" });
  await expect(layers.getByRole("checkbox", { name: "east.obj in the drawing" })).toBeVisible();
  await expect(layers.getByRole("checkbox", { name: "Chair (west) in the drawing" })).toBeVisible();
  await layers.getByRole("checkbox", { name: "west.obj in the drawing" }).click();
  await expect(downloadButton(page)).toHaveText(/2 of 3 layers/);
  const got = await download(page);
  expect(got.bytes.equals(cliOutput(dir, "dxf", ["--exclude-layer", "Chair (west)"])), "web and CLI outputs differ").toBe(true);
});

test("several loose models: convert separately, then Download all", async ({ page }) => {
  await open(page, ["negative_indices.obj", "ngons_nonplanar.obj", "precision.obj"].map((f) => path.join(fixtures, "edge", f)));
  await page.getByRole("button", { name: "Convert separately" }).click();
  const all = page.getByRole("button", { name: /Download all/ });
  await expect(all).toBeEnabled({ timeout: 30_000 });
  const [d] = await Promise.all([page.waitForEvent("download"), all.click()]);
  expect(d.suggestedFilename()).toMatch(/\.zip$/);
});

test("a .zip with a model and its materials opens as one file", async ({ page }) => {
  const read = (f: string) => new Uint8Array(readFileSync(path.join(fixtures, "edge", f)));
  const zip = zipSync({ "export/cube_materials.obj": read("cube_materials.obj"), "export/cube_materials.mtl": read("cube_materials.mtl"), "__MACOSX/._x": new Uint8Array(1) });
  await page.goto("/");
  await input(page).setInputFiles({ name: "export.zip", mimeType: "application/zip", buffer: Buffer.from(zip) });
  await expect(downloadButton(page)).toBeEnabled();
  await expect(page.getByText("cube_materials.obj")).toBeVisible();
  await expect(page.getByText("2 files")).toBeVisible();
});

test("the saved report describes the download: the command-line report, with its SHA-256", async ({ page }) => {
  const obj = path.join(fixtures, "edge", "names_layers.obj");
  await open(page, [obj]);
  const got = await download(page);
  const saved = await saveReport(page);
  const ref = cliRun(obj, "dxf");
  expect(saved.name).toBe("names_layers.report.json");
  expect((saved.report.output as { sha256: string }).sha256).toBe(sha256(got.bytes));
  expect(saved.report).toEqual(ref.report);
  await expect(page.getByText(`Saved ${saved.name}`)).toBeVisible();
});

test("unticked layers are left out: download, report and Ctrl+S match --exclude-layer", async ({ page }) => {
  const obj = path.join(fixtures, "edge", "names_layers.obj");
  await open(page, [obj]);
  await expect(downloadButton(page)).toBeEnabled();
  const boxes = page.getByRole("checkbox", { name: / in the drawing$/ });
  const layers = (await boxes.evaluateAll((bs) => bs.map((b) => b.getAttribute("aria-label")!.replace(/ in the drawing$/, "")))).slice(0, 2);
  for (const l of layers) await page.getByRole("checkbox", { name: `${l} in the drawing` }).click();
  await expect(downloadButton(page)).toHaveText(/of \d+ layers/);
  const got = await download(page);
  expect(got.name).toBe("names_layers (visible layers).dxf");
  const ref = cliRun(obj, "dxf", layers.flatMap((l) => ["--exclude-layer", l]));
  expect(got.bytes.equals(ref.bytes), "web and CLI outputs differ").toBe(true);
  const [k] = await Promise.all([page.waitForEvent("download"), page.keyboard.press("Control+s")]);
  expect(readFileSync((await k.path())!).equals(got.bytes), "Ctrl+S saves what the button saves").toBe(true);
  const saved = await saveReport(page);
  expect(saved.name).toBe("names_layers (visible layers).report.json");
  expect((saved.report.output as { sha256: string }).sha256).toBe(sha256(got.bytes));
  expect(normalized(saved.report)).toEqual(normalized(ref.report));
  // Every layer back: the whole drawing again.
  await page.getByRole("button", { name: "Include all" }).click();
  expect((await download(page)).bytes.equals(cliOutput(obj, "dxf"))).toBe(true);
});
