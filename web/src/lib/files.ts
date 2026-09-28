// Sorting what the user dropped or picked: models, material libraries, and .zip files
// (expanded here, inflating only the entries that are used).
import { unzip, zip, type AsyncZippable } from "fflate";

const ext = (name: string) => name.slice(name.lastIndexOf(".") + 1).toLowerCase();
/** The file name without any folders (zip entries and dropped folders carry paths). */
export const baseName = (path: string) => path.split(/[\\/]/).pop() ?? path;
export const stem = (name: string) => baseName(name).replace(/\.[^.]+$/, "");

export interface Picked {
  objs: File[];
  mtls: File[];
  /** Files that aren't models or materials. */
  ignored: string[];
}

const USED = new Set(["obj", "mtl"]);

/** Expand .zip files and sort the rest by type. */
export async function gather(files: File[]): Promise<Picked> {
  const flat: File[] = [];
  const ignored: string[] = [];
  for (const f of files) {
    if (ext(f.name) === "zip") {
      const { used, skipped } = await unzipUsed(f);
      flat.push(...used);
      ignored.push(...skipped);
    } else if (USED.has(ext(f.name))) flat.push(f);
    else ignored.push(f.name);
  }
  return {
    objs: unique(flat.filter((f) => ext(f.name) === "obj")),
    mtls: flat.filter((f) => ext(f.name) === "mtl"),
    ignored,
  };
}

/** Distinct display names for files that share one (same name in different folders). */
function unique(files: File[]): File[] {
  const seen = new Map<string, number>();
  return files.map((f) => {
    const key = f.name.toLowerCase();
    const n = (seen.get(key) ?? 0) + 1;
    seen.set(key, n);
    return n === 1 ? f : new File([f], `${stem(f.name)} (${n}).${ext(f.name)}`, { lastModified: f.lastModified, type: f.type });
  });
}

async function unzipUsed(archive: File): Promise<{ used: File[]; skipped: string[] }> {
  const data = new Uint8Array(await archive.arrayBuffer());
  const skipped: string[] = [];
  const entries = await new Promise<Record<string, Uint8Array>>((resolve, reject) =>
    unzip(
      data,
      {
        filter: (f) => {
          const hidden = f.name.startsWith("__MACOSX/") || baseName(f.name).startsWith(".") || f.name.endsWith("/");
          const keep = !hidden && USED.has(ext(f.name));
          if (!keep && !hidden) skipped.push(baseName(f.name));
          return keep;
        },
      },
      (err, out) => (err ? reject(new Error(`${archive.name}: ${err.message}`)) : resolve(out)),
    ),
  );
  // Entries keep the archive's date, so repeated conversions are identical.
  const used = Object.entries(entries).map(([path, bytes]) => new File([bytes as Uint8Array<ArrayBuffer>], baseName(path), { lastModified: archive.lastModified }));
  return { used, skipped };
}

/** The material library an OBJ names (by file name, ignoring folders and case); with a
 *  single model and a single library, that one. */
export function pickMtl(mtllibs: string[], mtls: File[], onlyModel: boolean): File | null {
  const wanted = new Set(mtllibs.map((l) => baseName(l).toLowerCase()));
  return mtls.find((m) => wanted.has(m.name.toLowerCase())) ?? (onlyModel && mtls.length === 1 ? mtls[0] : null);
}

/** A .zip of finished files. Text (DXF) is compressed; DWG is already compressed. */
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
