// Main-thread side of the conversion worker. The worker owns the WebAssembly engine and
// the parsed file; the UI sends small commands and receives the file as a Blob plus
// transferable preview buffers. If the engine crashes, the worker is replaced.
//
// The worker holds one drawing at a time and the viewer and the file list both use it, so
// every call here runs alone, in order, and a conversion names the drawing it is for: when
// the worker holds another one, that drawing is opened again first. Without this, opening
// a file from the list while the list was still converting could show (and download)
// another file's drawing under its name.
import type { Converted, Early as EarlyReply, Failure, ParseFailure, PointBatch, PreviewBuffers, Progress, Reply, Request, SourceFile } from "@/worker";
import type { EngineSettings, LayerMode, UpAxis, Units } from "@/lib/settings";

export type { Failure, ParseFailure, PointBatch, PreviewBuffers, Progress };

/** A file of the drawing and what it was used for (crates/obj2cad-core/src/bundle.rs). */
export interface BundleFile {
  name: string;
  role: "model" | "point_cloud" | "materials" | "texture" | "not_used" | "missing" | "unreadable";
  bytes: number;
  sha256: string | null;
  note?: string;
}

export interface Inspection {
  /** The drawing's name: the model's file name, or the zip's or folder's. */
  name: string;
  files: BundleFile[];
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
    units_source: "exporter" | "assumed";
    up_axis: UpAxis;
    up_axis_confident: boolean;
  };
}

/** Units and up direction as resolved, and where each came from. */
export interface Decisions {
  units: Units;
  units_from: "chosen" | "file" | "default" | "assumed";
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
    /** Free-form curves, as B-splines. */
    splines: number;
    vertices_written: number;
    unreferenced_vertices_skipped: number;
    bounds: [[number, number, number], [number, number, number]] | null;
  };
  options: { units: Units; up_axis: UpAxis; layer_mode: LayerMode; keep_loose_points: boolean; exclude_layers: string[] };
  texture_colored_faces: number;
  /** Faces colored with the average of their vertices' colors. */
  vertex_colored_faces?: number;
  files: BundleFile[];
  /** Curved surfaces written next to the mesh (crates/obj2cad-curves). */
  curves: { kind: "cylinder" | "cone" | "sphere" | "torus"; faces: number[]; max_deviation: number; tolerance: number }[];
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
  layers: { name: string; source: string; color: string; entity_colors: string[]; more_colors: boolean; faces: number; polylines: number; points: number; surfaces: number; file?: string }[];
  diagnostics: { severity: "info" | "warning"; code: string; line: number; count: number; message: string }[];
}

/** The model, shown before its file is written: the report is provisional (no parity
 *  hash, no output size). */
export interface Early {
  preview: PreviewBuffers;
  report: Report;
  decisions: Decisions;
}

/** A written file, as the writer's pieces (transferred from the worker, not copied). They
 *  become one `Blob` a few megabytes at a time while the page is idle: in one go, a
 *  large file (a scan's 370 MB) held the page for seconds. `blob()` finishes the job at
 *  once when it's needed sooner. */
export class OutputFile {
  readonly size: number;
  private parts: ArrayBuffer[];
  private packed: Blob[] = [];
  private whole: Blob | null = null;

  constructor(parts: ArrayBuffer[]) {
    this.parts = parts;
    this.size = parts.reduce((n, p) => n + p.byteLength, 0);
    this.schedule();
  }

  private schedule() {
    if (!this.parts.length) return;
    const idle = window.requestIdleCallback ?? ((cb: () => void) => setTimeout(cb, 16));
    idle(() => {
      this.pack(8);
      this.schedule();
    });
  }

  private pack(n: number) {
    const take = this.parts.splice(0, n);
    if (take.length) this.packed.push(new Blob(take));
  }

  /** The file (pieces not packed yet are packed now; a blob of blobs is not copied again). */
  blob(): Blob {
    if (!this.whole) {
      while (this.parts.length) this.pack(64);
      this.whole = new Blob(this.packed, { type: "application/octet-stream" });
      this.packed = [];
    }
    return this.whole;
  }
}

export interface Result {
  file: OutputFile;
  report: Report;
  decisions: Decisions;
  preview: PreviewBuffers | null;
  timings: Record<string, number>;
  ms: number;
}

/** The failure of calls stopped by a restart (a crash elsewhere, or a cancelled open). */
export const RESTARTED = "the engine was restarted";

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
  early?: (e: EarlyReply) => void;
  points?: (b: PointBatch) => void;
  /** Reading or converting a drawing (not warming up an engine module). */
  work: boolean;
}

/** The files of one drawing, as the app holds them (`Job` in lib/files.ts). A drawing is
 *  known by this object: a new one (another file, added materials, a reload) is new. */
export interface Drawing {
  name: string;
  sources: SourceFile[];
}

class Engine {
  private worker!: Worker;
  private seq = 0;
  private pending = new Map<number, Pending>();
  /** The drawing the worker holds; `null` after a crash or before the first open. */
  private loaded: Drawing | null = null;
  private queue: Promise<unknown> = Promise.resolve();
  /** Counts restarts: calls queued before one are dropped, not run. */
  private generation = 0;

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
      if (msg.type === "early") {
        p.early?.(msg.early);
        return;
      }
      if (msg.type === "points") {
        p.points?.(msg.batch);
        return;
      }
      this.pending.delete(msg.id);
      if (msg.type === "ok") p.resolve(msg.result);
      else {
        if (msg.failure.kind === "crash" || msg.failure.kind === "memory") this.restart();
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
    this.generation++;
    this.worker.terminate();
    this.failAll({ kind: "crash", message: RESTARTED });
    this.start();
  }

  private call<T>(
    req: Without<Request, "id">,
    progress?: (p: Progress) => void,
    early?: (e: EarlyReply) => void,
    points?: (b: PointBatch) => void,
  ): Promise<T> {
    const id = ++this.seq;
    return new Promise<T>((resolve, reject) => {
      this.pending.set(id, { resolve: resolve as (r: unknown) => void, reject, progress, early, points, work: req.type !== "warm" });
      this.worker.postMessage({ ...req, id } as Request);
    });
  }

  /** Run `task` after every call before it has finished, and before any call after it.
   *  A task queued before a restart is dropped (it was for work that was stopped). */
  private serial<T>(task: () => Promise<T>): Promise<T> {
    const generation = this.generation;
    const start = () => (generation === this.generation ? task() : Promise.reject(new EngineError({ kind: "crash", message: RESTARTED })));
    const run = this.queue.then(start, start);
    this.queue = run.catch(() => undefined);
    return run;
  }

  private async load(d: Drawing, dwg: boolean, progress?: (p: Progress) => void, points?: (b: PointBatch) => void): Promise<Inspection> {
    this.loaded = null;
    const info = await this.call<Inspection>({ type: "open", sources: d.sources, name: d.name, dwg }, progress, undefined, points);
    this.loaded = d;
    return info;
  }

  /** Load an engine module ahead of need, so the first DWG isn't kept waiting on it. */
  warm(dwg: boolean): void {
    void this.call({ type: "warm", dwg }).catch(() => undefined);
  }

  /** Read the files of one drawing. Its `name` names it when it holds several models.
   *  With `interrupt`, whatever the engine is doing for another drawing is stopped rather
   *  than waited for (a person opened another file; the old one's work is moot).
   *  `points` receives the points of large files as they are read, to show meanwhile. */
  open(d: Drawing, dwg: boolean, progress?: (p: Progress) => void, interrupt = false, points?: (b: PointBatch) => void): Promise<Inspection> {
    if (interrupt && [...this.pending.values()].some((p) => p.work)) this.restart();
    return this.serial(() => this.load(d, dwg, progress, points));
  }

  /** The model to show, with a provisional report, and no file (part of a bundle, shown
   *  while the rest of it loads; the file is written once, by `convert`, from all of it). */
  preview(d: Drawing, settings: EngineSettings): Promise<Early> {
    return this.serial(async () => {
      if (this.loaded !== d) await this.load(d, settings.format === "dwg");
      const e = await this.call<EarlyReply>({ type: "preview", settings });
      return { preview: e.preview, report: JSON.parse(e.report) as Report, decisions: JSON.parse(e.decisions) as Decisions };
    });
  }

  /** Convert the drawing. With `early`, the preview comes to it as soon as the model can be
   *  shown, and the result then has no preview. */
  convert(d: Drawing, settings: EngineSettings, preview: boolean, progress?: (p: Progress) => void, early?: (e: Early) => void): Promise<Result> {
    return this.serial(async () => {
      if (this.loaded !== d) await this.load(d, settings.format === "dwg", progress);
      const onEarly =
        early &&
        ((e: EarlyReply) => early({ preview: e.preview, report: JSON.parse(e.report) as Report, decisions: JSON.parse(e.decisions) as Decisions }));
      const r = await this.call<Converted>({ type: "convert", settings, preview, early: early !== undefined }, progress, onEarly);
      return {
        // The transferred pieces; packed into one Blob a little at a time (see OutputFile).
        file: new OutputFile(r.parts),
        report: JSON.parse(r.report) as Report,
        decisions: JSON.parse(r.decisions) as Decisions,
        preview: r.preview,
        timings: r.timings,
        ms: r.ms,
      };
    });
  }
}

export const engine = new Engine();

/** SHA-256 of a blob, as hex (for the report, on demand). */
export async function sha256Hex(blob: Blob): Promise<string> {
  const digest = await crypto.subtle.digest("SHA-256", await blob.arrayBuffer());
  return Array.from(new Uint8Array(digest), (b) => b.toString(16).padStart(2, "0")).join("");
}

/** Nothing was written: no faces, lines, curves or points. */
export const isEmpty = (r: Report) => r.output.faces + r.output.polylines + r.output.points + (r.output.splines ?? 0) === 0;
