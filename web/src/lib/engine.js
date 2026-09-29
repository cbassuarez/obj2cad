export class EngineError extends Error {
    failure;
    constructor(failure) {
        super(failure.kind === "parse" ? failure.parse.message : failure.message);
        this.failure = failure;
    }
}
class Engine {
    worker;
    seq = 0;
    pending = new Map();
    /** The drawing the worker holds; `null` after a crash or before the first open. */
    loaded = null;
    queue = Promise.resolve();
    constructor() {
        this.start();
    }
    start() {
        this.worker = new Worker(new URL("../worker.ts", import.meta.url), { type: "module" });
        this.worker.onmessage = (e) => {
            const msg = e.data;
            const p = this.pending.get(msg.id);
            if (!p)
                return;
            if (msg.type === "progress") {
                p.progress?.(msg.progress);
                return;
            }
            this.pending.delete(msg.id);
            if (msg.type === "ok")
                p.resolve(msg.result);
            else {
                if (msg.failure.kind === "crash")
                    this.restart();
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
    failAll(failure) {
        this.loaded = null;
        for (const p of this.pending.values())
            p.reject(new EngineError(failure));
        this.pending.clear();
    }
    /** Replace the worker (after a crash). The open file is lost and must be opened again. */
    restart() {
        this.worker.terminate();
        this.failAll({ kind: "crash", message: "the engine was restarted" });
        this.start();
    }
    call(req, progress) {
        const id = ++this.seq;
        return new Promise((resolve, reject) => {
            this.pending.set(id, { resolve: resolve, reject, progress });
            this.worker.postMessage({ ...req, id });
        });
    }
    /** Run `task` after every call before it has finished, and before any call after it. */
    serial(task) {
        const run = this.queue.then(task, task);
        this.queue = run.catch(() => undefined);
        return run;
    }
    async load(d, dwg, progress) {
        this.loaded = null;
        const info = await this.call({ type: "open", sources: d.sources, name: d.name, dwg }, progress);
        this.loaded = d;
        return info;
    }
    /** Read the files of one drawing. Its `name` names it when it holds several models. */
    open(d, dwg, progress) {
        return this.serial(() => this.load(d, dwg, progress));
    }
    convert(d, settings, preview, progress) {
        return this.serial(async () => {
            if (this.loaded !== d)
                await this.load(d, settings.format === "dwg");
            const r = await this.call({ type: "convert", settings, preview }, progress);
            return {
                file: r.file,
                report: JSON.parse(r.report),
                decisions: JSON.parse(r.decisions),
                preview: r.preview,
                timings: r.timings,
                ms: r.ms,
            };
        });
    }
}
export const engine = new Engine();
/** SHA-256 of a blob, as hex (for the report, on demand). */
export async function sha256Hex(blob) {
    const digest = await crypto.subtle.digest("SHA-256", await blob.arrayBuffer());
    return Array.from(new Uint8Array(digest), (b) => b.toString(16).padStart(2, "0")).join("");
}
/** Nothing was written: no faces, lines, curves or points. */
export const isEmpty = (r) => r.output.faces + r.output.polylines + r.output.points + (r.output.splines ?? 0) === 0;
