// Points of a file being read, from the engine to the viewer (large arrays: never React
// state). Batches that arrive before the viewer is there wait for it.
import type { PointBatch } from "@/lib/engine";

type Listener = (batch: PointBatch | null) => void;

let listener: Listener | null = null;
let backlog: PointBatch[] = [];

export const pointFeed = {
  push(batch: PointBatch) {
    if (listener) listener(batch);
    else backlog.push(batch);
  },
  /** Another drawing, or opening was stopped: the points shown so far go. */
  clear() {
    backlog = [];
    listener?.(null);
  },
  /** The viewer: receives the waiting batches, then each new one (`null`: clear). */
  listen(fn: Listener): () => void {
    listener = fn;
    for (const b of backlog) fn(b);
    backlog = [];
    return () => {
      if (listener === fn) listener = null;
    };
  },
};
