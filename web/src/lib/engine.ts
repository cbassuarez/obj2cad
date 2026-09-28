// Main-thread side of the conversion worker. The worker owns the WebAssembly engine and
// the parsed file; the UI sends small commands and receives the file as a Blob plus
// transferable preview buffers. If the engine crashes, the worker is replaced.
import type { Converted, Failure, ParseFailure, PreviewBuffers, Progress, Reply, Request, SourceFile } from "@/worker";
import type { EngineSettings, LayerMode, UpAxis, Units } from "@/lib/settings";

export type { Failure, ParseFailure, PreviewBuffers, Progress };

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
  texture_colored_faces: number;
  files: BundleFile[];
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
  layers: { name: string; source: string; color: string; entity_colors: string[]; more_colors: boolean; faces: number; polylines: number; points: number }[];
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

class Engine {
  private worker!: Worker;
  private seq = 0;
  private pending = new Map<number, Pending>();

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

  /** Read the files of one drawing. `name` names it when it holds several models. */
  open(sources: SourceFile[], name: string, dwg: boolean, progress?: (p: Progress) => void): Promise<Inspection> {
    return this.call<Inspection>({ type: "open", sources, name, dwg }, progress);
  }

  async convert(settings: EngineSettings, preview: boolean, progress?: (p: Progress) => void): Promise<Result> {
    const r = await this.call<Converted>({ type: "convert", settings, preview }, progress);
    return {
      file: r.file,
      report: JSON.parse(r.report) as Report,
      decisions: JSON.parse(r.decisions) as Decisions,
      preview: r.preview,
      timings: r.timings,
      ms: r.ms,
    };
  }
}

export const engine = new Engine();

/** SHA-256 of a blob, as hex (for the report, on demand). */
export async function sha256Hex(blob: Blob): Promise<string> {
  const digest = await crypto.subtle.digest("SHA-256", await blob.arrayBuffer());
  return Array.from(new Uint8Array(digest), (b) => b.toString(16).padStart(2, "0")).join("");
}

/** Nothing was written: no faces, lines or points. */
export const isEmpty = (r: Report) => r.output.faces + r.output.polylines + r.output.points === 0;
