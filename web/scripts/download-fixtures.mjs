// Download every test model through the real app, the way a person does, so that
// tests/harness/web_parity.py can check the files the browser saves (not just the engine):
// byte-identical to the command-line tool, and correct by the harness's independent reader.
//
//   npm run build && node scripts/download-fixtures.mjs <out-dir> <fixture-dir>...
//
// Writes <out>/<format>/<model>.<ext> and .report.json for each format (a file the app
// rejects is recorded instead), a visible-layers download, a batch .zip, and manifest.json.
import { mkdirSync, readdirSync, existsSync, writeFileSync, rmSync, statSync } from "node:fs";
import { join, basename, resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { preview } from "vite";
import { chromium } from "playwright";

const [out, ...dirs] = process.argv.slice(2).map((p) => resolve(p));
if (!out || !dirs.length) {
  console.error("usage: node scripts/download-fixtures.mjs <out-dir> <fixture-dir>...");
  process.exit(2);
}
const objsIn = (dir) =>
  readdirSync(dir, { recursive: true })
    .map((f) => join(dir, f))
    .filter((f) => f.endsWith(".obj") && statSync(f).isFile())
    .sort();
const objs = dirs.flatMap(objsIn);
const withMtl = (obj) => [obj, ...(existsSync(obj.replace(/\.obj$/, ".mtl")) ? [obj.replace(/\.obj$/, ".mtl")] : [])];

const server = await preview({ root: resolve(dirname(fileURLToPath(import.meta.url)), ".."), preview: { port: 4173, strictPort: false } });
const url = server.resolvedUrls.local[0];
const browser = await chromium.launch();
const manifest = { formats: {}, visible: null, batch: null };
rmSync(out, { recursive: true, force: true });

const session = async (format) => {
  const ctx = await browser.newContext({ acceptDownloads: true, viewport: { width: 1440, height: 900 } });
  await ctx.addInitScript((f) => localStorage.setItem("obj2cad.prefs.v1", JSON.stringify({ format: f, layerMode: "objects", houseUnits: null, includeName: true })), format);
  return ctx;
};
// The download button is ready: enabled and not converting.
const ready = (page) =>
  page.waitForFunction(
    () => {
      const b = [...document.querySelectorAll("button")].find((x) => x.innerText.startsWith("Download "));
      return b && !b.disabled && !b.innerText.includes("…");
    },
    null,
    { timeout: 180_000 },
  );
const open = async (ctx, files) => {
  const page = await ctx.newPage();
  await page.goto(url);
  await page.setInputFiles('input[accept=".obj,.mtl,.zip"]', files);
  const outcome = await Promise.race([
    page.waitForSelector('aside[aria-label="Result"]', { timeout: 180_000 }).then(() => "ok"),
    page.waitForSelector('main [role="alert"]', { timeout: 180_000 }).then(() => "rejected"),
  ]);
  if (outcome === "ok") await ready(page);
  return { page, outcome };
};
const saveVia = async (page, dir, trigger) => {
  const [d] = await Promise.all([page.waitForEvent("download"), trigger()]);
  await d.saveAs(join(dir, d.suggestedFilename()));
  return d.suggestedFilename();
};
const saveDrawingAndReport = async (page, dir) => {
  const drawing = await saveVia(page, dir, () => page.getByRole("button", { name: /^Download / }).click());
  const report = await saveVia(page, dir, async () => {
    await page.getByRole("button", { name: "More" }).click();
    await page.getByRole("menuitem", { name: /conversion report/ }).click();
  });
  return { drawing, report };
};

for (const format of ["dxf", "dxf-binary", "dwg"]) {
  const dir = join(out, format);
  mkdirSync(dir, { recursive: true });
  const ctx = await session(format);
  manifest.formats[format] = {};
  for (const obj of objs) {
    const { page, outcome } = await open(ctx, withMtl(obj));
    manifest.formats[format][obj] =
      outcome === "ok" ? await saveDrawingAndReport(page, dir) : { rejected: (await page.locator('main [role="alert"]').innerText()).split("\n")[0] };
    console.log(`${format}\t${basename(obj)}\t${outcome}`);
    await page.close();
  }
  await ctx.close();
}

// Visible layers: untick one layer of a multi-layer model; Download and the report must then
// describe the drawing without it.
const layered = objs.find((o) => basename(o) === "names_layers.obj");
if (layered) {
  const dir = join(out, "visible");
  mkdirSync(dir, { recursive: true });
  const ctx = await session("dxf");
  const { page } = await open(ctx, withMtl(layered));
  const box = page.getByRole("checkbox", { name: / in the drawing$/ }).first();
  const hidden = (await box.getAttribute("aria-label")).replace(/ in the drawing$/, "");
  await box.click(); // untick: leave it out of the drawing
  await ready(page);
  manifest.visible = { obj: layered, hidden: [hidden], ...(await saveDrawingAndReport(page, dir)) };
  console.log(`visible\t${basename(layered)} without ${hidden}`);
  await ctx.close();
}

// Batch: every valid model at once, then "Download all (.zip)".
{
  const ctx = await session("dxf");
  const page = await ctx.newPage();
  await page.goto(url);
  const valid = Object.entries(manifest.formats.dxf).filter(([, v]) => !v.rejected).map(([o]) => o);
  await page.setInputFiles('input[accept=".obj,.mtl,.zip"]', valid.flatMap(withMtl));
  await page.waitForFunction(
    () => {
      const b = [...document.querySelectorAll("button")].find((x) => x.innerText.includes("Download all"));
      return b && !b.disabled;
    },
    null,
    { timeout: 300_000 },
  );
  const zip = await saveVia(page, out, () => page.getByRole("button", { name: /Download all/ }).click());
  manifest.batch = { objs: valid, zip };
  console.log(`batch\t${valid.length} files\t${zip}`);
  await ctx.close();
}

writeFileSync(join(out, "manifest.json"), JSON.stringify(manifest, null, 2));
await browser.close();
await server.close();
