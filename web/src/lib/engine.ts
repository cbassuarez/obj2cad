// Main-thread side of the conversion worker. The worker owns the WebAssembly engine and
// the parsed file; the UI only sends small commands and receives transferable buffers.
import type { ConvertResult, Inspection, Request, Response } from "@/worker";

export type { ConvertResult, Inspection };

type Without<T, K extends keyof T> = T extends unknown ? Omit<T, K> : never;

const worker = new Worker(new URL("../worker.ts", import.meta.url), { type: "module" });
let seq = 0;
const pending = new Map<number, (r: Response) => void>();
worker.onmessage = (e: MessageEvent<Response>) => {
  pending.get(e.data.id)?.(e.data);
  pending.delete(e.data.id);
};

function call<T>(req: Without<Request, "id">, transfer: Transferable[] = []): Promise<T> {
  const id = ++seq;
  return new Promise((resolve, reject) => {
    pending.set(id, (r) => (r.ok ? resolve(r.result as T) : reject(new Error(r.error))));
    worker.postMessage({ ...req, id } as Request, transfer);
  });
}

export async function load(obj: File): Promise<Inspection> {
  const buf = await obj.arrayBuffer();
  return call<Inspection>({ type: "load", obj: buf, mtl: null, name: obj.name }, [buf]);
}

export async function setMtl(mtl: File): Promise<void> {
  const buf = await mtl.arrayBuffer();
  await call<null>({ type: "mtl", mtl: buf }, [buf]);
}

export function convert(units: string, up: string): Promise<ConvertResult> {
  return call<ConvertResult>({ type: "convert", units, up });
}

export interface Report {
  parity_hash: string;
  engine_version: string;
  input: { name: string; bytes: number; sha256: string; vertices: number; faces: number };
  output: {
    faces: number;
    mesh_entities: number;
    polylines: number;
    points: number;
    vertices_written: number;
    bytes: number;
    sha256: string;
    bounds: [[number, number, number], [number, number, number]] | null;
  };
  layers: { name: string; source: string; faces: number; polylines: number; points: number }[];
  diagnostics: { severity: "info" | "warning"; code: string; line: number; count: number; message: string }[];
}
