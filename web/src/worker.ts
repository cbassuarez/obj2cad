/// <reference lib="webworker" />
// Runs the Rust engine off the main thread. The worker keeps the parsed file, so a
// settings change only re-converts. Two engine modules: the small default one, and one
// with the DWG writer that is loaded the first time DWG is chosen.
import initCore, * as core from "./wasm/obj2cad_wasm.js";
import type { EngineSettings } from "@/lib/settings";
import { geometryKey } from "@/lib/settings";

type Module = typeof core;
type Kind = "core" | "dwg";

export interface Progress {
  stage: "read" | "parse";
  done: number;
  total: number;
}

export interface ParseFailure {
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
  file: Blob;
  report: string;
  decisions: string;
  timings: Record<string, number>;
  preview: PreviewBuffers | null;
  ms: number;
}

export type Request =
  | { id: number; type: "open"; file: File; mtl: File | null; dwg: boolean }
  | { id: number; type: "mtl"; mtl: File }
  | { id: number; type: "convert"; settings: EngineSettings; preview: boolean };

export type Reply =
  | { id: number; type: "progress"; progress: Progress }
  | { id: number; type: "ok"; result: unknown }
  | { id: number; type: "error"; failure: Failure };

const post = (msg: Reply, transfer: Transferable[] = []) => (self as DedicatedWorkerGlobalScope).postMessage(msg, transfer);

// ---------------------------------------------------------------- engine modules

let panicMessage: string | null = null;
const modules: Partial<Record<Kind, Promise<Module>>> = {};

function engine(kind: Kind): Promise<Module> {
  const onPanic = (m: string) => {
    panicMessage = m;
  };
  modules[kind] ??=
    kind === "core"
      ? initCore().then(() => {
          core.on_panic(onPanic);
          return core;
        })
      : import("./wasm/obj2cad_wasm_dwg.js").then(async (m) => {
          await m.default();
          m.on_panic(onPanic);
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
  file: File;
  mtl: File | null;
  sha: string;
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

async function load(id: number, file: File, mtl: File | null, kind: Kind): Promise<Open> {
  const mod = await engine(kind).catch((e: unknown) => {
    throw Object.assign(new Error(String(e instanceof Error ? e.message : e)), { engineFailed: true });
  });
  const bytes = await read(file, (done) => post({ id, type: "progress", progress: { stage: "read", done, total: file.size } }));
  const sha = await sha256(bytes);
  open?.session.free();
  open = null;
  const session = new mod.Session(bytes, file.name, sha, (done: number, total: number) =>
    post({ id, type: "progress", progress: { stage: "parse", done, total } }),
  );
  if (mtl) session.set_mtl(new Uint8Array(await mtl.arrayBuffer()));
  return { kind, session, file, mtl, sha, parity: new Map() };
}

function convert(o: Open, settings: EngineSettings, wantPreview: boolean): Promise<Converted> {
  return (async () => {
    const t0 = performance.now();
    const json = JSON.stringify(settings);
    const key = geometryKey(settings);
    let parity = o.parity.get(key);
    if (!parity) {
      parity = await sha256(o.session.parity_stream(json) as Uint8Array<ArrayBuffer>);
      o.parity.set(key, parity);
    }
    const chunks: Uint8Array<ArrayBuffer>[] = [];
    const c = o.session.convert(json, parity, wantPreview, (chunk: Uint8Array<ArrayBuffer>) => chunks.push(chunk));
    try {
      const preview: PreviewBuffers | null = c.has_preview()
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
      return {
        file: new Blob(chunks, { type: "application/octet-stream" }),
        report: c.report(),
        decisions: c.decisions(),
        timings: JSON.parse(c.timings()) as Record<string, number>,
        preview,
        ms: performance.now() - t0,
      };
    } finally {
      c.free();
    }
  })();
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
    if (req.type === "open") {
      open = await load(req.id, req.file, req.mtl, req.dwg ? "dwg" : "core");
      post({ id: req.id, type: "ok", result: JSON.parse(open.session.inspect()) });
    } else if (req.type === "mtl") {
      if (!open) throw new Error("no file is open");
      open.session.set_mtl(new Uint8Array(await req.mtl.arrayBuffer()));
      open.mtl = req.mtl;
      post({ id: req.id, type: "ok", result: null });
    } else {
      if (!open) throw new Error("no file is open");
      // DWG needs the larger engine: move the open file into it once.
      if (req.settings.format === "dwg" && open.kind !== "dwg") open = await load(req.id, open.file, open.mtl, "dwg");
      const r = await convert(open, req.settings, req.preview);
      const p = r.preview;
      const transfer = p
        ? [p.positions, p.colors, p.indices, p.edges, p.groups, p.lines, p.lineColors, p.lineGroups, p.points, p.pointColors, p.pointGroups].map((a) => a.buffer)
        : [];
      post({ id: req.id, type: "ok", result: r }, transfer);
    }
  } catch (err) {
    post({ id: req.id, type: "error", failure: failure(err) });
  }
};
