import { lazy, Suspense, useCallback, useEffect, useRef, useState } from "react";
import { Dropzone } from "@mantine/dropzone";
import { notifications } from "@mantine/notifications";
import { useRegisterSW } from "virtual:pwa-register/react";
import { AnimatePresence, motion } from "motion/react";
import { CubeArt } from "@/components/brand";
import { BatchScreen, type BatchItem } from "@/components/BatchScreen";
import { ChoiceScreen } from "@/components/ChoiceScreen";
import { DropScreen } from "@/components/DropScreen";
import { ErrorBoundary } from "@/components/ErrorBoundary";
import { FailedScreen } from "@/components/FailedScreen";
import { PreflightScreen } from "@/components/PreflightScreen";
import { LoadRail } from "@/components/LoadRail";
import { TopBar } from "@/components/TopBar";
import { WorkspaceSkeleton } from "@/components/WorkspaceSkeleton";
import { Button } from "@/components/ui/button";
import type { PreviewState } from "@/components/Workspace";
import { engine, EngineError, isEmpty, OutputFile, RESTARTED, sha256Hex, type Failure, type Inspection, type Progress, type Result } from "@/lib/engine";
import { explain, type Explained } from "@/lib/errors";
import { fmt } from "@/lib/format";
import { pointFeed } from "@/lib/pointFeed";
import { finishRun, onHash, onProgress, onShown, runLabel, startRun, type Run } from "@/lib/run";
import { baseName, heavySplit, jobName, plan, saveFile, sourceSize, stem, zipFiles, type Job, type Source } from "@/lib/files";
import {
  AUTO,
  engineSettings,
  formatInfo,
  loadPrefs,
  previewKey,
  savePrefs,
  type FileChoices,
  type Format,
  type LayerMode,
  type Prefs,
  type UpAxis,
  type Units,
} from "@/lib/settings";

type Screen = "empty" | "preflight" | "loading" | "failed" | "work" | "batch" | "choose";

/** A file handle from the File System Access API (Chromium), for reloading on change. */
interface FileHandleLike {
  name: string;
  getFile(): Promise<File>;
}

interface Current {
  id: number;
  job: Job;
  /** For a drawing made from one file picked with the file picker: reload on change. */
  handle: FileHandleLike | null;
  inspection: Inspection;
  choices: FileChoices;
  /** Opened from the file list. */
  batchItem: number | null;
}

interface OpenOptions {
  force?: boolean;
  reload?: boolean;
  choices?: FileChoices;
  batchItem?: number | null;
}

/** Above this, suggest the command-line version first. */
const BIG = 1_000_000_000;

/** The busy label for a step of a conversion shown in the workspace (the file is being
 *  written while its model is shown, or a setting changed). */
const busyLabel = (stage: Progress["stage"], format: Format): string =>
  ({
    engine: "Loading the DWG writer…",
    read: "Reading the files…",
    parse: "Reading the model…",
    curves: "Finding curved surfaces…",
    preview: "Updating the view…",
    hash: "Fingerprinting the geometry…",
    write: `Writing the ${format === "dwg" ? "DWG" : "DXF"}…`,
  })[stage];

// The 3D workspace (three.js) loads on demand, so the first screen is small and fast;
// it is prefetched as soon as the browser is idle.
const loadWorkspace = () => import("@/components/Workspace");
const Workspace = lazy(() => loadWorkspace().then((m) => ({ default: m.Workspace })));

const failureOf = (e: unknown): Failure => (e instanceof EngineError ? e.failure : { kind: "other", message: e instanceof Error ? e.message : String(e) });
const jobSize = (job: Job) => job.sources.reduce((n, s) => n + sourceSize(s), 0);
/** What the engine parses: the models and point clouds. */
const geometrySize = (job: Job) => job.sources.reduce((n, s) => n + (/\.(obj|xyz)$/i.test(s.path) ? sourceSize(s) : 0), 0);
/** How long the rail stays, all done, once the file is written. */
const RAIL_LINGER_MS = 1400;
/** The file a failure is about: the one that couldn't be read, else the drawing. */
const failedName = (job: Job, e: unknown) => (e instanceof EngineError && e.failure.kind === "parse" && e.failure.parse.file) || jobName(job);

export function App() {
  const [screen, setScreenState] = useState<Screen>("empty");
  const [prefs, setPrefsState] = useState<Prefs>(loadPrefs);
  const [current, setCurrent] = useState<Current | null>(null);
  const [result, setResult] = useState<Result | null>(null);
  /** The drawing without the layers left out: what Download saves then. */
  const [visible, setVisible] = useState<Result | null>(null);
  const [preview, setPreview] = useState<PreviewState | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  /** Files of the open drawing still loading (a large scan, shown after the rest). */
  const [pending, setPendingState] = useState<string[]>([]);
  const pendingRef = useRef<string[]>([]);
  const setPending = (names: string[]) => {
    pendingRef.current = names;
    setPendingState(names);
  };
  /** The drawing being opened. */
  const [loading, setLoading] = useState<{ name: string }>({ name: "" });
  /** Opening, step by step (the station rail); null when nothing is being opened. Updated
   *  at most once a frame: reading reports every few hundred kilobytes. */
  const [run, setRunState] = useState<Run | null>(null);
  const runRef = useRef<Run | null>(null);
  const runFrame = useRef(0);
  const setRun = (r: Run | null) => {
    runRef.current = r;
    cancelAnimationFrame(runFrame.current);
    runFrame.current = 0;
    setRunState(r);
  };
  const updateRun = (f: (r: Run) => Run) => {
    if (!runRef.current) return;
    runRef.current = f(runRef.current);
    runFrame.current ||= requestAnimationFrame(() => {
      runFrame.current = 0;
      setRunState(runRef.current);
    });
  };
  /** The file being written: elements written of all. */
  const [write, setWrite] = useState<{ done: number; total: number } | null>(null);
  const [failed, setFailed] = useState<{ name: string; explained: Explained; job?: Job; retry?: () => void } | null>(null);
  const [preflight, setPreflight] = useState<{ name: string; size: number; job: Job; go: () => void } | null>(null);
  const [choice, setChoice] = useState<{ combined: Job; separate: Job[] } | null>(null);
  const [batch, setBatch] = useState<BatchItem[]>([]);
  const [downloaded, setDownloaded] = useState<string | null>(null);
  const [watching, setWatching] = useState(false);
  const [zipping, setZipping] = useState(false);

  const input = useRef<HTMLInputElement>(null);
  const mtlInput = useRef<HTMLInputElement>(null);
  const folderInput = useRef<HTMLInputElement>(null);
  /** Choose a folder: every file in it (and its subfolders) makes one drawing. */
  const pickFolder = () => folderInput.current?.click();
  const screenRef = useRef<Screen>("empty");
  const prefsRef = useRef(prefs);
  const currentRef = useRef<Current | null>(null);
  const resultRef = useRef<Result | null>(null);
  const visibleRef = useRef<Result | null>(null);
  const visibleToken = useRef(0);
  /** Names of the layers left out of the drawing (unticked in the layers pane). They
   *  survive a rebuilt preview (a reload, loose points) while those layers still exist. */
  const [hidden, setHiddenState] = useState<string[]>([]);
  const hiddenRef = useRef<string[]>([]);
  const setHidden = (names: string[]) => {
    hiddenRef.current = names;
    setHiddenState(names);
  };
  const batchRef = useRef<BatchItem[]>([]);
  const shownPreview = useRef<{ key: string; fileId: number } | null>(null);
  const choiceRef = useRef<{ combined: Job; separate: Job[] } | null>(null);
  const convertToken = useRef(0);
  const openToken = useRef(0);
  const batchToken = useRef(0);
  const seq = useRef(0);
  prefsRef.current = prefs;
  resultRef.current = result;

  // ---------------------------------------------------------------- navigation
  // Each page is a history entry, so the browser's Back button goes back a step.
  const setScreen = useCallback((s: Screen, history: "push" | "replace" | "none" = "none") => {
    screenRef.current = s;
    setScreenState(s);
    if (history === "push" && window.history.state?.screen !== s) window.history.pushState({ screen: s }, "");
    else if (history !== "none") window.history.replaceState({ screen: s }, "");
  }, []);

  useEffect(() => {
    if (!window.history.state?.screen) window.history.replaceState({ screen: "empty" }, "");
    const onPop = (e: PopStateEvent) => {
      const s = (e.state?.screen as Screen | undefined) ?? "empty";
      if (s === "work" && currentRef.current && resultRef.current) setScreen("work");
      else if (s === "batch" && batchRef.current.length) setScreen("batch");
      else if (s === "choose" && choiceRef.current) setScreen("choose");
      else setScreen("empty");
    };
    window.addEventListener("popstate", onPop);
    return () => window.removeEventListener("popstate", onPop);
  }, [setScreen]);

  const setPrefs = (update: Partial<Prefs>) => {
    const next = { ...prefsRef.current, ...update };
    prefsRef.current = next;
    setPrefsState(next);
    savePrefs(next);
  };

  const setCur = (cur: Current | null) => {
    currentRef.current = cur;
    setCurrent(cur);
  };

  const updateBatch = (id: number, patch: Partial<BatchItem>) => {
    batchRef.current = batchRef.current.map((i) => (i.id === id ? { ...i, ...patch } : i));
    setBatch(batchRef.current);
  };

  // ---------------------------------------------------------------- updates (PWA)
  const {
    offlineReady: [offlineReady],
    needRefresh: [needRefresh],
    updateServiceWorker,
  } = useRegisterSW();
  useEffect(() => {
    if (!needRefresh) return;
    // Never reload on our own: the user may be mid-conversion.
    notifications.show({
      id: "update",
      autoClose: false,
      withCloseButton: true,
      title: "A new version is ready",
      message: (
        <div className="mt-2">
          <Button size="sm" variant="primary" onClick={() => updateServiceWorker(true)}>
            Reload
          </Button>
        </div>
      ),
    });
  }, [needRefresh, updateServiceWorker]);

  // ---------------------------------------------------------------- failures
  const fail = (name: string, e: unknown, retry?: () => void, job?: Job) => {
    setRun(null);
    pointFeed.clear();
    const failure = failureOf(e);
    const retryable = failure.kind === "crash" || failure.kind === "read" || failure.kind === "other";
    setFailed({ name, explained: explain(failure), job, retry: retryable ? retry : undefined });
    setScreen("failed", screenRef.current === "empty" ? "push" : "replace");
  };

  // ---------------------------------------------------------------- conversion
  /** Convert the open file with the current preferences and its choices. */
  const convert = useCallback(async (cur: Current, forcePreview = false, label = "Converting…"): Promise<boolean> => {
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
      const r = await engine.convert(
        cur.job,
        settings,
        wantPreview,
        (p) => {
          // Opening: the rail shows each step. The busy label says it too (and keeps the
          // download waiting).
          updateRun((r) => onProgress(r, p));
          if (p.stage === "write") setWrite({ done: p.done, total: p.total });
          if (screenRef.current !== "loading") setBusy(busyLabel(p.stage, settings.format));
        },
        // The model is shown as soon as it can be, while its file is still being written:
        // the result is provisional (Download waits, busy) until the file arrives.
        (e) => {
          if (token !== convertToken.current || isEmpty(e.report)) return;
          const provisional: Result = { file: new OutputFile([]), report: e.report, decisions: e.decisions, preview: null, timings: {}, ms: 0 };
          resultRef.current = provisional;
          setResult(provisional);
          shownPreview.current = { key, fileId: cur.id };
          setPreview({ buffers: e.preview, builtUp: e.decisions.up_axis, id: ++seq.current, fileId: cur.id });
          setBusy(busyLabel("hash", settings.format));
          setPending([]);
          updateRun((r) => onShown(r, null));
          if (screenRef.current === "loading") setScreen("work", "push");
        },
      );
      if (token !== convertToken.current) return false;
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
      if (cur.batchItem !== null) updateBatch(cur.batchItem, { status: "done", result: r });
      // Keep the layers left out that still exist (a reload can remove some).
      const names = new Set(r.report.layers.filter((l) => l.faces + l.polylines + l.points + l.surfaces > 0).map((l) => l.name));
      const kept = hiddenRef.current.filter((n) => names.has(n));
      if (kept.length !== hiddenRef.current.length) setHidden(kept);
      if (kept.length) await convertVisible(cur);
      return true;
    } catch (e) {
      if (token !== convertToken.current) return false;
      fail(cur.inspection.name, e, () => void openJob(cur.job, cur.handle, { choices: cur.choices, batchItem: cur.batchItem, force: true }), cur.job);
      return false;
    } finally {
      if (token === convertToken.current) {
        setBusy(null);
        setWrite(null);
      }
    }
    // Stable: everything it reads is a ref or a state setter (openJob is only called later).
  }, []);

  /** Convert the drawing without the layers left out, so everything shown describes the download. */
  const convertVisible = useCallback(async (cur: Current) => {
    const token = ++visibleToken.current;
    visibleRef.current = null;
    setVisible(null);
    const names = hiddenRef.current;
    const full = resultRef.current;
    const layers = full ? full.report.layers.filter((l) => l.faces + l.polylines + l.points + l.surfaces > 0).length : 0;
    if (!names.length || names.length >= layers) return; // nothing left out, or nothing left to download
    setBusy("Converting…");
    try {
      const r = await engine.convert(cur.job, engineSettings(prefsRef.current, cur.choices, names), false);
      if (token !== visibleToken.current) return;
      visibleRef.current = r;
      setVisible(r);
    } catch (e) {
      if (token === visibleToken.current) notifications.show({ color: "red", title: "Couldn't convert the visible layers", message: (e as Error).message });
    } finally {
      if (token === visibleToken.current) setBusy(null);
    }
  }, []);

  /** Open one drawing into the workspace. */
  const openJob = useCallback(
    async (job: Job, handle: FileHandleLike | null, opts: OpenOptions = {}): Promise<void> => {
      const name = jobName(job);
      const size = jobSize(job);
      if (size > BIG && !opts.force) {
        setPreflight({ name, size, job, go: () => void openJob(job, handle, { ...opts, force: true }) });
        setScreen("preflight");
        return;
      }
      const token = ++openToken.current;
      const previous = screenRef.current === "work" ? currentRef.current?.inspection.name : undefined;
      setLoading({ name });
      pointFeed.clear();
      setRun(startRun({ total: size, geometry: geometrySize(job), format: prefsRef.current.format === "dwg" ? "DWG" : "DXF", curves: prefsRef.current.curves }));
      if (!opts.reload) {
        setScreen("loading");
        setHidden([]); // another drawing: every layer in again
      } else setBusy("Reloading…");
      // Another drawing: work still running for the one before is dropped, not waited for.
      ++convertToken.current;
      ++visibleToken.current;
      const dwg = prefsRef.current.format === "dwg";
      // A large point cloud next to lighter models: show the models first, while the
      // cloud loads. The file is still written once, from everything, after that.
      const split = heavySplit(job);
      const heavyNames = split ? split.heavy.map((h) => baseName(h.path)) : [];
      const toRun = (p: Progress) => updateRun((r) => onProgress(r, p));
      const toScene = (b: Parameters<typeof pointFeed.push>[0]) => {
        if (token === openToken.current) pointFeed.push(b);
      };
      try {
        if (split) {
          const light = await engine.open(split.light, dwg, toRun, true, toScene);
          if (token !== openToken.current) return;
          // Named and sized as the whole drawing (the top bar, adding materials), shown in part.
          const lightCur: Current = {
            id: ++seq.current,
            job,
            handle,
            inspection: { ...light, name: jobName(job) },
            choices: opts.choices ?? AUTO,
            batchItem: opts.batchItem ?? null,
          };
          // A scan still to come: the model is shown as exported. Scanners write Z up, and a
          // model that comes with a scan shares its coordinates; the scan's points are shown
          // in this frame as they are read. The drawing's own decision, from everything,
          // settles it once all of it is read.
          const scanToCome = !lightCur.choices.up && split.heavy.some((h) => /\.xyz$/i.test(h.path));
          const e = await engine.preview(split.light, engineSettings(prefsRef.current, scanToCome ? { ...lightCur.choices, up: "as_is" } : lightCur.choices));
          if (scanToCome) e.decisions = { ...e.decisions, up_from: "detected" };
          if (token !== openToken.current) return;
          if (!isEmpty(e.report)) {
            setCur(lightCur);
            const provisional: Result = { file: new OutputFile([]), report: e.report, decisions: e.decisions, preview: null, timings: {}, ms: 0 };
            resultRef.current = provisional;
            setResult(provisional);
            shownPreview.current = null; // the full drawing brings its own
            setPreview({ buffers: e.preview, builtUp: e.decisions.up_axis, id: ++seq.current, fileId: lightCur.id });
            setPending(heavyNames);
            setBusy(`Loading ${heavyNames.join(", ")}…`);
            updateRun((r) => onShown(r, { names: heavyNames }));
            if (screenRef.current !== "work") setScreen("work", "push");
          }
        }
        const info = await engine.open(job, dwg, toRun, !split, toScene);
        if (token !== openToken.current) return;
        // Choices made while part of it was shown carry over.
        const choices = (split && currentRef.current?.job === job ? currentRef.current.choices : null) ?? opts.choices ?? AUTO;
        const cur: Current = { id: ++seq.current, job, handle, inspection: info, choices, batchItem: opts.batchItem ?? null };
        setCur(cur);
        if (!(await convert(cur, true)) || token !== openToken.current) return;
        // Written: every station done, then the rail goes.
        updateRun((r) => finishRun(onHash(r, resultRef.current?.report.parity_hash ?? "")));
        const finished = runRef.current;
        setTimeout(() => {
          if (runRef.current === finished) setRun(null);
        }, RAIL_LINGER_MS);
        setScreen("work", opts.reload || screenRef.current === "work" ? "replace" : "push");
        if (!opts.reload && previous && previous !== info.name) notifications.show({ message: `Replaced ${previous} with ${info.name}` });
        if (opts.reload) notifications.show({ message: `Reloaded ${info.name}` });
      } catch (e) {
        if (token === openToken.current) fail(failedName(job, e), e, () => void openJob(job, handle, { ...opts, force: true }), job);
      } finally {
        if (token === openToken.current) {
          setPending([]);
          // Stopped without finishing (an empty drawing, a failure): no rail left behind.
          if (runRef.current && !runRef.current.finished) setRun(null);
        }
        if (opts.reload) setBusy(null);
      }
    },
    [convert, setScreen],
  );

  /** Stop opening a file: the engine is stopped mid-step and restarted, and the app goes
   *  back to where it was (a drawing that was open is opened again when next needed). */
  const cancelOpen = () => {
    ++openToken.current;
    ++convertToken.current;
    engine.restart(); // a file-list conversion it interrupts is retried (runBatch)
    setRun(null);
    pointFeed.clear();
    setBusy(null);
    shownPreview.current = null;
    setScreen(currentRef.current && resultRef.current ? "work" : batchRef.current.length ? "batch" : "empty", "replace");
  };

  // ---------------------------------------------------------------- batch
  const runBatch = useCallback(async (items: BatchItem[]) => {
    const token = ++batchToken.current;
    for (const item of items) {
      if (token !== batchToken.current) return;
      updateBatch(item.id, { status: "converting", result: undefined, error: undefined });
      for (let attempt = 0; ; attempt++) {
        try {
          await engine.open(item.job, prefsRef.current.format === "dwg");
          const r = await engine.convert(item.job, engineSettings(prefsRef.current, AUTO), false);
          if (isEmpty(r.report)) updateBatch(item.id, { status: "failed", error: explain({ kind: "empty" }) });
          else updateBatch(item.id, { status: "done", result: r });
        } catch (e) {
          // Stopped because another file's opening was cancelled (or crashed): not this file's fault.
          const interrupted = e instanceof EngineError && e.failure.kind === "crash" && e.failure.message === RESTARTED;
          if (interrupted && attempt === 0 && token === batchToken.current) continue;
          updateBatch(item.id, { status: "failed", error: explain(failureOf(e)) });
        }
        break;
      }
    }
  }, []);

  const startBatch = (jobs: Job[]) => {
    batchRef.current = jobs.map((job) => ({ id: ++seq.current, job, name: jobName(job), size: jobSize(job), status: "waiting" as const }));
    setBatch(batchRef.current);
    setCur(null);
    shownPreview.current = null;
    setScreen("batch", screenRef.current === "choose" ? "replace" : "push");
    void runBatch(batchRef.current);
  };

  // ---------------------------------------------------------------- opening files
  const openFiles = useCallback(
    async (files: File[], handles: FileHandleLike[] = []) => {
      let p;
      try {
        p = await plan(files);
      } catch (e) {
        notifications.show({ color: "red", title: "Couldn't open the .zip", message: (e as Error).message });
        return;
      }
      if (p.kind === "none") {
        notifications.show({ title: "Nothing to convert", message: p.ignored.length ? `Not used: ${p.ignored.slice(0, 3).join(", ")}` : "Choose an .obj, an .xyz or a .zip." });
        return;
      }
      if (p.kind === "materials") {
        if (screenRef.current === "work" && currentRef.current) return addSources(p.mtls);
        notifications.show({ title: "No model", message: "Choose an .obj, an .xyz or a .zip." });
        return;
      }
      batchToken.current++; // a new drop stops a running batch
      setWatching(false);
      if (p.kind === "one") {
        const only = p.job.sources.length === 1 ? p.job.sources[0] : null;
        const handle = only && !only.zip ? (handles.find((h) => h.name === only.file.name) ?? null) : null;
        return openJob(p.job, handle);
      }
      if (p.kind === "choose") {
        choiceRef.current = { combined: p.combined, separate: p.separate };
        setChoice(choiceRef.current);
        setScreen("choose", "push");
        return;
      }
      startBatch(p.jobs);
    },
    [openJob],
  );

  /** Add files (material libraries) to the open drawing and read it again. */
  const addSources = async (extra: Source[]) => {
    const cur = currentRef.current;
    if (!cur) return;
    const names = new Set(extra.map((s) => baseName(s.path).toLowerCase()));
    const job: Job = { ...cur.job, sources: [...cur.job.sources.filter((s) => !names.has(baseName(s.path).toLowerCase())), ...extra] };
    await openJob(job, cur.handle, { reload: true, force: true, choices: cur.choices, batchItem: cur.batchItem });
  };

  /** Choose files: the browser's file picker (with handles, for reloading) or the input. */
  const pick = useCallback(async () => {
    const w = window as Window & { showOpenFilePicker?: (o: object) => Promise<FileHandleLike[]> };
    if (!w.showOpenFilePicker) return input.current?.click();
    let handles: FileHandleLike[];
    try {
      handles = await w.showOpenFilePicker({ multiple: true, types: [{ description: "3D models", accept: { "application/octet-stream": [".obj", ".xyz", ".mtl", ".jpg", ".jpeg", ".png", ".zip"] } }] });
    } catch (e) {
      if (!(e instanceof DOMException && e.name === "AbortError")) input.current?.click();
      return;
    }
    await openFiles(await Promise.all(handles.map((h) => h.getFile())), handles);
  }, [openFiles]);

  // ---------------------------------------------------------------- settings
  const change = (choices: Partial<FileChoices>) => {
    const cur = currentRef.current;
    if (!cur) return;
    const next = { ...cur, choices: { ...cur.choices, ...choices } };
    setCur(next);
    // Part of the drawing is still loading: the choice applies when all of it is converted.
    if (!pendingRef.current.length) void convert(next);
  };

  const changePrefs = (update: Partial<Prefs>) => {
    setPrefs(update);
    const cur = currentRef.current;
    if (cur && screenRef.current === "work" && !pendingRef.current.length) void convert(cur);
  };

  // ---------------------------------------------------------------- downloads
  const ext = () => formatInfo(prefsRef.current.format).ext;

  const save = async (blob: Blob, name: string, pickLocation = false) => {
    try {
      const saved = await saveFile(blob, name, pickLocation);
      if (saved) setDownloaded(saved);
    } catch (e) {
      notifications.show({ color: "red", title: "Couldn't save the file", message: (e as Error).message });
    }
  };

  /** What Download saves: the drawing, or without the layers left out when some are. */
  const downloadable = () => {
    const cur = currentRef.current;
    const partial = hiddenRef.current.length > 0;
    const r = partial ? visibleRef.current : resultRef.current;
    return cur && r ? { r, name: `${stem(cur.inspection.name)}${partial ? " (visible layers)" : ""}` } : null;
  };

  const download = (pickLocation: boolean) => {
    const d = downloadable();
    if (d && !busy) void save(d.r.file.blob(), `${d.name}.${ext()}`, pickLocation);
  };

  const changeHidden = (names: string[]) => {
    setHidden(names);
    setDownloaded(null);
    const cur = currentRef.current;
    if (cur) void convertVisible(cur);
  };

  /** The report of exactly what Download saves. */
  const downloadReport = async () => {
    const d = downloadable();
    if (!d || busy) return; // while busy the report may be provisional
    const { r } = d;
    const report = { ...r.report, output: { ...r.report.output, sha256: await sha256Hex(r.file.blob()) } };
    const saved = await saveFile(new Blob([JSON.stringify(report, null, 2)], { type: "application/json" }), `${d.name}.report.json`).catch(() => null);
    if (saved) notifications.show({ message: `Saved ${saved}` });
  };

  const downloadAll = async () => {
    const done = batchRef.current.filter((i) => i.status === "done" && i.result);
    setZipping(true);
    try {
      const names = new Set<string>();
      const files = done.map((i) => {
        let name = `${stem(i.name)}.${ext()}`;
        for (let n = 2; names.has(name.toLowerCase()); n++) name = `${stem(i.name)} (${n}).${ext()}`;
        names.add(name.toLowerCase());
        return { name, blob: i.result!.file.blob() };
      });
      await save(await zipFiles(files), `obj2cad ${done.length} files.zip`);
    } catch (e) {
      notifications.show({ color: "red", title: "Couldn't make the .zip", message: (e as Error).message });
    } finally {
      setZipping(false);
    }
  };

  // ---------------------------------------------------------------- watching
  useEffect(() => {
    if (!watching || !current?.handle) return;
    const timer = setInterval(async () => {
      const cur = currentRef.current;
      if (!cur?.handle || screenRef.current !== "work") return;
      try {
        const f = await cur.handle.getFile();
        const old = cur.job.sources[0].file;
        if (f.lastModified !== old.lastModified || f.size !== old.size) {
          const job: Job = { ...cur.job, sources: [{ file: f, path: cur.job.sources[0].path }, ...cur.job.sources.slice(1)] };
          await openJob(job, cur.handle, { reload: true, force: true, choices: cur.choices, batchItem: cur.batchItem });
        }
      } catch {
        setWatching(false); // file moved or permission withdrawn
      }
    }, 2000);
    return () => clearInterval(timer);
  }, [watching, current?.handle, openJob]);

  // ---------------------------------------------------------------- global keys
  useEffect(() => {
    const idle = window.requestIdleCallback ?? ((cb: () => void) => setTimeout(cb, 1500));
    idle(() => {
      void loadWorkspace();
      if (prefsRef.current.format === "dwg") engine.warm(true);
    });
  }, []);

  // The tab says what's happening, for anyone who switched away while a big file opens.
  const title = useRef(document.title);
  useEffect(() => {
    document.title =
      screen === "loading"
        ? `Opening${run ? `: ${runLabel(run)}` : ""} · ${loading.name}`
        : screen === "work" && busy && current
          ? `${busy.replace(/…$/, "")} · ${current.inspection.name}`
          : title.current;
  }, [screen, loading, busy, current, run]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!(e.ctrlKey || e.metaKey)) return;
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
  /** How far the files still loading are, for their rows in the layers pane. */
  const pendingNote = (() => {
    if (!run || !pending.length) return undefined;
    const readSt = run.stations.find((s) => s.key === "read");
    if (run.counting && pending.includes(run.counting.file) && run.stations.find((s) => s.key === "model")?.state === "active")
      return `${fmt(run.counting.count)} ${run.counting.cloud ? "points" : "vertices"}`;
    if (readSt?.state === "active" && readSt.frac !== null) return `${Math.floor(readSt.frac * 100)}% read`;
    return undefined;
  })();
  const inBatch = current?.batchItem != null && batch.length > 0;
  return (
    <div className="relative h-dvh overflow-hidden">
      <TopBar
        file={
          screen === "work" && current
            ? {
                name: current.inspection.name,
                size: jobSize(current.job),
                exporter: current.inspection.hints.exporter,
                files: current.inspection.files.filter((f) => f.role !== "missing" && f.role !== "not_used").length,
              }
            : null
        }
        offlineReady={offlineReady}
        onOpen={() => void pick()}
        onOpenFolder={pickFolder}
        onBack={screen === "work" && inBatch ? () => window.history.back() : undefined}
        backLabel="All files"
        watch={screen === "work" && current?.handle ? watching : null}
        onWatch={setWatching}
      />

      {screen === "empty" && <DropScreen onPick={() => void pick()} onPickFolder={pickFolder} />}
      {screen === "preflight" && preflight && (
        <PreflightScreen
          name={preflight.name}
          size={preflight.size}
          job={preflight.job}
          prefs={prefs}
          onContinue={preflight.go}
          onCancel={() => setScreen(currentRef.current && resultRef.current ? "work" : "empty")}
        />
      )}
      {screen === "failed" && failed && <FailedScreen name={failed.name} explained={failed.explained} job={failed.job} prefs={prefs} onPick={() => void pick()} onRetry={failed.retry} />}
      {screen === "choose" && choice && (
        <ChoiceScreen separate={choice.separate} onCombine={() => void openJob(choice.combined, null)} onSeparate={() => startBatch(choice.separate)} />
      )}
      {screen === "batch" && (
        <BatchScreen
          items={batch}
          format={prefs.format}
          zipping={zipping}
          onFormat={(format: Format) => {
            setPrefs({ format });
            void runBatch(batchRef.current);
          }}
          onOpen={(item) => void openJob(item.job, null, { batchItem: item.id })}
          onDownload={(item) => item.result && void save(item.result.file.blob(), `${stem(item.name)}.${ext()}`)}
          onDownloadAll={() => void downloadAll()}
        />
      )}
      {/* While a drawing is opened the workspace is already there: the scene builds from
          what is read, and the rail shows each step. */}
      {(screen === "loading" || (screen === "work" && result)) && (
        <ErrorBoundary>
          <Suspense fallback={<WorkspaceSkeleton />}>
            <Workspace
              result={screen === "work" ? result : null}
              visible={visible}
              hidden={hidden}
              preview={screen === "work" ? preview : null}
              inspection={current?.inspection ?? null}
              pending={pending}
              pendingNote={pendingNote}
              prefs={prefs}
              busy={busy}
              write={write}
              railShown={run !== null}
              downloaded={downloaded}
              onUp={(up: UpAxis | null) => change({ up })}
              onUnits={(units: Units | null) => change({ units })}
              onHouseUnits={(houseUnits: Units | null) => changePrefs({ houseUnits })}
              onShowIn={(showIn: Units | null) => setPrefs({ showIn })} // display only: nothing to convert
              onKeepLoose={(keepLoose) => change({ keepLoose })}
              onLayerMode={(layerMode: LayerMode) => {
                setHidden([]); // other layers: nothing is left out any more
                changePrefs({ layerMode });
              }}
              onFormat={(format: Format) => changePrefs({ format })}
              onIncludeName={(includeName) => changePrefs({ includeName })}
              onCurves={(curves) => changePrefs({ curves })}
              onDownload={download}
              onHidden={changeHidden}
              onDownloadReport={() => void downloadReport()}
              onAddMtl={() => mtlInput.current?.click()}
              onAnother={() => void pick()}
            />
          </Suspense>
        </ErrorBoundary>
      )}

      <AnimatePresence>
        {run && (screen === "loading" || screen === "work") && (
          <motion.div
            key="rail"
            initial={{ opacity: 0, y: -6 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, y: -6 }}
            transition={{ duration: 0.22, ease: "easeOut" }}
            className="pointer-events-none absolute inset-x-0 top-[68px] z-20 flex justify-center px-3 lg:top-[76px] lg:right-[448px] lg:left-[300px] lg:px-0"
          >
            <LoadRail run={run} label={`${screen === "loading" ? "Opening" : "Preparing"} ${loading.name}`} onCancel={screen === "loading" ? cancelOpen : undefined} />
          </motion.div>
        )}
      </AnimatePresence>

      <Dropzone.FullScreen
        onDrop={(files) => void openFiles(files)}
        activateOnClick={false}
        multiple
        zIndex={400}
        classNames={{ fullScreen: "!bg-[var(--backdrop)] backdrop-blur-sm", root: "!h-full !border-0 !bg-transparent !p-0", inner: "!h-full" }}
      >
        <AnimatePresence>
          <motion.div initial={{ scale: 0.97, opacity: 0 }} animate={{ scale: 1, opacity: 1 }} className="flex h-full items-center justify-center p-8">
            <div className="panel flex flex-col items-center gap-4 border-2 border-dashed !border-accent px-12 py-12 text-center">
              <CubeArt className="size-16 text-accent" />
              <div className="font-display text-[32px] font-semibold tracking-tight">Drop to open</div>
              <div className="text-[14px] text-fg-3">.obj, .xyz, .mtl, images, a .zip or a folder</div>
            </div>
          </motion.div>
        </AnimatePresence>
      </Dropzone.FullScreen>

      <input
        ref={mtlInput}
        type="file"
        accept=".mtl"
        hidden
        onChange={(e) => {
          const f = e.target.files?.[0];
          e.target.value = "";
          if (f) void addSources([{ file: f, path: f.name }]);
        }}
      />
      <input
        ref={folderInput}
        type="file"
        // Folder picking: files arrive with their paths inside the folder (webkitRelativePath).
        {...{ webkitdirectory: "", directory: "" }}
        multiple
        hidden
        aria-label="Choose a folder"
        onChange={(e) => {
          const files = Array.from(e.target.files ?? []);
          e.target.value = "";
          if (files.length) void openFiles(files);
        }}
      />
      <input
        ref={input}
        type="file"
        multiple
        accept=".obj,.xyz,.mtl,.jpg,.jpeg,.png,.zip"
        hidden
        onChange={(e) => {
          const files = Array.from(e.target.files ?? []);
          e.target.value = "";
          if (files.length) void openFiles(files);
        }}
      />
    </div>
  );
}
