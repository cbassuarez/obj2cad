// Sorting what the user dropped or picked into drawings. A .zip or a folder is one
// drawing; so are loose files that include a point cloud or a texture. Several loose
// models on their own can be combined or converted separately.
import { zip, type AsyncZippable } from "fflate";
import { listZip, type ZipEntry } from "@/lib/zip";

// As the engine reads it (bundle.rs `kind`): no dot, no extension.
const ext = (name: string) => {
  const dot = name.lastIndexOf(".");
  return dot < 0 ? "" : name.slice(dot + 1).toLowerCase();
};
/** The file name without any folders (zip entries and dropped folders carry paths). */
export const baseName = (path: string) => path.split(/[\\/]/).pop() ?? path;
export const stem = (name: string) => baseName(name).replace(/\.[^.]+$/, "");

const MODEL = new Set(["obj"]);
const CLOUD = new Set(["xyz"]);
const IMAGE = new Set(["jpg", "jpeg", "png"]);
const USED = new Set(["obj", "xyz", "mtl", "jpg", "jpeg", "png"]);

/** One file of a drawing, and its path (folders kept, for display and matching). A file
 *  inside a .zip is `zip` in the archive `file`; the engine takes it out when it reads it. */
export interface Source {
  file: File;
  path: string;
  zip?: ZipEntry;
}

/** A source's size in bytes (uncompressed, for a file inside a .zip). */
export const sourceSize = (s: Source) => s.zip?.size ?? s.file.size;

/** The files of one drawing. `name` names it when it holds several models or clouds. */
export interface Job {
  name: string;
  sources: Source[];
}

export type Plan =
  | { kind: "none"; ignored: string[] }
  /** Only material libraries: they go to the open drawing. */
  | { kind: "materials"; mtls: Source[] }
  | { kind: "one"; job: Job }
  | { kind: "several"; jobs: Job[] }
  /** Several loose models and nothing else: one drawing, or one each. */
  | { kind: "choose"; combined: Job; separate: Job[] };

const hidden = (path: string) => path.split(/[\\/]/).some((p) => p.startsWith(".") || p === "__MACOSX");

/** The path a dropped or picked file came with (folders when a folder was dropped). */
function pathOf(f: File): string {
  const p = (f as File & { path?: string }).path || f.webkitRelativePath || f.name;
  return p.replace(/^\.?\//, "");
}

/** Material libraries an OBJ names (read from its first 256 KB, where `mtllib` lives). */
async function namedLibraries(f: File): Promise<string[]> {
  const head = await f.slice(0, 256 * 1024).text();
  const names: string[] = [];
  for (const m of head.matchAll(/^[ \t]*mtllib[ \t]+(.+?)[ \t]*$/gm)) {
    const rest = m[1];
    const toks = rest.split(/\s+/);
    names.push(...(toks.every((t) => t.toLowerCase().endsWith(".mtl")) ? toks : [rest]).map((n) => baseName(n).toLowerCase()));
  }
  return names;
}

/** Expand .zip files and folders, and decide how many drawings to make. */
export async function plan(files: File[]): Promise<Plan> {
  const ignored: string[] = [];
  const jobs: Job[] = [];
  const loose: Source[] = [];
  const folders = new Map<string, Source[]>();
  for (const f of files) {
    const path = pathOf(f);
    if (hidden(path)) continue;
    if (ext(f.name) === "zip") {
      const { used, skipped } = await unzipUsed(f);
      ignored.push(...skipped);
      if (used.length) jobs.push({ name: f.name, sources: used });
      continue;
    }
    if (!USED.has(ext(f.name))) {
      ignored.push(f.name);
      continue;
    }
    const top = path.includes("/") ? path.split("/")[0] : null;
    if (top) folders.set(top, [...(folders.get(top) ?? []), { file: f, path }]);
    else loose.push({ file: f, path });
  }
  for (const [name, sources] of folders) jobs.push({ name, sources });

  const models = loose.filter((s) => MODEL.has(ext(s.path)));
  const extras = loose.filter((s) => CLOUD.has(ext(s.path)) || IMAGE.has(ext(s.path)));
  const mtls = loose.filter((s) => ext(s.path) === "mtl");
  if (loose.length && (models.length || extras.length)) {
    if (models.length > 1 && !extras.length && !jobs.length) {
      const separate = await Promise.all(
        models.map(async (m) => {
          const named = await namedLibraries(m.file);
          const libs = mtls.filter((l) => named.includes(baseName(l.path).toLowerCase()));
          return { name: "", sources: [m, ...libs] };
        }),
      );
      return { kind: "choose", combined: { name: "", sources: loose }, separate };
    }
    jobs.push({ name: "", sources: loose });
  } else if (mtls.length && !jobs.length) {
    return { kind: "materials", mtls };
  }
  if (!jobs.length) return { kind: "none", ignored };
  return jobs.length === 1 ? { kind: "one", job: jobs[0] } : { kind: "several", jobs };
}

/** Point clouds this large are shown after the rest of their drawing. */
export const HEAVY = 16 << 20;

/** A drawing with a large point cloud next to lighter models: the models can be shown
 *  first, while the clouds load (`light` holds everything but them). `null` otherwise. */
export function heavySplit(job: Job): { light: Job; heavy: Source[] } | null {
  const heavy = job.sources.filter((s) => CLOUD.has(ext(s.path)) && sourceSize(s) >= HEAVY);
  const light = job.sources.filter((s) => !heavy.includes(s));
  const lightGeometry = light.some((s) => MODEL.has(ext(s.path)) || CLOUD.has(ext(s.path)));
  return heavy.length && lightGeometry ? { light: { name: jobName(job), sources: light }, heavy } : null;
}

/** A job's display name before the engine has read it: the zip/folder, or the first model. */
export function jobName(job: Job): string {
  if (job.name) return job.name;
  const geometry = job.sources.filter((s) => MODEL.has(ext(s.path)) || CLOUD.has(ext(s.path)));
  const first = [...geometry].sort((a, b) => baseName(a.path).toLowerCase().localeCompare(baseName(b.path).toLowerCase()))[0];
  return first ? baseName(first.path) : "drawing";
}

/** The files of a .zip the app can use, as sources into the archive (nothing is unpacked
 *  here: only the archive's directory is read). */
async function unzipUsed(archive: File): Promise<{ used: Source[]; skipped: string[] }> {
  const skipped: string[] = [];
  const used: Source[] = [];
  for (const entry of await listZip(archive, archive.name)) {
    if (hidden(entry.name)) continue;
    if (USED.has(ext(entry.name))) used.push({ file: archive, path: entry.name, zip: entry });
    else skipped.push(baseName(entry.name));
  }
  return { used, skipped };
}

export async function zipFiles(files: { name: string; blob: Blob }[]): Promise<Blob> {
  const input: AsyncZippable = {};
  for (const f of files) input[f.name] = [new Uint8Array(await f.blob.arrayBuffer()), { level: f.name.toLowerCase().endsWith(".dwg") ? 0 : 6 }];
  const out = await new Promise<Uint8Array>((resolve, reject) => zip(input, (err, data) => (err ? reject(err) : resolve(data))));
  return new Blob([out as Uint8Array<ArrayBuffer>], { type: "application/zip" });
}

/** Save a file: the browser's save dialog when asked and available, else a download. */
export async function saveFile(blob: Blob, name: string, pick = false): Promise<string | null> {
  const w = window as Window & { showSaveFilePicker?: (o: object) => Promise<FileSystemFileHandle & { createWritable: () => Promise<{ write: (b: Blob) => Promise<void>; close: () => Promise<void> }> }> };
  if (pick && w.showSaveFilePicker) {
    try {
      const handle = await w.showSaveFilePicker({ suggestedName: name });
      const out = await handle.createWritable();
      await out.write(blob);
      await out.close();
      return handle.name;
    } catch (e) {
      if (e instanceof DOMException && e.name === "AbortError") return null; // cancelled
      throw e;
    }
  }
  const url = URL.createObjectURL(blob);
  const a = Object.assign(document.createElement("a"), { href: url, download: name });
  a.click();
  setTimeout(() => URL.revokeObjectURL(url), 30_000);
  return name;
}

export const canPickSaveLocation = () => typeof window !== "undefined" && "showSaveFilePicker" in window;
