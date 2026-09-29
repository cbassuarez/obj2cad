/// <reference lib="webworker" />
// Runs the Rust engine off the main thread. The worker keeps the parsed file, so a
// settings change only re-converts. Two engine modules: the small default one, and one
// with the DWG writer that is loaded the first time DWG is chosen.
import initCore, * as core from "./wasm/obj2cad_wasm.js";
import { geometryKey } from "@/lib/settings";
const post = (msg, transfer = []) => self.postMessage(msg, transfer);
// ---------------------------------------------------------------- engine modules
let panicMessage = null;
const modules = {};
function engine(kind) {
    const onPanic = (m) => {
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
                return m;
            });
    return modules[kind];
}
// Start compiling the default engine right away.
void engine("core").catch(() => undefined);
let open = null;
let dead = false;
async function sha256(data) {
    const digest = await crypto.subtle.digest("SHA-256", data);
    return Array.from(new Uint8Array(digest), (b) => b.toString(16).padStart(2, "0")).join("");
}
/** Read a file, reporting progress for big ones. */
async function read(file, progress) {
    if (file.size < 16 << 20)
        return new Uint8Array(await file.arrayBuffer());
    const out = new Uint8Array(file.size);
    const reader = file.stream().getReader();
    let at = 0;
    for (;;) {
        const { done, value } = await reader.read();
        if (done)
            break;
        if (at + value.length > out.length)
            throw new ReadError("The file changed while it was being read.");
        out.set(value, at);
        at += value.length;
        progress(at);
    }
    if (at !== out.length)
        throw new ReadError("The file changed while it was being read.");
    return out;
}
class ReadError extends Error {
}
async function load(id, sources, name, kind) {
    const mod = await engine(kind).catch((e) => {
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
        session.load(name, (done, all) => post({ id, type: "progress", progress: { stage: "parse", done, total: all } }));
    }
    catch (e) {
        session.free();
        throw e;
    }
    return { kind, session, sources, name, parity: new Map() };
}
function convert(o, settings, wantPreview) {
    return (async () => {
        const t0 = performance.now();
        const json = JSON.stringify(settings);
        const key = geometryKey(settings);
        let parity = o.parity.get(key);
        if (!parity) {
            parity = await sha256(o.session.parity_stream(json));
            o.parity.set(key, parity);
        }
        const chunks = [];
        const c = o.session.convert(json, parity, wantPreview, (chunk) => chunks.push(chunk));
        try {
            const preview = c.has_preview()
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
                timings: JSON.parse(c.timings()),
                preview,
                ms: performance.now() - t0,
            };
        }
        finally {
            c.free();
        }
    })();
}
function failure(err) {
    if (panicMessage !== null || err instanceof WebAssembly.RuntimeError) {
        dead = true;
        return { kind: "crash", message: panicMessage ?? String(err) };
    }
    if (err && typeof err === "object" && "kind" in err && "line" in err && "issues" in err)
        return { kind: "parse", parse: err };
    if (err instanceof Error && "engineFailed" in err)
        return { kind: "engine", message: err.message };
    if (err instanceof ReadError || (err instanceof DOMException && err.name === "NotReadableError"))
        return { kind: "read", message: err.message };
    return { kind: "other", message: err instanceof Error ? err.message : String(err) };
}
self.onmessage = async (e) => {
    const req = e.data;
    if (dead) {
        post({ id: req.id, type: "error", failure: { kind: "crash", message: panicMessage ?? "the engine stopped" } });
        return;
    }
    try {
        if (req.type === "open") {
            open = await load(req.id, req.sources, req.name, req.dwg ? "dwg" : "core");
            post({ id: req.id, type: "ok", result: JSON.parse(open.session.inspect()) });
        }
        else {
            if (!open)
                throw new Error("no file is open");
            // DWG needs the larger engine: move the open file into it once.
            if (req.settings.format === "dwg" && open.kind !== "dwg")
                open = await load(req.id, open.sources, open.name, "dwg");
            const r = await convert(open, req.settings, req.preview);
            const p = r.preview;
            const transfer = p
                ? [p.positions, p.colors, p.indices, p.edges, p.groups, p.lines, p.lineColors, p.lineGroups, p.points, p.pointColors, p.pointGroups].map((a) => a.buffer)
                : [];
            post({ id: req.id, type: "ok", result: r }, transfer);
        }
    }
    catch (err) {
        post({ id: req.id, type: "error", failure: failure(err) });
    }
};
