/// <reference lib="webworker" />
// Runs the Rust core off the main thread. The worker keeps the loaded file so changing a
// setting re-converts without copying the (possibly large) OBJ again.
import init, { Session } from "./wasm/obj2cad_wasm.js";

export type Request =
  | { id: number; type: "load"; obj: ArrayBuffer; mtl: ArrayBuffer | null; name: string }
  | { id: number; type: "mtl"; mtl: ArrayBuffer }
  | { id: number; type: "convert"; units: string; up: string };

export interface Inspection {
  vertices: number;
  faces: number;
  lines: number;
  points: number;
  objects: number;
  groups: number;
  materials: string[];
  mtllibs: string[];
  header_comments: string[];
  parse_ms: number;
  hints: {
    exporter: string | null;
    units: string;
    units_reason: string;
    up_axis: string;
    up_axis_reason: string;
  };
}

export interface ConvertResult {
  dxf: Uint8Array;
  report: string;
  positions: Float32Array;
  indices: Uint32Array;
  edges: Uint32Array;
  colors: Uint8Array;
  lines: Float32Array;
  /** Per mesh: [layer, indexStart, indexCount, edgeStart, edgeCount]. */
  groups: Uint32Array;
  points: Float32Array;
  /** False when the model is too large to display; the DXF is unaffected. */
  previewAvailable: boolean;
  origin: number[];
  ms: number;
  timings: Record<string, number>;
}

export type Response =
  | { id: number; ok: true; result: Inspection | ConvertResult | null }
  | { id: number; ok: false; error: string };

const ready = init();
// The parsed file lives in wasm memory for as long as it is open.
let session: Session | null = null;

async function sha256(data: ArrayBuffer | Uint8Array): Promise<string> {
  const digest = await crypto.subtle.digest("SHA-256", data as BufferSource);
  return Array.from(new Uint8Array(digest), (b) => b.toString(16).padStart(2, "0")).join("");
}

const reply = (msg: Response, transfer: Transferable[] = []) => (self as DedicatedWorkerGlobalScope).postMessage(msg, transfer);

self.onmessage = async (e: MessageEvent<Request>) => {
  const req = e.data;
  try {
    await ready;
    if (req.type === "load") {
      session?.free();
      session = null;
      const sha = await sha256(req.obj); // native, hardware-accelerated
      session = new Session(new Uint8Array(req.obj), req.mtl ? new Uint8Array(req.mtl) : undefined, req.name, sha);
      reply({ id: req.id, ok: true, result: JSON.parse(session.inspect()) as Inspection });
    } else if (req.type === "mtl") {
      if (!session) throw new Error("no file loaded");
      session.set_mtl(new Uint8Array(req.mtl));
      reply({ id: req.id, ok: true, result: null });
    } else {
      if (!session) throw new Error("no file loaded");
      const t0 = performance.now();
      // Parity hash: canonical bytes from Rust, SHA-256 from the browser (hardware-accelerated).
      const parity = await sha256(session.parity_stream(req.units, req.up));
      const hashMs = performance.now() - t0;
      const c = session.convert(req.units, req.up, parity);
      const dxf = c.take_dxf();
      const t1 = performance.now();
      const report = JSON.parse(c.report());
      report.output.sha256 = await sha256(dxf);
      const timings = { ...JSON.parse(c.timings()), parity_ms: hashMs, output_sha_ms: performance.now() - t1 };
      const result: ConvertResult = {
        dxf,
        report: JSON.stringify(report, null, 2),
        positions: c.take_positions(),
        indices: c.take_indices(),
        edges: c.take_edges(),
        colors: c.take_colors(),
        lines: c.take_lines(),
        groups: c.take_groups(),
        points: c.take_points(),
        previewAvailable: c.preview_available(),
        origin: Array.from(c.origin()),
        ms: performance.now() - t0,
        timings,
      };
      c.free();
      reply({ id: req.id, ok: true, result }, [
        result.dxf.buffer,
        result.positions.buffer,
        result.indices.buffer,
        result.edges.buffer,
        result.colors.buffer,
        result.lines.buffer,
        result.groups.buffer,
        result.points.buffer,
      ]);
    }
  } catch (err) {
    reply({ id: req.id, ok: false, error: err instanceof Error ? err.message : String(err) });
  }
};

