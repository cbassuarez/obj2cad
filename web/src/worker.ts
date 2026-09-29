/// <reference lib="webworker" />
// Runs the Rust engine off the main thread. The worker keeps the parsed file, so a
// settings change only re-converts. Two engine modules: the small default one, and one
// with the DWG writer that is loaded the first time DWG is chosen.
import initCore, * as core from "./wasm/obj2cad_wasm.js";
import type { EngineSettings } from "@/lib/settings";
import { geometryKey } from "@/lib/settings";

type Module = typeof core;
type Kind = "core" | "dwg";

/** What the engine is doing. `read` and `parse` count bytes of `total`; `write` counts
 *  the bytes written so far (the total isn't known in advance, so `total` is 0); the
 *  others are steps without a count. `engine`: waiting for an engine module to load. */
export interface Progress {
  stage: "engine" | "read" | "parse" | "hash" | "curves" | "write" | "preview";
  done: number;
  total: number;
}

export interface ParseFailure {
  /** The file of the drawing that couldn't be read. */
  file: string;
  kind: string;
  line: number;
  message: string;
  issues: { line: number; kind: string; message: string }[];
  truncated: boolean;
}

/** `parse`: the file can't be read without guessing. `crash`: the engine stopped (the
 *  worker must be replaced). `engine`: the engine couldn't start. */
export type Failure =
  | { kind: "parse"; parse: ParseFailure }
  | { kind: "crash"; message: string }
  | { kind: "engine"; message: string }
  | { kind: "read"; message: string }
  | { kind: "other"; message: string };

export interface PreviewBuffers {
  positions: Float32Array;
  colors: Uint8Array;
  indices: Uint32Array;
  edges: Uint32Array;
  groups: Uint32Array;
  lines: Float32Array;
  lineColors: Uint8Array;
  lineGroups: Uint32Array;
  points: Float32Array;
  pointColors: Uint8Array;
  pointGroups: Uint32Array;
  origin: number[];
  available: boolean;
}

export interface Converted {
  /** The file, in the pieces the writer produced (transferred, not copied). */
  parts: ArrayBuffer[];
  report: string;
  decisions: string;
  timings: Record<string, number>;
  preview: PreviewBuffers | null;
  ms: number;
}

/** One file of a drawing (see `Source` in lib/files.ts). */
export interface SourceFile {
  file: File;
  path: string;
}

export type Request =
  | { id: number; type: "open"; sources: SourceFile[]; name: string; dwg: boolean }
  | { id: number; type: "convert"; settings: EngineSettings; preview: boolean; early: boolean }
  | { id: number; type: "warm"; dwg: boolean };

/** The model, ready to show before its file is written (a provisional report: no parity
 *  hash, no output size). */
export interface Early {
  preview: PreviewBuffers;
  report: string;
  decisions: string;
}

export type Reply =
  | { id: number; type: "progress"; progress: Progress }
  | { id: number; type: "early"; early: Early }
  | { id: number; type: "ok"; result: unknown }
  | { id: number; type: "error"; failure: Failure };

const post = (msg: Reply, transfer: Transferable[] = []) => (self as DedicatedWorkerGlobalScope).postMessage(msg, transfer);

// ---------------------------------------------------------------- engine modules

let panicMessage: string | null = null;
const modules: Partial<Record<Kind, Promise<Module>>> = {};
const ready = new Set<Kind>();

function engine(kind: Kind): Promise<Module> {
  const onPanic = (m: string) => {
    panicMessage = m;
  };
  modules[kind] ??=
    kind === "core"
      ? initCore().then(() => {
          core.on_panic(onPanic);
          ready.add("core");
          return core;
        })
      : import("./wasm/obj2cad_wasm_dwg.js").then(async (m) => {
          await m.default();
          m.on_panic(onPanic);
          ready.add("dwg");
          return m as unknown as Module;
        });
  return modules[kind]!;
}
// Start compiling the default engine right away.
void engine("core").catch(() => undefined);

// ---------------------------------------------------------------- state

interface Open {
  kind: Kind;
  session: core.Session;
  sources: SourceFile[];
  name: string;
  /** Parity hash per geometry-changing settings. */
  parity: Map<string, string>;
}
let open: Open | null = null;
let dead = false;

async function sha256(data: BufferSource): Promise<string> {
  const digest = await crypto.subtle.digest("SHA-256", data);
  return Array.from(new Uint8Array(digest), (b) => b.toString(16).padStart(2, "0")).join("");
}

/** Read a file, reporting progress for big ones. */
async function read(file: File, progress: (done: number) => void): Promise<Uint8Array<ArrayBuffer>> {
  if (file.size < 16 << 20) return new Uint8Array(await file.arrayBuffer());
  const out = new Uint8Array(file.size);
  const reader = file.stream().getReader();
  let at = 0;
  for (;;) {
    const { done, value } = await reader.read();
    if (done) break;
    if (at + value.length > out.length) throw new ReadError("The file changed while it was being read.");
    out.set(value, at);
    at += value.length;
    progress(at);
  }
  if (at !== out.length) throw new ReadError("The file changed while it was being read.");
  return out;
}

class ReadError extends Error {}

async function load(id: number, sources: SourceFile[], name: string, kind: Kind): Promise<Open> {
  if (!ready.has(kind)) post({ id, type: "progress", progress: { stage: "engine", done: 0, total: 0 } });
  const mod = await engine(kind).catch((e: unknown) => {
    throw Object.assign(new Error(String(e instanceof Error ? e.message : e)), { engineFailed: true });
  });
  open?.session.free();
  open = null;
  const session = new mod.Session();
  const total = sources.reduce((n, s) => n + s.file.size, 0);
  let before = 0;
  try {
    for (const s of sources) {
      const bytes = await read(s.file, (done) => post({ id, type: "progress", progress: { stage: "read", done: before + done, total } }));
      before += s.file.size;
      // Whole seconds, like the command-line tool; -1 when the date is unknown.
      const modified = s.file.lastModified > 0 ? Math.floor(s.file.lastModified / 1000) : -1;
      session.add_file(s.path, bytes, await sha256(bytes), modified);
    }
    session.load(name, (done: number, all: number) => post({ id, type: "progress", progress: { stage: "parse", done, total: all } }));
  } catch (e) {
    session.free();
    throw e;
  }
  return { kind, session, sources, name, parity: new Map() };
}

/** Move a conversion's preview buffers out of the engine. */
function takePreview(c: core.Conversion): PreviewBuffers | null {
  return c.has_preview()
    ? {
        positions: c.take_positions(),
        colors: c.take_colors(),
        indices: c.take_indices(),
        edges: c.take_edges(),
        groups: c.take_groups(),
        lines: c.take_lines(),
        lineColors: c.take_line_colors(),
        lineGroups: c.take_line_groups(),
        points: c.take_points(),
        pointColors: c.take_point_colors(),
        pointGroups: c.take_point_groups(),
        origin: Array.from(c.origin()),
        available: c.preview_available(),
      }
    : null;
}

const buffers = (p: PreviewBuffers) =>
  [p.positions, p.colors, p.indices, p.edges, p.groups, p.lines, p.lineColors, p.lineGroups, p.points, p.pointColors, p.pointGroups].map((a) => a.buffer);

function convert(id: number, o: Open, settings: EngineSettings, wantPreview: boolean, early: boolean): Converted {
  const t0 = performance.now();
  const json = JSON.stringify(settings);
  const key = geometryKey(settings);
  const progress = (stage: Progress["stage"], done = 0) => post({ id, type: "progress", progress: { stage, done, total: 0 } });
  const parts: ArrayBuffer[] = [];
  let written = 0;
  let reported = 0;
  const c = o.session.convert(
    json,
    o.parity.get(key) ?? "",
    wantPreview,
    (chunk: Uint8Array<ArrayBuffer>) => {
      parts.push(chunk.buffer);
      written += chunk.length;
      // Every 8 MB is plenty to show the file growing.
      if (written - reported >= 8 << 20) progress("write", (reported = written));
    },
    (stage: string) => progress(stage as Progress["stage"]),
    // The model goes to the app as soon as it can be shown; the file follows.
    !early
      ? undefined
      : (first: core.Conversion) => {
          try {
            const preview = takePreview(first);
            if (preview) post({ id, type: "early", early: { preview, report: first.report(), decisions: first.decisions() } }, buffers(preview));
          } finally {
            first.free();
          }
        },
  );
  try {
    o.parity.set(key, c.parity());
    const preview = takePreview(c);
    return {
      parts,
      report: c.report(),
      decisions: c.decisions(),
      timings: JSON.parse(c.timings()) as Record<string, number>,
      preview,
      ms: performance.now() - t0,
    };
  } finally {
    c.free();
  }
}

function failure(err: unknown): Failure {
  if (panicMessage !== null || err instanceof WebAssembly.RuntimeError) {
    dead = true;
    return { kind: "crash", message: panicMessage ?? String(err) };
  }
  if (err && typeof err === "object" && "kind" in err && "line" in err && "issues" in err) return { kind: "parse", parse: err as ParseFailure };
  if (err instanceof Error && "engineFailed" in err) return { kind: "engine", message: err.message };
  if (err instanceof ReadError || (err instanceof DOMException && err.name === "NotReadableError")) return { kind: "read", message: (err as Error).message };
  return { kind: "other", message: err instanceof Error ? err.message : String(err) };
}

self.onmessage = async (e: MessageEvent<Request>) => {
  const req = e.data;
  if (dead) {
    post({ id: req.id, type: "error", failure: { kind: "crash", message: panicMessage ?? "the engine stopped" } });
    return;
  }
  try {
    if (req.type === "warm") {
      // Compile an engine module ahead of need (the DWG writer, for people who use it).
      await engine(req.dwg ? "dwg" : "core");
      post({ id: req.id, type: "ok", result: null });
    } else if (req.type === "open") {
      open = await load(req.id, req.sources, req.name, req.dwg ? "dwg" : "core");
      post({ id: req.id, type: "ok", result: JSON.parse(open.session.inspect()) });
    } else {
      if (!open) throw new Error("no file is open");
      // DWG needs the larger engine: move the open file into it once.
      if (req.settings.format === "dwg" && open.kind !== "dwg") open = await load(req.id, open.sources, open.name, "dwg");
      const r = convert(req.id, open, req.settings, req.preview, req.early);
      post({ id: req.id, type: "ok", result: r }, [...(r.preview ? buffers(r.preview) : []), ...r.parts]);
    }
  } catch (err) {
    post({ id: req.id, type: "error", failure: failure(err) });
  }
};
