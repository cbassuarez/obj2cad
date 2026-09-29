import { lazy, Suspense, useCallback, useEffect, useRef, useState } from "react";
import { Dropzone } from "@mantine/dropzone";
import { notifications } from "@mantine/notifications";
import { useRegisterSW } from "virtual:pwa-register/react";
import { AnimatePresence, motion } from "motion/react";
import { CubeArt } from "@/components/brand";
import { BatchScreen, type BatchItem } from "@/components/BatchScreen";
import { DropScreen } from "@/components/DropScreen";
import { ErrorBoundary } from "@/components/ErrorBoundary";
import { FailedScreen } from "@/components/FailedScreen";
import { LoadingScreen, PreflightScreen } from "@/components/LoadingScreen";
import { TopBar } from "@/components/TopBar";
import { Button } from "@/components/ui/button";
import type { PreviewState } from "@/components/Workspace";
import { engine, EngineError, isEmpty, sha256Hex, type Failure, type Inspection, type Progress, type Result } from "@/lib/engine";
import { CLI_URL, explain, type Explained } from "@/lib/errors";
import { gather, pickMtl, saveFile, stem, zipFiles } from "@/lib/files";
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

type Screen = "empty" | "preflight" | "loading" | "failed" | "work" | "batch";

/** A file handle from the File System Access API (Chromium), for reloading on change. */
interface FileHandleLike {
  name: string;
  getFile(): Promise<File>;
}

interface Current {
  id: number;
  file: File;
  mtl: File | null;
  /** Material libraries that came with it (paired by name). */
  mtls: File[];
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

// The 3D workspace (three.js) loads on demand, so the first screen is small and fast;
// it is prefetched as soon as the browser is idle.
const loadWorkspace = () => import("@/components/Workspace");
const Workspace = lazy(() => loadWorkspace().then((m) => ({ default: m.Workspace })));

const failureOf = (e: unknown): Failure => (e instanceof EngineError ? e.failure : { kind: "other", message: e instanceof Error ? e.message : String(e) });

export function App() {
  const [screen, setScreenState] = useState<Screen>("empty");
  const [prefs, setPrefsState] = useState<Prefs>(loadPrefs);
  const [current, setCurrent] = useState<Current | null>(null);
  const [result, setResult] = useState<Result | null>(null);
  const [preview, setPreview] = useState<PreviewState | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [loading, setLoading] = useState<{ name: string; progress: Progress | null }>({ name: "", progress: null });
  const [failed, setFailed] = useState<{ name: string; explained: Explained; retry?: () => void } | null>(null);
  const [preflight, setPreflight] = useState<{ file: File; go: () => void } | null>(null);
  const [batch, setBatch] = useState<BatchItem[]>([]);
  const [downloaded, setDownloaded] = useState<string | null>(null);
  const [watching, setWatching] = useState(false);
  const [zipping, setZipping] = useState(false);

  const input = useRef<HTMLInputElement>(null);
  const mtlInput = useRef<HTMLInputElement>(null);
  const screenRef = useRef<Screen>("empty");
  const prefsRef = useRef(prefs);
  const currentRef = useRef<Current | null>(null);
  const resultRef = useRef<Result | null>(null);
  const batchRef = useRef<BatchItem[]>([]);
  const batchMtls = useRef<File[]>([]);
  const shownPreview = useRef<{ key: string; fileId: number } | null>(null);
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
  const fail = (name: string, e: unknown, retry?: () => void) => {
    const failure = failureOf(e);
    const retryable = failure.kind === "crash" || failure.kind === "read" || failure.kind === "other";
    setFailed({ name, explained: explain(failure), retry: retryable ? retry : undefined });
    setScreen("failed", screenRef.current === "empty" ? "push" : "replace");
  };

  // ---------------------------------------------------------------- conversion
  /** Convert the open file with the current preferences and its choices. */
  const convert = useCallback(async (cur: Current, forcePreview = false, label = "Converting…"): Promise<boolean> => {
    const token = ++convertToken.current;
    const settings = engineSettings(prefsRef.current, cur.choices, cur.file);
    const key = previewKey(settings);
    const shown = shownPreview.current;
    const wantPreview = forcePreview || !shown || shown.key !== key || shown.fileId !== cur.id;
    setBusy(label);
    setDownloaded(null);
    try {
      const r = await engine.convert(cur, settings, wantPreview, () => setBusy(settings.format === "dwg" ? "Preparing DWG…" : label));
      if (token !== convertToken.current) return false;
      if (isEmpty(r.report)) {
        setFailed({ name: cur.file.name, explained: explain({ kind: "empty" }) });
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
      return true;
    } catch (e) {
      if (token !== convertToken.current) return false;
      fail(cur.file.name, e, () => void openSingle(cur.file, cur.mtls, cur.handle, { choices: cur.choices, batchItem: cur.batchItem, force: true }));
      return false;
    } finally {
      if (token === convertToken.current) setBusy(null);
    }
    // Stable: everything it reads is a ref or a state setter (openSingle is only called later).
  }, []);

  /** Open one file into the workspace. */
  const openSingle = useCallback(
    async (file: File, mtls: File[], handle: FileHandleLike | null, opts: OpenOptions = {}): Promise<void> => {
      if (file.size > BIG && !opts.force) {
        setPreflight({ file, go: () => void openSingle(file, mtls, handle, { ...opts, force: true }) });
        setScreen("preflight");
        return;
      }
      const token = ++openToken.current;
      const previous = screenRef.current === "work" ? currentRef.current?.file.name : undefined;
      setLoading({ name: file.name, progress: null });
      if (!opts.reload) setScreen("loading");
      else setBusy("Reloading…");
      try {
        const info = await engine.open(file, prefsRef.current.format === "dwg", (p) => setLoading({ name: file.name, progress: p }));
        if (token !== openToken.current) return;
        const mtl = pickMtl(info.mtllibs, mtls, true);
        if (mtl) await engine.setMtl(file, mtl);
        const cur: Current = { id: ++seq.current, file, mtl, mtls, handle, inspection: info, choices: opts.choices ?? AUTO, batchItem: opts.batchItem ?? null };
        setCur(cur);
        if (!(await convert(cur, true)) || token !== openToken.current) return;
        setScreen("work", opts.reload || screenRef.current === "work" ? "replace" : "push");
        if (!opts.reload && previous && previous !== file.name) notifications.show({ message: `Replaced ${previous} with ${file.name}` });
        if (opts.reload) notifications.show({ message: `Reloaded ${file.name}` });
      } catch (e) {
        if (token === openToken.current) fail(file.name, e, () => void openSingle(file, mtls, handle, { ...opts, force: true }));
      } finally {
        if (opts.reload) setBusy(null);
      }
    },
    [convert, setScreen],
  );

  // ---------------------------------------------------------------- batch
  const runBatch = useCallback(async (items: BatchItem[]) => {
    const token = ++batchToken.current;
    for (const item of items) {
      if (token !== batchToken.current) return;
      updateBatch(item.id, { status: "converting", result: undefined, error: undefined });
      try {
        const info = await engine.open(item.file, prefsRef.current.format === "dwg");
        const mtl = pickMtl(info.mtllibs, batchMtls.current, false);
        if (mtl) await engine.setMtl(item.file, mtl);
        const r = await engine.convert({ file: item.file, mtl }, engineSettings(prefsRef.current, AUTO, item.file), false);
        if (isEmpty(r.report)) updateBatch(item.id, { status: "failed", error: explain({ kind: "empty" }) });
        else updateBatch(item.id, { status: "done", result: r });
      } catch (e) {
        updateBatch(item.id, { status: "failed", error: explain(failureOf(e)) });
      }
    }
  }, []);

  const startBatch = (objs: File[], mtls: File[]) => {
    batchMtls.current = mtls;
    batchRef.current = objs.map((file) => ({ id: ++seq.current, file, status: "waiting" as const }));
    setBatch(batchRef.current);
    setCur(null);
    shownPreview.current = null;
    setScreen("batch", "push");
    void runBatch(batchRef.current);
  };

  // ---------------------------------------------------------------- opening files
  const openFiles = useCallback(
    async (files: File[], handles: FileHandleLike[] = []) => {
      let picked;
      try {
        picked = await gather(files);
      } catch (e) {
        notifications.show({ color: "red", title: "Couldn't open the .zip", message: (e as Error).message });
        return;
      }
      if (!picked.objs.length) {
        const cur = currentRef.current;
        if (picked.mtls.length && screenRef.current === "work" && cur) return addMtl(picked.mtls[0]);
        notifications.show({ title: "No .obj file", message: picked.ignored.length ? `Not used: ${picked.ignored.slice(0, 3).join(", ")}` : "Choose an .obj file." });
        return;
      }
      batchToken.current++; // a new drop stops a running batch
      if (picked.objs.length === 1) {
        const obj = picked.objs[0];
        const handle = handles.find((h) => h.name === obj.name) ?? null;
        setWatching(false);
        return openSingle(obj, picked.mtls, handle);
      }
      startBatch(picked.objs, picked.mtls);
    },
    [openSingle],
  );

  const addMtl = async (mtl: File) => {
    const cur = currentRef.current;
    if (!cur) return;
    try {
      await engine.setMtl(cur.file, mtl);
    } catch (e) {
      return fail(cur.file.name, e);
    }
    const next = { ...cur, mtl };
    setCur(next);
    await convert(next, true);
  };

  /** Choose files: the browser's file picker (with handles, for reloading) or the input. */
  const pick = useCallback(async () => {
    const w = window as Window & { showOpenFilePicker?: (o: object) => Promise<FileHandleLike[]> };
    if (!w.showOpenFilePicker) return input.current?.click();
    let handles: FileHandleLike[];
    try {
      handles = await w.showOpenFilePicker({ multiple: true, types: [{ description: "3D models", accept: { "application/octet-stream": [".obj", ".mtl", ".zip"] } }] });
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
    void convert(next);
  };

  const changePrefs = (update: Partial<Prefs>) => {
    setPrefs(update);
    const cur = currentRef.current;
    if (cur && screenRef.current === "work") void convert(cur);
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

  const download = (pickLocation: boolean) => {
    const cur = currentRef.current;
    const r = resultRef.current;
    if (cur && r && !busy) void save(r.file, `${stem(cur.file.name)}.${ext()}`, pickLocation);
  };

  const downloadVisible = async (hiddenLayers: string[]) => {
    const cur = currentRef.current;
    if (!cur) return;
    setBusy("Converting…");
    try {
      const r = await engine.convert(cur, engineSettings(prefsRef.current, cur.choices, cur.file, hiddenLayers), false);
      await save(r.file, `${stem(cur.file.name)} (visible layers).${ext()}`);
    } catch (e) {
      notifications.show({ color: "red", title: "Couldn't convert the visible layers", message: (e as Error).message });
    } finally {
      setBusy(null);
    }
  };

  const downloadReport = async () => {
    const cur = currentRef.current;
    const r = resultRef.current;
    if (!cur || !r) return;
    const report = { ...r.report, output: { ...r.report.output, sha256: await sha256Hex(r.file) } };
    const saved = await saveFile(new Blob([JSON.stringify(report, null, 2)], { type: "application/json" }), `${stem(cur.file.name)}.report.json`).catch(() => null);
    if (saved) notifications.show({ message: `Saved ${saved}` });
  };

  const downloadAll = async () => {
    const done = batchRef.current.filter((i) => i.status === "done" && i.result);
    setZipping(true);
    try {
      const names = new Set<string>();
      const files = done.map((i) => {
        let name = `${stem(i.file.name)}.${ext()}`;
        for (let n = 2; names.has(name.toLowerCase()); n++) name = `${stem(i.file.name)} (${n}).${ext()}`;
        names.add(name.toLowerCase());
        return { name, blob: i.result!.file };
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
        if (f.lastModified !== cur.file.lastModified || f.size !== cur.file.size) {
          await openSingle(f, cur.mtls, cur.handle, { reload: true, force: true, choices: cur.choices, batchItem: cur.batchItem });
        }
      } catch {
        setWatching(false); // file moved or permission withdrawn
      }
    }, 2000);
    return () => clearInterval(timer);
  }, [watching, current?.handle, openSingle]);

  // ---------------------------------------------------------------- global keys
  useEffect(() => {
    const idle = window.requestIdleCallback ?? ((cb: () => void) => setTimeout(cb, 1500));
    idle(() => void loadWorkspace());
  }, []);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!(e.ctrlKey || e.metaKey)) return;
      // Ctrl/⌘+S is the workspace's: it knows which layers are hidden.
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
  return (
    <div className="relative h-dvh overflow-hidden">
      <TopBar
        file={screen === "work" && current ? { name: current.file.name, size: current.file.size, exporter: current.inspection.hints.exporter, mtl: current.mtl?.name ?? null } : null}
        offlineReady={offlineReady}
        onOpen={() => void pick()}
        onBack={screen === "work" && inBatch ? () => window.history.back() : undefined}
        backLabel="All files"
        watch={screen === "work" && current?.handle ? watching : null}
        onWatch={setWatching}
      />

      {screen === "empty" && <DropScreen onPick={() => void pick()} />}
      {screen === "preflight" && preflight && (
        <PreflightScreen
          name={preflight.file.name}
          size={preflight.file.size}
          cliUrl={CLI_URL}
          onContinue={preflight.go}
          onCancel={() => setScreen(currentRef.current && resultRef.current ? "work" : "empty")}
        />
      )}
      {screen === "loading" && <LoadingScreen name={loading.name} progress={loading.progress} />}
      {screen === "failed" && failed && <FailedScreen name={failed.name} explained={failed.explained} onPick={() => void pick()} onRetry={failed.retry} />}
      {screen === "batch" && (
        <BatchScreen
          items={batch}
          format={prefs.format}
          zipping={zipping}
          onFormat={(format: Format) => {
            setPrefs({ format });
            void runBatch(batchRef.current);
          }}
          onOpen={(item) => void openSingle(item.file, batchMtls.current, null, { batchItem: item.id })}
          onDownload={(item) => item.result && void save(item.result.file, `${stem(item.file.name)}.${ext()}`)}
          onDownloadAll={() => void downloadAll()}
        />
      )}
      {screen === "work" && result && (
        <ErrorBoundary>
          <Suspense fallback={<main className="h-dvh bg-viewport" />}>
            <Workspace
              result={result}
              preview={preview}
              inspection={current?.inspection ?? null}
              prefs={prefs}
              busy={busy}
              downloaded={downloaded}
              onUp={(up: UpAxis | null) => change({ up })}
              onUnits={(units: Units | null) => change({ units })}
              onHouseUnits={(houseUnits: Units | null) => changePrefs({ houseUnits })}
              onKeepLoose={(keepLoose) => change({ keepLoose })}
              onLayerMode={(layerMode: LayerMode) => changePrefs({ layerMode })}
              onFormat={(format: Format) => changePrefs({ format })}
              onIncludeName={(includeName) => changePrefs({ includeName })}
              onDownload={download}
              onDownloadVisible={(hidden) => void downloadVisible(hidden)}
              onLayersChanged={() => setDownloaded(null)}
              onDownloadReport={() => void downloadReport()}
              onAddMtl={() => mtlInput.current?.click()}
              onAnother={() => void pick()}
            />
          </Suspense>
        </ErrorBoundary>
      )}

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
              <div className="text-[14px] text-fg-3">.obj, .mtl or .zip</div>
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
          if (f) void addMtl(f);
        }}
      />
      <input
        ref={input}
        type="file"
        multiple
        accept=".obj,.mtl,.zip"
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
