// Opening a drawing, as the station rail shows it: one station per real step, each with
// its own measure. A bar is only shown for something counted against a known total
// (bytes read of the files' size, bytes parsed of the models' and clouds', elements
// written of the drawing's); every other step is a named state. Measures never go back:
// a bundle's light files are read, shown, then read again with its large ones, and each
// bar keeps the most it has shown.
import type { Progress } from "@/lib/engine";
import { bytes, fmt } from "@/lib/format";

export type StationKey = "engine" | "read" | "model" | "curves" | "view" | "hash" | "write";
/** `paused`: started, not finished, not running now (a bundle's read, stopped while its
 *  models are shown). */
export type StationState = "idle" | "active" | "paused" | "done";

export interface Station {
  key: StationKey;
  label: string;
  state: StationState;
  /** 0..1 for a counted step, else null. */
  frac: number | null;
  /** What it has done, in words. */
  value: string;
}

export interface Run {
  stations: Station[];
  /** Sizes of the drawing's files, and of its models and clouds (what the engine parses). */
  total: number;
  geometry: number;
  /** The most read and parsed so far, in bytes. */
  read: number;
  parsed: number;
  /** A file's count while it is parsed ("scan.xyz", 1204113, cloud). */
  counting: { file: string; count: number; cloud: boolean } | null;
  format: string;
  finished: boolean;
}

const LABELS: Record<StationKey, string> = {
  engine: "DWG writer",
  read: "Read",
  model: "Model",
  curves: "Curves",
  view: "View",
  hash: "Fingerprint",
  write: "Write",
};

/** A new run: every station waiting. */
export function startRun(o: { total: number; geometry: number; format: "DXF" | "DWG"; curves: boolean }): Run {
  const keys: StationKey[] = [...(o.format === "DWG" ? (["engine"] as const) : []), "read", "model", ...(o.curves ? (["curves"] as const) : []), "view", "hash", "write"];
  return {
    stations: keys.map((key) => ({ key, label: key === "write" ? `Write ${o.format}` : LABELS[key], state: "idle", frac: null, value: "" })),
    total: o.total,
    geometry: o.geometry,
    read: 0,
    parsed: 0,
    counting: null,
    format: o.format,
    finished: false,
  };
}

const ORDER: StationKey[] = ["engine", "read", "model", "curves", "view", "hash", "write"];
const STAGE: Record<Progress["stage"], StationKey> = { engine: "engine", read: "read", parse: "model", curves: "curves", preview: "view", hash: "hash", write: "write" };

function set(run: Run, key: StationKey, patch: Partial<Station>): Run {
  return { ...run, stations: run.stations.map((s) => (s.key === key ? { ...s, ...patch } : s)) };
}

const get = (run: Run, key: StationKey) => run.stations.find((s) => s.key === key);
const pct = (f: number) => `${Math.floor(f * 100)}%`;

/** Stations before `key` that were running: finished, or paused where their measure isn't
 *  complete (a bundle's large files are still to come). The view is only finished by
 *  `shown`. */
function settleBefore(run: Run, key: StationKey): Run {
  const at = ORDER.indexOf(key);
  let r = run;
  // A step after parsing runs on everything read: reading and parsing are complete.
  if (at > ORDER.indexOf("model"))
    for (const s of run.stations) if (ORDER.indexOf(s.key) <= ORDER.indexOf("model")) r = set(r, s.key, { state: "done", frac: s.frac === null ? null : 1 });
  for (const s of r.stations) {
    if (ORDER.indexOf(s.key) >= at || s.state !== "active") continue;
    const complete = s.frac === null ? s.key !== "view" : s.frac >= 1;
    // "computing", "building": words for while it runs.
    r = set(r, s.key, { state: complete ? "done" : "paused", value: complete && s.frac === null && /ing$/.test(s.value) ? "" : s.value });
  }
  return r;
}

/** A step started or moved on (the engine's progress). */
export function onProgress(run: Run, p: Progress): Run {
  const key = STAGE[p.stage];
  if (!get(run, key) || run.finished) return run;
  let r = settleBefore(run, key);
  switch (key) {
    case "read": {
      const read = Math.min(Math.max(r.read, p.done), r.total);
      const frac = r.total > 0 ? read / r.total : 1;
      r = { ...set(r, "read", { state: "active", frac, value: read >= r.total ? bytes(r.total) : `${bytes(read)} / ${bytes(r.total)}` }), read };
      break;
    }
    case "model": {
      const parsed = Math.min(Math.max(r.parsed, p.done), r.geometry);
      const frac = r.geometry > 0 ? parsed / r.geometry : 1;
      const counting = p.file !== undefined && p.count !== undefined ? { file: p.file, count: p.count, cloud: !!p.cloud } : r.counting;
      const value = counting ? `${fmt(counting.count)} ${counting.cloud ? "points" : "vertices"}` : pct(frac);
      r = { ...set(r, "model", { state: "active", frac, value }), parsed, counting };
      break;
    }
    case "view":
      r = set(r, "view", { state: "active", value: "building" });
      break;
    case "hash":
      r = set(r, "hash", { state: "active", value: "computing" });
      break;
    case "write": {
      const frac = p.total > 0 ? Math.min(p.done / p.total, 1) : null;
      const prev = get(r, "write")?.frac ?? 0;
      const f = frac === null ? null : Math.max(frac, prev ?? 0);
      r = set(r, "write", { state: "active", frac: f, value: f === null ? "writing" : pct(f) });
      // The fingerprint is known once writing starts (it goes in the file).
      if (get(r, "hash")?.state !== "done") r = set(r, "hash", { state: "done", value: "" });
      break;
    }
    default:
      r = set(r, key, { state: "active" });
  }
  // Everything before a running step has been done at least in part: a step that never
  // reported (a small file's read, a known fingerprint) is done.
  const at = ORDER.indexOf(key);
  // (Reading and parsing are counted: their measures say how far they are.)
  for (const s of r.stations)
    if (ORDER.indexOf(s.key) < at && s.state === "idle" && s.key !== "view" && s.key !== "read" && s.key !== "model") r = set(r, s.key, { state: "done" });
  return r;
}

/** The view shows part of the drawing (`names` still loading) or all of it. */
export function onShown(run: Run, part: { names: string[] } | null): Run {
  if (run.finished) return run;
  let r = run;
  // What is shown has been read and parsed; the rest may still be to come.
  for (const k of ["engine", "read", "model"] as const) {
    const s = get(r, k);
    if (s && (s.state === "active" || s.state === "idle")) r = set(r, k, { state: (s.frac ?? 1) >= 1 ? "done" : "paused" });
  }
  if (part) return set(r, "view", { state: "paused", value: "in part" });
  // All of it: every step before the view is complete.
  for (const k of ["engine", "read", "model", "curves"] as const) if (get(r, k)) r = set(r, k, { state: "done", frac: get(r, k)!.frac === null ? null : 1 });
  return set(r, "view", { state: "done", value: "shown" });
}

/** The fingerprint's first and last groups, once it is known. */
export function onHash(run: Run, parity: string): Run {
  return set(run, "hash", { state: "done", value: parity ? `${parity.slice(0, 4)}…${parity.slice(-4)}` : "" });
}

/** The file is written: every station done. */
export function finishRun(run: Run): Run {
  const r = { ...run, finished: true };
  return { ...r, stations: r.stations.map((s) => ({ ...s, state: "done", frac: s.frac === null ? null : 1, value: s.key === "write" ? "done" : s.value })) };
}

/** One line for assistive tech and the tab title: what is running now. */
export function runLabel(run: Run): string {
  if (run.finished) return "Done";
  const active = run.stations.filter((s) => s.state === "active");
  return active.length ? active.map((s) => (s.value ? `${s.label} ${s.value}` : s.label)).join(", ") : "Waiting";
}
