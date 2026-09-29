// Reading .zip files without unpacking them on the page. The page only reads the
// archive's table of contents (a few KB at its end) to plan the drawing; the engine's
// worker then takes each needed file out of the archive with the browser's own
// decompressor. Unpacking a large scan on the page used to freeze it for seconds.
import { inflateSync } from "fflate";

/** Where a file is inside a .zip. */
export interface ZipEntry {
  name: string;
  /** Offset of the entry's local header. */
  offset: number;
  compressed: number;
  size: number;
  /** 0 stored, 8 deflate. */
  method: number;
}

const u16 = (d: DataView, at: number) => d.getUint16(at, true);
const u32 = (d: DataView, at: number) => d.getUint32(at, true);
const u64 = (d: DataView, at: number) => Number(d.getBigUint64(at, true));

async function bytes(file: Blob, start: number, end: number): Promise<DataView> {
  return new DataView(await file.slice(start, end).arrayBuffer());
}

/** Names are UTF-8 when the entry says so, else the old DOS code page (read as Latin-1, as
 *  the unzip library the app used before does). */
function decodeName(raw: Uint8Array, utf8: boolean): string {
  return utf8 ? new TextDecoder().decode(raw) : Array.from(raw, (b) => String.fromCharCode(b)).join("");
}

/** The files in an archive, from its central directory. Throws for what can't be read. */
export async function listZip(file: Blob, label: string): Promise<ZipEntry[]> {
  const fail = (why: string) => new Error(`${label}: ${why}`);
  // The end-of-directory record is in the last 64 KB + 22 bytes.
  const tailStart = Math.max(0, file.size - (65535 + 22));
  const tail = await bytes(file, tailStart, file.size);
  let eocd = -1;
  for (let i = tail.byteLength - 22; i >= 0; i--) {
    if (u32(tail, i) === 0x06054b50) {
      eocd = i;
      break;
    }
  }
  if (eocd < 0) throw fail("not a .zip file (no directory found)");
  let count = u16(tail, eocd + 10);
  let cdSize = u32(tail, eocd + 12);
  let cdOffset = u32(tail, eocd + 16);
  if (count === 0xffff || cdSize === 0xffffffff || cdOffset === 0xffffffff) {
    // ZIP64: the real values are in the ZIP64 end record, found through its locator.
    const loc = eocd - 20;
    if (loc < 0 || u32(tail, loc) !== 0x07064b50) throw fail("damaged ZIP64 directory");
    const at = u64(tail, loc + 8);
    const rec = await bytes(file, at, at + 56);
    if (u32(rec, 0) !== 0x06064b50) throw fail("damaged ZIP64 directory");
    count = u64(rec, 32);
    cdSize = u64(rec, 40);
    cdOffset = u64(rec, 48);
  }
  const cd = await bytes(file, cdOffset, cdOffset + cdSize);
  const entries: ZipEntry[] = [];
  let p = 0;
  for (let k = 0; k < count; k++) {
    if (p + 46 > cd.byteLength || u32(cd, p) !== 0x02014b50) throw fail("damaged directory");
    const flags = u16(cd, p + 8);
    const method = u16(cd, p + 10);
    let compressed = u32(cd, p + 20);
    let size = u32(cd, p + 24);
    const nameLen = u16(cd, p + 28);
    const extraLen = u16(cd, p + 30);
    const commentLen = u16(cd, p + 32);
    let offset = u32(cd, p + 42);
    const name = decodeName(new Uint8Array(cd.buffer, cd.byteOffset + p + 46, nameLen), (flags & 0x800) !== 0);
    // ZIP64 sizes and offset, in that order, for the fields that overflowed.
    let e = p + 46 + nameLen;
    const end = e + extraLen;
    while (e + 4 <= end) {
      const id = u16(cd, e);
      const len = u16(cd, e + 2);
      if (id === 0x0001) {
        let q = e + 4;
        if (size === 0xffffffff) (size = u64(cd, q)), (q += 8);
        if (compressed === 0xffffffff) (compressed = u64(cd, q)), (q += 8);
        if (offset === 0xffffffff) offset = u64(cd, q);
      }
      e += 4 + len;
    }
    p += 46 + nameLen + extraLen + commentLen;
    if (name.endsWith("/")) continue; // a folder
    if (flags & 1) throw fail(`${name} is encrypted`);
    if (method !== 0 && method !== 8) throw fail(`${name} uses an unsupported compression method (${method})`);
    entries.push({ name, offset, compressed, size, method });
  }
  return entries;
}

/** An entry's bytes (in the worker). `progress(done)` counts uncompressed bytes. */
export async function extract(archive: Blob, entry: ZipEntry, progress?: (done: number) => void): Promise<Uint8Array<ArrayBuffer>> {
  const head = await bytes(archive, entry.offset, entry.offset + 30);
  if (u32(head, 0) !== 0x04034b50) throw new Error(`${entry.name}: damaged entry in the .zip`);
  const start = entry.offset + 30 + u16(head, 26) + u16(head, 28);
  const data = archive.slice(start, start + entry.compressed);
  if (entry.method === 0) return new Uint8Array(await data.arrayBuffer());
  const out = new Uint8Array(entry.size);
  let native: DecompressionStream | null = null;
  try {
    native = new DecompressionStream("deflate-raw");
  } catch {
    // Browsers without it (or without raw deflate) inflate in JavaScript, still off the page.
  }
  if (!native) {
    const inflated = inflateSync(new Uint8Array(await data.arrayBuffer()), { out });
    if (inflated.length !== entry.size) throw new Error(`${entry.name}: damaged entry in the .zip`);
    return out;
  }
  const reader = data.stream().pipeThrough(native).getReader();
  let at = 0;
  for (;;) {
    const { done, value } = await reader.read();
    if (done) break;
    if (at + value.length > out.length) throw new Error(`${entry.name}: damaged entry in the .zip`);
    out.set(value, at);
    at += value.length;
    progress?.(at);
  }
  if (at !== out.length) throw new Error(`${entry.name}: damaged entry in the .zip`);
  return out;
}
