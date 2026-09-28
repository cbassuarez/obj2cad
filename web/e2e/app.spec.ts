// The web app end to end: real files in, real downloads out. Every download must be
// byte-identical to what the command-line tool writes for the same file, so everything
// tests/harness/parity.py verifies for the CLI holds for the web app too.
import { expect, test, type Page } from "@playwright/test";
import { execFileSync } from "node:child_process";
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

async function open(page: Page, files: string[], format: Format = "dxf") {
  await page.addInitScript((f) => localStorage.setItem("obj2cad.prefs.v1", JSON.stringify({ format: f })), format);
  await page.goto("/");
  await input(page).setInputFiles(files);
}

const downloadButton = (page: Page) => page.getByRole("button", { name: /^Download (DXF|DWG)/ });

async function download(page: Page): Promise<{ name: string; bytes: Buffer }> {
  await expect(downloadButton(page)).toBeEnabled();
  const [d] = await Promise.all([page.waitForEvent("download"), downloadButton(page).click()]);
  return { name: d.suggestedFilename(), bytes: readFileSync((await d.path())!) };
}

function cliOutput(inputs: string | string[], format: Format): Buffer {
  const out = path.join(mkdtempSync(path.join(tmpdir(), "obj2cad-e2e-")), `out.${format === "dwg" ? "dwg" : "dxf"}`);
  execFileSync(cli, ["convert", ...[inputs].flat(), "-o", out, "--format", format, "--quiet"]);
  return readFileSync(out);
}

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
  await expect(page.getByText("Texture colors, approximate")).toBeVisible();
  await expect(page.getByText("Not used: unused.png")).toBeVisible();
  await open(page, [zipFolder(path.join(fixtures, "bundle", "two_models"))]);
  await expect(page.getByText("Missing: missing.mtl")).toBeVisible();
});

test("several loose models: combine into one drawing", async ({ page }) => {
  const files = ["a.obj", "b.obj", "a.mtl", "b.mtl"].map((f) => path.join(fixtures, "bundle", "two_models", f));
  await open(page, files);
  await page.getByRole("button", { name: "Combine into one drawing" }).click();
  const got = await download(page);
  expect(got.bytes.equals(cliOutput(files, "dxf")), "web and CLI outputs differ").toBe(true);
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
  await expect(page.getByText(/Size in CAD 10 × 10 × 10/)).toBeVisible();
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
  await expect(page.getByRole("button", { name: "Open" })).toBeVisible();
  await page.goBack();
  await expect(page.getByRole("button", { name: "Choose files…" })).toBeVisible();
  await page.goForward();
  await expect(downloadButton(page)).toBeEnabled();
});

test("changing units relabels the drawing without moving geometry", async ({ page }) => {
  await open(page, [path.join(fixtures, "edge", "names_layers.obj")]);
  await expect(downloadButton(page)).toBeEnabled();
  const hash = async () => {
    const details = page.getByRole("button", { name: "Technical details" });
    if ((await details.getAttribute("aria-expanded")) !== "true") await details.click();
    return page.locator("code[title]").first().getAttribute("title");
  };
  const before = await hash();
  await page.getByRole("status").getByRole("button", { name: /Millimeters|Meters|None/ }).click();
  await page.getByRole("menuitem", { name: "Feet" }).click();
  await expect(page.getByText(/Size in CAD .* ft$/)).toBeVisible();
  await expect(downloadButton(page)).toBeEnabled();
  expect(await hash()).toBe(before);
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
