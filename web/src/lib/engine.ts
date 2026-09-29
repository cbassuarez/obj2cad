// Main-thread side of the conversion worker. The worker owns the WebAssembly engine and
// the parsed file; the UI sends small commands and receives the file as a Blob plus
// transferable preview buffers. If the engine crashes, the worker is replaced.
//
// The worker holds one file at a time and the viewer and the file list both use it, so
// every call here runs alone, in order, and a conversion names the file it is for: when
// the worker holds another one, that file is opened again first. Without this, opening a
// file from the list while the list was still converting could show (and download)
// another file's drawing under its name.
import type { Converted, Failure, ParseFailure, PreviewBuffers, Progress, Reply, Request } from "@/worker";
import type { EngineSettings, LayerMode, UpAxis, Units } from "@/lib/settings";

export type { Failure, ParseFailure, PreviewBuffers, Progress };

export interface Inspection {
  vertices: number;
  faces: number;
  lines: number;
  points: number;
  objects: number;
  groups: number;
  materials: string[];
  mtllibs: string[];
  parse_ms: number;
  hints: {
    exporter: string | null;
    units: Units;
    units_source: "exporter" | "size" | "none";
    up_axis: UpAxis;
    up_axis_confident: boolean;
  };
}

/** Units and up direction as resolved, and where each came from. */
export interface Decisions {
  units: Units;
  units_from: "chosen" | "file" | "default" | "size" | "none";
  up_axis: UpAxis;
  up_from: "chosen" | "detected";
  detected_units: Units;
  detected_up_axis: UpAxis;
}

/** The report the engine writes next to every file (crates/obj2cad-core/src/report.rs). */
export interface Report {
  schema: string;
  engine_version: string;
  parity_hash: string;
  rotated: boolean;
  point_cloud: boolean;
  input: { name: string; bytes: number; sha256: string; vertices: number; faces: number; lines: number; point_elements: number };
  output: {
    format: string;
    bytes: number;
    sha256: string | null;
    mesh_entities: number;
    faces: number;
    polylines: number;
    points: number;
    vertices_written: number;
    unreferenced_vertices_skipped: number;
    bounds: [[number, number, number], [number, number, number]] | null;
  };
  options: { units: Units; up_axis: UpAxis; layer_mode: LayerMode; keep_loose_points: boolean; exclude_layers: string[] };
  omissions: {
    freeform_surfaces: number;
    freeform_curves: number;
    broken_faces: number;
    loose_points: number;
    excluded_layers: string[];
    excluded_faces: number;
    excluded_lines: number;
    excluded_points: number;
  };
  layers: { name: string; source: string; color: string; entity_colors: string[]; faces: number; polylines: number; points: number }[];
  diagnostics: { severity: "info" | "warning"; code: string; line: number; count: number; message: string }[];
}

export interface Result {
  file: Blob;
  report: Report;
  decisions: Decisions;
  preview: PreviewBuffers | null;
  timings: Record<string, number>;
  ms: number;
}

export class EngineError extends Error {
  constructor(public failure: Failure) {
    super(failure.kind === "parse" ? failure.parse.message : failure.message);
  }
}

type Without<T, K extends keyof T> = T extends unknown ? Omit<T, K> : never;
interface Pending {
  resolve: (r: unknown) => void;
  reject: (e: EngineError) => void;
  progress?: (p: Progress) => void;
}

/** A model and the material library that goes with it. */
export interface Source {
  file: File;
  mtl: File | null;
}

class Engine {
  private worker!: Worker;
  private seq = 0;
  private pending = new Map<number, Pending>();
  /** What the worker holds now; `null` after a crash or before the first open. */
  private loaded: Source | null = null;
  private queue: Promise<unknown> = Promise.resolve();

  constructor() {
    this.start();
  }

  private start() {
    this.worker = new Worker(new URL("../worker.ts", import.meta.url), { type: "module" });
    this.worker.onmessage = (e: MessageEvent<Reply>) => {
      const msg = e.data;
      const p = this.pending.get(msg.id);
      if (!p) return;
      if (msg.type === "progress") {
        p.progress?.(msg.progress);
        return;
      }
      this.pending.delete(msg.id);
      if (msg.type === "ok") p.resolve(msg.result);
      else {
        if (msg.failure.kind === "crash") this.restart();
        p.reject(new EngineError(msg.failure));
      }
    };
    // The worker script itself failed (e.g. out of memory while loading).
    this.worker.onerror = (e) => {
      e.preventDefault();
      this.failAll({ kind: "crash", message: e.message || "the engine stopped" });
      this.restart();
    };
  }

  private failAll(failure: Failure) {
    this.loaded = null;
    for (const p of this.pending.values()) p.reject(new EngineError(failure));
    this.pending.clear();
  }

  /** Replace the worker (after a crash). The open file is lost and must be opened again. */
  restart() {
    this.worker.terminate();
    this.failAll({ kind: "crash", message: "the engine was restarted" });
    this.start();
  }

  private call<T>(req: Without<Request, "id">, progress?: (p: Progress) => void): Promise<T> {
    const id = ++this.seq;
    return new Promise<T>((resolve, reject) => {
      this.pending.set(id, { resolve: resolve as (r: unknown) => void, reject, progress });
      this.worker.postMessage({ ...req, id } as Request);
    });
  }

  /** Run `task` after every call before it has finished, and before any call after it. */
  private serial<T>(task: () => Promise<T>): Promise<T> {
    const run = this.queue.then(task, task);
    this.queue = run.catch(() => undefined);
    return run;
  }

  /** Make the worker hold `src` (and its material library), opening it again if needed. */
  private async hold(src: Source, dwg: boolean): Promise<void> {
    const l = this.loaded;
    if (l?.file === src.file && (l.mtl === src.mtl || src.mtl === null)) return;
    if (l?.file === src.file && src.mtl) {
      await this.call<void>({ type: "mtl", mtl: src.mtl });
      this.loaded = { file: src.file, mtl: src.mtl };
      return;
    }
    this.loaded = null;
    await this.call<Inspection>({ type: "open", file: src.file, mtl: src.mtl, dwg });
    this.loaded = { file: src.file, mtl: src.mtl };
  }

  open(file: File, dwg: boolean, progress?: (p: Progress) => void): Promise<Inspection> {
    return this.serial(async () => {
      this.loaded = null;
      const info = await this.call<Inspection>({ type: "open", file, mtl: null, dwg }, progress);
      this.loaded = { file, mtl: null };
      return info;
    });
  }

  /** Give `file` its material library. */
  setMtl(file: File, mtl: File): Promise<void> {
    return this.serial(() => this.hold({ file, mtl }, false));
  }

  convert(src: Source, settings: EngineSettings, preview: boolean, progress?: (p: Progress) => void): Promise<Result> {
    return this.serial(async () => {
      await this.hold(src, settings.format === "dwg");
      return parse(await this.call<Converted>({ type: "convert", settings, preview }, progress));
    });
  }
}

function parse(r: Converted): Result {
  return {
    file: r.file,
    report: JSON.parse(r.report) as Report,
    decisions: JSON.parse(r.decisions) as Decisions,
    preview: r.preview,
    timings: r.timings,
    ms: r.ms,
  };
}

export const engine = new Engine();

/** SHA-256 of a blob, as hex (for the report, on demand). */
export async function sha256Hex(blob: Blob): Promise<string> {
  const digest = await crypto.subtle.digest("SHA-256", await blob.arrayBuffer());
  return Array.from(new Uint8Array(digest), (b) => b.toString(16).padStart(2, "0")).join("");
}

/** Nothing was written: no faces, lines or points. */
export const isEmpty = (r: Report) => r.output.faces + r.output.polylines + r.output.points === 0;
