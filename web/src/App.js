import { jsx as _jsx, jsxs as _jsxs } from "react/jsx-runtime";
import { lazy, Suspense, useCallback, useEffect, useRef, useState } from "react";
import { Dropzone } from "@mantine/dropzone";
import { notifications } from "@mantine/notifications";
import { useRegisterSW } from "virtual:pwa-register/react";
import { AnimatePresence, motion } from "motion/react";
import { CubeArt } from "@/components/brand";
import { BatchScreen } from "@/components/BatchScreen";
import { ChoiceScreen } from "@/components/ChoiceScreen";
import { DropScreen } from "@/components/DropScreen";
import { ErrorBoundary } from "@/components/ErrorBoundary";
import { FailedScreen } from "@/components/FailedScreen";
import { LoadingScreen, PreflightScreen } from "@/components/LoadingScreen";
import { TopBar } from "@/components/TopBar";
import { Button } from "@/components/ui/button";
import { engine, EngineError, isEmpty, sha256Hex } from "@/lib/engine";
import { CLI_URL, explain } from "@/lib/errors";
import { baseName, jobName, plan, saveFile, stem, zipFiles } from "@/lib/files";
import { AUTO, engineSettings, formatInfo, loadPrefs, previewKey, savePrefs, } from "@/lib/settings";
/** Above this, suggest the command-line version first. */
const BIG = 1_000_000_000;
// The 3D workspace (three.js) loads on demand, so the first screen is small and fast;
// it is prefetched as soon as the browser is idle.
const loadWorkspace = () => import("@/components/Workspace");
const Workspace = lazy(() => loadWorkspace().then((m) => ({ default: m.Workspace })));
const failureOf = (e) => (e instanceof EngineError ? e.failure : { kind: "other", message: e instanceof Error ? e.message : String(e) });
const jobSize = (job) => job.sources.reduce((n, s) => n + s.file.size, 0);
/** The file a failure is about: the one that couldn't be read, else the drawing. */
const failedName = (job, e) => (e instanceof EngineError && e.failure.kind === "parse" && e.failure.parse.file) || jobName(job);
export function App() {
    const [screen, setScreenState] = useState("empty");
    const [prefs, setPrefsState] = useState(loadPrefs);
    const [current, setCurrent] = useState(null);
    const [result, setResult] = useState(null);
    /** The drawing without the layers left out: what Download saves then. */
    const [visible, setVisible] = useState(null);
    const [preview, setPreview] = useState(null);
    const [busy, setBusy] = useState(null);
    const [loading, setLoading] = useState({ name: "", progress: null });
    const [failed, setFailed] = useState(null);
    const [preflight, setPreflight] = useState(null);
    const [choice, setChoice] = useState(null);
    const [batch, setBatch] = useState([]);
    const [downloaded, setDownloaded] = useState(null);
    const [watching, setWatching] = useState(false);
    const [zipping, setZipping] = useState(false);
    const input = useRef(null);
    const mtlInput = useRef(null);
    const folderInput = useRef(null);
    /** Choose a folder: every file in it (and its subfolders) makes one drawing. */
    const pickFolder = () => folderInput.current?.click();
    const screenRef = useRef("empty");
    const prefsRef = useRef(prefs);
    const currentRef = useRef(null);
    const resultRef = useRef(null);
    const visibleRef = useRef(null);
    const visibleToken = useRef(0);
    /** Names of the layers left out of the drawing (unticked in the layers pane). They
     *  survive a rebuilt preview (a reload, loose points) while those layers still exist. */
    const [hidden, setHiddenState] = useState([]);
    const hiddenRef = useRef([]);
    const setHidden = (names) => {
        hiddenRef.current = names;
        setHiddenState(names);
    };
    const batchRef = useRef([]);
    const shownPreview = useRef(null);
    const choiceRef = useRef(null);
    const convertToken = useRef(0);
    const openToken = useRef(0);
    const batchToken = useRef(0);
    const seq = useRef(0);
    prefsRef.current = prefs;
    resultRef.current = result;
    // ---------------------------------------------------------------- navigation
    // Each page is a history entry, so the browser's Back button goes back a step.
    const setScreen = useCallback((s, history = "none") => {
        screenRef.current = s;
        setScreenState(s);
        if (history === "push" && window.history.state?.screen !== s)
            window.history.pushState({ screen: s }, "");
        else if (history !== "none")
            window.history.replaceState({ screen: s }, "");
    }, []);
    useEffect(() => {
        if (!window.history.state?.screen)
            window.history.replaceState({ screen: "empty" }, "");
        const onPop = (e) => {
            const s = e.state?.screen ?? "empty";
            if (s === "work" && currentRef.current && resultRef.current)
                setScreen("work");
            else if (s === "batch" && batchRef.current.length)
                setScreen("batch");
            else if (s === "choose" && choiceRef.current)
                setScreen("choose");
            else
                setScreen("empty");
        };
        window.addEventListener("popstate", onPop);
        return () => window.removeEventListener("popstate", onPop);
    }, [setScreen]);
    const setPrefs = (update) => {
        const next = { ...prefsRef.current, ...update };
        prefsRef.current = next;
        setPrefsState(next);
        savePrefs(next);
    };
    const setCur = (cur) => {
        currentRef.current = cur;
        setCurrent(cur);
    };
    const updateBatch = (id, patch) => {
        batchRef.current = batchRef.current.map((i) => (i.id === id ? { ...i, ...patch } : i));
        setBatch(batchRef.current);
    };
    // ---------------------------------------------------------------- updates (PWA)
    const { offlineReady: [offlineReady], needRefresh: [needRefresh], updateServiceWorker, } = useRegisterSW();
    useEffect(() => {
        if (!needRefresh)
            return;
        // Never reload on our own: the user may be mid-conversion.
        notifications.show({
            id: "update",
            autoClose: false,
            withCloseButton: true,
            title: "A new version is ready",
            message: (_jsx("div", { className: "mt-2", children: _jsx(Button, { size: "sm", variant: "primary", onClick: () => updateServiceWorker(true), children: "Reload" }) })),
        });
    }, [needRefresh, updateServiceWorker]);
    // ---------------------------------------------------------------- failures
    const fail = (name, e, retry) => {
        const failure = failureOf(e);
        const retryable = failure.kind === "crash" || failure.kind === "read" || failure.kind === "other";
        setFailed({ name, explained: explain(failure), retry: retryable ? retry : undefined });
        setScreen("failed", screenRef.current === "empty" ? "push" : "replace");
    };
    // ---------------------------------------------------------------- conversion
    /** Convert the open file with the current preferences and its choices. */
    const convert = useCallback(async (cur, forcePreview = false, label = "Converting…") => {
        const token = ++convertToken.current;
        const settings = engineSettings(prefsRef.current, cur.choices);
        const key = previewKey(settings);
        const shown = shownPreview.current;
        const wantPreview = forcePreview || !shown || shown.key !== key || shown.fileId !== cur.id;
        // Until the conversion below finishes, no visible-layers drawing is current.
        ++visibleToken.current;
        visibleRef.current = null;
        setVisible(null);
        setBusy(label);
        setDownloaded(null);
        try {
            const r = await engine.convert(cur.job, settings, wantPreview, () => setBusy(settings.format === "dwg" ? "Preparing DWG…" : label));
            if (token !== convertToken.current)
                return false;
            if (isEmpty(r.report)) {
                setFailed({ name: cur.inspection.name, explained: explain({ kind: "empty" }) });
                setScreen("failed", screenRef.current === "empty" ? "push" : "replace");
                return false;
            }
            resultRef.current = r;
            setResult(r);
            if (r.preview) {
                shownPreview.current = { key, fileId: cur.id };
                setPreview({ buffers: r.preview, builtUp: r.decisions.up_axis, id: ++seq.current, fileId: cur.id });
            }
            if (cur.batchItem !== null)
                updateBatch(cur.batchItem, { status: "done", result: r });
            // Keep the layers left out that still exist (a reload can remove some).
            const names = new Set(r.report.layers.filter((l) => l.faces + l.polylines + l.points + l.surfaces > 0).map((l) => l.name));
            const kept = hiddenRef.current.filter((n) => names.has(n));
            if (kept.length !== hiddenRef.current.length)
                setHidden(kept);
            if (kept.length)
                await convertVisible(cur);
            return true;
        }
        catch (e) {
            if (token !== convertToken.current)
                return false;
            fail(cur.inspection.name, e, () => void openJob(cur.job, cur.handle, { choices: cur.choices, batchItem: cur.batchItem, force: true }));
            return false;
        }
        finally {
            if (token === convertToken.current)
                setBusy(null);
        }
        // Stable: everything it reads is a ref or a state setter (openJob is only called later).
    }, []);
    /** Convert the drawing without the layers left out, so everything shown describes the download. */
    const convertVisible = useCallback(async (cur) => {
        const token = ++visibleToken.current;
        visibleRef.current = null;
        setVisible(null);
        const names = hiddenRef.current;
        const full = resultRef.current;
        const layers = full ? full.report.layers.filter((l) => l.faces + l.polylines + l.points + l.surfaces > 0).length : 0;
        if (!names.length || names.length >= layers)
            return; // nothing left out, or nothing left to download
        setBusy("Converting…");
        try {
            const r = await engine.convert(cur.job, engineSettings(prefsRef.current, cur.choices, names), false);
            if (token !== visibleToken.current)
                return;
            visibleRef.current = r;
            setVisible(r);
        }
        catch (e) {
            if (token === visibleToken.current)
                notifications.show({ color: "red", title: "Couldn't convert the visible layers", message: e.message });
        }
        finally {
            if (token === visibleToken.current)
                setBusy(null);
        }
    }, []);
    /** Open one drawing into the workspace. */
    const openJob = useCallback(async (job, handle, opts = {}) => {
        const name = jobName(job);
        const size = jobSize(job);
        if (size > BIG && !opts.force) {
            setPreflight({ name, size, go: () => void openJob(job, handle, { ...opts, force: true }) });
            setScreen("preflight");
            return;
        }
        const token = ++openToken.current;
        const previous = screenRef.current === "work" ? currentRef.current?.inspection.name : undefined;
        setLoading({ name, progress: null });
        if (!opts.reload) {
            setScreen("loading");
            setHidden([]); // another drawing: every layer in again
        }
        else
            setBusy("Reloading…");
        try {
            const info = await engine.open(job, prefsRef.current.format === "dwg", (p) => setLoading({ name, progress: p }));
            if (token !== openToken.current)
                return;
            const cur = { id: ++seq.current, job, handle, inspection: info, choices: opts.choices ?? AUTO, batchItem: opts.batchItem ?? null };
            setCur(cur);
            if (!(await convert(cur, true)) || token !== openToken.current)
                return;
            setScreen("work", opts.reload || screenRef.current === "work" ? "replace" : "push");
            if (!opts.reload && previous && previous !== info.name)
                notifications.show({ message: `Replaced ${previous} with ${info.name}` });
            if (opts.reload)
                notifications.show({ message: `Reloaded ${info.name}` });
        }
        catch (e) {
            if (token === openToken.current)
                fail(failedName(job, e), e, () => void openJob(job, handle, { ...opts, force: true }));
        }
        finally {
            if (opts.reload)
                setBusy(null);
        }
    }, [convert, setScreen]);
    // ---------------------------------------------------------------- batch
    const runBatch = useCallback(async (items) => {
        const token = ++batchToken.current;
        for (const item of items) {
            if (token !== batchToken.current)
                return;
            updateBatch(item.id, { status: "converting", result: undefined, error: undefined });
            try {
                await engine.open(item.job, prefsRef.current.format === "dwg");
                const r = await engine.convert(item.job, engineSettings(prefsRef.current, AUTO), false);
                if (isEmpty(r.report))
                    updateBatch(item.id, { status: "failed", error: explain({ kind: "empty" }) });
                else
                    updateBatch(item.id, { status: "done", result: r });
            }
            catch (e) {
                updateBatch(item.id, { status: "failed", error: explain(failureOf(e)) });
            }
        }
    }, []);
    const startBatch = (jobs) => {
        batchRef.current = jobs.map((job) => ({ id: ++seq.current, job, name: jobName(job), size: jobSize(job), status: "waiting" }));
        setBatch(batchRef.current);
        setCur(null);
        shownPreview.current = null;
        setScreen("batch", screenRef.current === "choose" ? "replace" : "push");
        void runBatch(batchRef.current);
    };
    // ---------------------------------------------------------------- opening files
    const openFiles = useCallback(async (files, handles = []) => {
        let p;
        try {
            p = await plan(files);
        }
        catch (e) {
            notifications.show({ color: "red", title: "Couldn't open the .zip", message: e.message });
            return;
        }
        if (p.kind === "none") {
            notifications.show({ title: "Nothing to convert", message: p.ignored.length ? `Not used: ${p.ignored.slice(0, 3).join(", ")}` : "Choose an .obj, an .xyz or a .zip." });
            return;
        }
        if (p.kind === "materials") {
            if (screenRef.current === "work" && currentRef.current)
                return addSources(p.mtls);
            notifications.show({ title: "No model", message: "Choose an .obj, an .xyz or a .zip." });
            return;
        }
        batchToken.current++; // a new drop stops a running batch
        setWatching(false);
        if (p.kind === "one") {
            const only = p.job.sources.length === 1 ? p.job.sources[0] : null;
            const handle = only ? (handles.find((h) => h.name === only.file.name) ?? null) : null;
            return openJob(p.job, handle);
        }
        if (p.kind === "choose") {
            choiceRef.current = { combined: p.combined, separate: p.separate };
            setChoice(choiceRef.current);
            setScreen("choose", "push");
            return;
        }
        startBatch(p.jobs);
    }, [openJob]);
    /** Add files (material libraries) to the open drawing and read it again. */
    const addSources = async (extra) => {
        const cur = currentRef.current;
        if (!cur)
            return;
        const names = new Set(extra.map((s) => baseName(s.path).toLowerCase()));
        const job = { ...cur.job, sources: [...cur.job.sources.filter((s) => !names.has(baseName(s.path).toLowerCase())), ...extra] };
        await openJob(job, cur.handle, { reload: true, force: true, choices: cur.choices, batchItem: cur.batchItem });
    };
    /** Choose files: the browser's file picker (with handles, for reloading) or the input. */
    const pick = useCallback(async () => {
        const w = window;
        if (!w.showOpenFilePicker)
            return input.current?.click();
        let handles;
        try {
            handles = await w.showOpenFilePicker({ multiple: true, types: [{ description: "3D models", accept: { "application/octet-stream": [".obj", ".xyz", ".mtl", ".jpg", ".jpeg", ".png", ".zip"] } }] });
        }
        catch (e) {
            if (!(e instanceof DOMException && e.name === "AbortError"))
                input.current?.click();
            return;
        }
        await openFiles(await Promise.all(handles.map((h) => h.getFile())), handles);
    }, [openFiles]);
    // ---------------------------------------------------------------- settings
    const change = (choices) => {
        const cur = currentRef.current;
        if (!cur)
            return;
        const next = { ...cur, choices: { ...cur.choices, ...choices } };
        setCur(next);
        void convert(next);
    };
    const changePrefs = (update) => {
        setPrefs(update);
        const cur = currentRef.current;
        if (cur && screenRef.current === "work")
            void convert(cur);
    };
    // ---------------------------------------------------------------- downloads
    const ext = () => formatInfo(prefsRef.current.format).ext;
    const save = async (blob, name, pickLocation = false) => {
        try {
            const saved = await saveFile(blob, name, pickLocation);
            if (saved)
                setDownloaded(saved);
        }
        catch (e) {
            notifications.show({ color: "red", title: "Couldn't save the file", message: e.message });
        }
    };
    /** What Download saves: the drawing, or without the layers left out when some are. */
    const downloadable = () => {
        const cur = currentRef.current;
        const partial = hiddenRef.current.length > 0;
        const r = partial ? visibleRef.current : resultRef.current;
        return cur && r ? { r, name: `${stem(cur.inspection.name)}${partial ? " (visible layers)" : ""}` } : null;
    };
    const download = (pickLocation) => {
        const d = downloadable();
        if (d && !busy)
            void save(d.r.file, `${d.name}.${ext()}`, pickLocation);
    };
    const changeHidden = (names) => {
        setHidden(names);
        setDownloaded(null);
        const cur = currentRef.current;
        if (cur)
            void convertVisible(cur);
    };
    /** The report of exactly what Download saves. */
    const downloadReport = async () => {
        const d = downloadable();
        if (!d)
            return;
        const { r } = d;
        const report = { ...r.report, output: { ...r.report.output, sha256: await sha256Hex(r.file) } };
        const saved = await saveFile(new Blob([JSON.stringify(report, null, 2)], { type: "application/json" }), `${d.name}.report.json`).catch(() => null);
        if (saved)
            notifications.show({ message: `Saved ${saved}` });
    };
    const downloadAll = async () => {
        const done = batchRef.current.filter((i) => i.status === "done" && i.result);
        setZipping(true);
        try {
            const names = new Set();
            const files = done.map((i) => {
                let name = `${stem(i.name)}.${ext()}`;
                for (let n = 2; names.has(name.toLowerCase()); n++)
                    name = `${stem(i.name)} (${n}).${ext()}`;
                names.add(name.toLowerCase());
                return { name, blob: i.result.file };
            });
            await save(await zipFiles(files), `obj2cad ${done.length} files.zip`);
        }
        catch (e) {
            notifications.show({ color: "red", title: "Couldn't make the .zip", message: e.message });
        }
        finally {
            setZipping(false);
        }
    };
    // ---------------------------------------------------------------- watching
    useEffect(() => {
        if (!watching || !current?.handle)
            return;
        const timer = setInterval(async () => {
            const cur = currentRef.current;
            if (!cur?.handle || screenRef.current !== "work")
                return;
            try {
                const f = await cur.handle.getFile();
                const old = cur.job.sources[0].file;
                if (f.lastModified !== old.lastModified || f.size !== old.size) {
                    const job = { ...cur.job, sources: [{ file: f, path: cur.job.sources[0].path }, ...cur.job.sources.slice(1)] };
                    await openJob(job, cur.handle, { reload: true, force: true, choices: cur.choices, batchItem: cur.batchItem });
                }
            }
            catch {
                setWatching(false); // file moved or permission withdrawn
            }
        }, 2000);
        return () => clearInterval(timer);
    }, [watching, current?.handle, openJob]);
    // ---------------------------------------------------------------- global keys
    useEffect(() => {
        const idle = window.requestIdleCallback ?? ((cb) => setTimeout(cb, 1500));
        idle(() => void loadWorkspace());
    }, []);
    useEffect(() => {
        const onKey = (e) => {
            if (!(e.ctrlKey || e.metaKey))
                return;
            // Ctrl/⌘+S is the workspace's: it knows which layers are left out.
            if (e.key.toLowerCase() === "o") {
                e.preventDefault();
                void pick();
            }
        };
        window.addEventListener("keydown", onKey);
        return () => window.removeEventListener("keydown", onKey);
    });
    // ---------------------------------------------------------------- render
    const inBatch = current?.batchItem != null && batch.length > 0;
    return (_jsxs("div", { className: "relative h-dvh overflow-hidden", children: [_jsx(TopBar, { file: screen === "work" && current
                    ? {
                        name: current.inspection.name,
                        size: jobSize(current.job),
                        exporter: current.inspection.hints.exporter,
                        files: current.inspection.files.filter((f) => f.role !== "missing" && f.role !== "not_used").length,
                    }
                    : null, offlineReady: offlineReady, onOpen: () => void pick(), onOpenFolder: pickFolder, onBack: screen === "work" && inBatch ? () => window.history.back() : undefined, backLabel: "All files", watch: screen === "work" && current?.handle ? watching : null, onWatch: setWatching }), screen === "empty" && _jsx(DropScreen, { onPick: () => void pick(), onPickFolder: pickFolder }), screen === "preflight" && preflight && (_jsx(PreflightScreen, { name: preflight.name, size: preflight.size, cliUrl: CLI_URL, onContinue: preflight.go, onCancel: () => setScreen(currentRef.current && resultRef.current ? "work" : "empty") })), screen === "loading" && _jsx(LoadingScreen, { name: loading.name, progress: loading.progress }), screen === "failed" && failed && _jsx(FailedScreen, { name: failed.name, explained: failed.explained, onPick: () => void pick(), onRetry: failed.retry }), screen === "choose" && choice && (_jsx(ChoiceScreen, { separate: choice.separate, onCombine: () => void openJob(choice.combined, null), onSeparate: () => startBatch(choice.separate) })), screen === "batch" && (_jsx(BatchScreen, { items: batch, format: prefs.format, zipping: zipping, onFormat: (format) => {
                    setPrefs({ format });
                    void runBatch(batchRef.current);
                }, onOpen: (item) => void openJob(item.job, null, { batchItem: item.id }), onDownload: (item) => item.result && void save(item.result.file, `${stem(item.name)}.${ext()}`), onDownloadAll: () => void downloadAll() })), screen === "work" && result && (_jsx(ErrorBoundary, { children: _jsx(Suspense, { fallback: _jsx("main", { className: "h-dvh bg-viewport" }), children: _jsx(Workspace, { result: result, visible: visible, hidden: hidden, preview: preview, inspection: current?.inspection ?? null, prefs: prefs, busy: busy, downloaded: downloaded, onUp: (up) => change({ up }), onUnits: (units) => change({ units }), onHouseUnits: (houseUnits) => changePrefs({ houseUnits }), onShowIn: (showIn) => setPrefs({ showIn }), onKeepLoose: (keepLoose) => change({ keepLoose }), onLayerMode: (layerMode) => {
                            setHidden([]); // other layers: nothing is left out any more
                            changePrefs({ layerMode });
                        }, onFormat: (format) => changePrefs({ format }), onIncludeName: (includeName) => changePrefs({ includeName }), onCurves: (curves) => changePrefs({ curves }), onDownload: download, onHidden: changeHidden, onDownloadReport: () => void downloadReport(), onAddMtl: () => mtlInput.current?.click(), onAnother: () => void pick() }) }) })), _jsx(Dropzone.FullScreen, { onDrop: (files) => void openFiles(files), activateOnClick: false, multiple: true, zIndex: 400, classNames: { fullScreen: "!bg-[var(--backdrop)] backdrop-blur-sm", root: "!h-full !border-0 !bg-transparent !p-0", inner: "!h-full" }, children: _jsx(AnimatePresence, { children: _jsx(motion.div, { initial: { scale: 0.97, opacity: 0 }, animate: { scale: 1, opacity: 1 }, className: "flex h-full items-center justify-center p-8", children: _jsxs("div", { className: "panel flex flex-col items-center gap-4 border-2 border-dashed !border-accent px-12 py-12 text-center", children: [_jsx(CubeArt, { className: "size-16 text-accent" }), _jsx("div", { className: "font-display text-[32px] font-semibold tracking-tight", children: "Drop to open" }), _jsx("div", { className: "text-[14px] text-fg-3", children: ".obj, .xyz, .mtl, images, a .zip or a folder" })] }) }) }) }), _jsx("input", { ref: mtlInput, type: "file", accept: ".mtl", hidden: true, onChange: (e) => {
                    const f = e.target.files?.[0];
                    e.target.value = "";
                    if (f)
                        void addSources([{ file: f, path: f.name }]);
                } }), _jsx("input", { ref: folderInput, type: "file", webkitdirectory: "", directory: "", multiple: true, hidden: true, "aria-label": "Choose a folder", onChange: (e) => {
                    const files = Array.from(e.target.files ?? []);
                    e.target.value = "";
                    if (files.length)
                        void openFiles(files);
                } }), _jsx("input", { ref: input, type: "file", multiple: true, accept: ".obj,.xyz,.mtl,.jpg,.jpeg,.png,.zip", hidden: true, onChange: (e) => {
                    const files = Array.from(e.target.files ?? []);
                    e.target.value = "";
                    if (files.length)
                        void openFiles(files);
                } })] }));
}
