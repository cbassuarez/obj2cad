import { lazy, Suspense, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Dropzone } from "@mantine/dropzone";
import { notifications } from "@mantine/notifications";
import { useRegisterSW } from "virtual:pwa-register/react";
import { AnimatePresence, motion } from "motion/react";
import { CubeArt } from "@/components/brand";
import { DropScreen } from "@/components/DropScreen";
import { FailedScreen } from "@/components/FailedScreen";
import { SetupDialog } from "@/components/SetupDialog";
import { TopBar, type FileInfo } from "@/components/TopBar";
import { Button } from "@/components/ui/button";
import { ErrorBoundary } from "@/components/ErrorBoundary";
import type { Suggestion } from "@/components/Dock";
import * as engine from "@/lib/engine";
import type { ConvertResult, Inspection, Report } from "@/lib/engine";
import { HINT_UNITS, HINT_UP, UNITS, UPS, loadSettings, saveSettings, type Settings, type Theme } from "@/lib/settings";

type Phase = "empty" | "loading" | "failed" | "work";

// The 3D workspace (three.js) loads on demand, so the first screen is small and fast;
// it is prefetched as soon as the browser is idle.
const loadWorkspace = () => import("@/components/Workspace");
const Workspace = lazy(() => loadWorkspace().then((m) => ({ default: m.Workspace })));

function download(data: BlobPart, name: string, type: string) {
  const url = URL.createObjectURL(new Blob([data], { type }));
  const a = Object.assign(document.createElement("a"), { href: url, download: name });
  a.click();
  setTimeout(() => URL.revokeObjectURL(url), 10_000);
}

export function App({ theme, onToggleTheme }: { theme: Theme; onToggleTheme: () => void }) {
  const [phase, setPhase] = useState<Phase>("empty");
  const [file, setFile] = useState<FileInfo | null>(null);
  const [inspection, setInspection] = useState<Inspection | null>(null);
  const [settings, setSettings] = useState<Settings>(() => loadSettings() ?? { units: "mm", up: "as-is" });
  const [asking, setAsking] = useState(false);
  const [result, setResult] = useState<ConvertResult | null>(null);
  const [report, setReport] = useState<Report | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<{ name: string; message: string } | null>(null);
  const input = useRef<HTMLInputElement>(null);
  const mtlInput = useRef<HTMLInputElement>(null);
  const runToken = useRef(0);
  const fileName = useRef("");
  const phaseRef = useRef<Phase>("empty");
  const settingsRef = useRef<Settings>(settings);
  phaseRef.current = phase;
  settingsRef.current = settings;

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
      title: "A new version of obj2cad is ready",
      message: (
        <div className="mt-2 flex items-center gap-3">
          <span className="text-fg-2">Reload when you're done with this file.</span>
          <Button size="sm" variant="primary" onClick={() => updateServiceWorker(true)}>
            Reload
          </Button>
        </div>
      ),
    });
  }, [needRefresh, updateServiceWorker]);

  // ---------------------------------------------------------------- conversion
  const run = useCallback(async (s: Settings) => {
    const token = ++runToken.current;
    setBusy(true);
    try {
      const r = await engine.convert(s.units, s.up);
      if (token !== runToken.current) return; // superseded by a newer run
      const rep: Report = JSON.parse(r.report);
      if (rep.output.faces + rep.output.polylines + rep.output.points === 0) {
        setError({ name: fileName.current, message: "No faces, lines or points found, so there is nothing to convert." });
        setPhase("failed");
        return;
      }
      setResult(r);
      setReport(rep);
      setPhase("work");
    } catch (err) {
      if (token !== runToken.current) return;
      setError({ name: fileName.current, message: (err as Error).message });
      setPhase("failed");
    } finally {
      if (token === runToken.current) setBusy(false);
    }
  }, []);

  const openFiles = useCallback(async (files: File[]) => {
    const obj = files.find((f) => f.name.toLowerCase().endsWith(".obj"));
    const onlyMtl = files.find((f) => f.name.toLowerCase().endsWith(".mtl"));
    if (!obj && onlyMtl && phaseRef.current === "work") {
      // Materials for the open model: apply them and re-convert.
      await engine.setMtl(onlyMtl);
      setFile((prev) => (prev ? { ...prev, mtl: onlyMtl.name } : prev));
      void run(settingsRef.current);
      return;
    }
    if (!obj) {
      notifications.show({ title: "Choose an .obj file", message: "You can add its .mtl alongside it.", color: "accent" });
      return;
    }
    const mtls = files.filter((f) => f.name.toLowerCase().endsWith(".mtl"));
    runToken.current++;
    fileName.current = obj.name;
    setFile({ name: obj.name, size: obj.size, exporter: null, mtl: null });
    setPhase("loading");
    setBusy(true);
    let info: Inspection;
    try {
      info = await engine.load(obj);
    } catch (err) {
      setError({ name: obj.name, message: (err as Error).message });
      setPhase("failed");
      setBusy(false);
      return;
    }
    // Prefer the MTL the OBJ names; otherwise the only one given.
    const named = mtls.find((m) => info.mtllibs.some((l) => l.split(/[\\/]/).pop()?.toLowerCase() === m.name.toLowerCase()));
    const mtl = named ?? (mtls.length === 1 ? mtls[0] : undefined);
    if (mtl) await engine.setMtl(mtl);
    setInspection(info);
    setFile({ name: obj.name, size: obj.size, exporter: info.hints.exporter, mtl: mtl?.name ?? null });
    const saved = loadSettings();
    if (saved) {
      setSettings(saved);
      await run(saved);
    } else {
      setBusy(false);
      setAsking(true);
    }
  }, [run]);

  const change = (next: Settings) => {
    setSettings(next);
    if (loadSettings()) saveSettings(next); // keep remembering the latest choice
    void run(next);
  };

  useEffect(() => {
    const idle = window.requestIdleCallback ?? ((cb: () => void) => setTimeout(cb, 1500));
    idle(() => void loadWorkspace());
  }, []);

  // Ctrl/Cmd+O opens a file, like a desktop app.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "o") {
        e.preventDefault();
        input.current?.click();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  // ---------------------------------------------------------------- derived
  const suggestion = useMemo<Suggestion | null>(() => {
    if (!inspection || phase !== "work") return null;
    const h = inspection.hints;
    const su = HINT_UNITS[h.units];
    if (su && su !== settings.units && h.units !== "unitless") {
      const name = UNITS.find((u) => u.value === su)!.name;
      return { text: `Suggested: ${name} (${h.units_reason})`, apply: () => change({ ...settings, units: su }) };
    }
    const sup = HINT_UP[h.up_axis];
    // Orientation is only worth suggesting when we know the exporter's convention.
    if (h.exporter && sup && sup !== settings.up) {
      const label = UPS.find((u) => u.value === sup)!.label;
      return { text: `Suggested: ${label} (${h.up_axis_reason})`, apply: () => change({ ...settings, up: sup }) };
    }
    return null;
  }, [inspection, settings, phase]); // `change` is recreated each render; these are its inputs

  const stem = (file?.name ?? "model.obj").replace(/\.obj$/i, "");
  const exportDxf = () => result && download(result.dxf as BlobPart, `${stem}.dxf`, "application/dxf");
  const exportReport = () => result && download(result.report, `${stem}.report.json`, "application/json");

  return (
    <div className="relative h-full">
      <TopBar
        file={phase === "work" || phase === "loading" ? file : null}
        theme={theme}
        offlineReady={offlineReady}
        onToggleTheme={onToggleTheme}
        onOpen={() => input.current?.click()}
        onDownloadReport={phase === "work" ? exportReport : undefined}
      />

      {phase === "empty" && <DropScreen onPick={() => input.current?.click()} />}
      {phase === "failed" && error && <FailedScreen name={error.name} message={error.message} onPick={() => input.current?.click()} />}
      {phase === "work" && result && report && (
        <ErrorBoundary>
        <Suspense fallback={<main className="h-full bg-viewport" />}>
        <Workspace
          result={result}
          report={report}
          settings={settings}
          theme={theme}
          busy={busy}
          exporter={file?.exporter ?? null}
          onAddMtl={() => mtlInput.current?.click()}
          suggestion={suggestion}
          onUnits={(units) => change({ ...settings, units })}
          onUp={(up) => change({ ...settings, up })}
          onExport={exportDxf}
          onDownloadReport={exportReport}
        />
        </Suspense>
        </ErrorBoundary>
      )}
      {phase === "loading" && (
        <main className="paper flex h-full items-center justify-center" role="status">
          <div className="panel flex flex-col items-center gap-4 px-10 py-8">
            <motion.div animate={{ rotate: [0, 0, 120, 120] }} transition={{ duration: 1.8, repeat: Infinity, times: [0, 0.3, 0.7, 1] }}>
              <CubeArt className="size-12 text-accent" />
            </motion.div>
            <div className="text-[14px] text-fg-2">
              Reading <span className="num text-fg">{file?.name ?? "file"}</span>…
            </div>
          </div>
        </main>
      )}

      {asking && inspection && (
        <SetupDialog
          inspection={inspection}
          onCancel={() => {
            setAsking(false);
            setPhase("empty");
          }}
          onDone={(s, remember) => {
            setAsking(false);
            if (remember) saveSettings(s);
            setSettings(s);
            void run(s);
          }}
        />
      )}

      <Dropzone.FullScreen
        onDrop={(files) => void openFiles(files)}
        activateOnClick={false}
        multiple
        zIndex={400}
        classNames={{ fullScreen: "!bg-[var(--backdrop)] backdrop-blur-sm", root: "!h-full !border-0 !bg-transparent !p-0", inner: "!h-full" }}
      >
        <AnimatePresence>
          <motion.div
            initial={{ scale: 0.97, opacity: 0 }}
            animate={{ scale: 1, opacity: 1 }}
            className="flex h-full items-center justify-center p-8"
          >
            <div className="panel flex flex-col items-center gap-4 border-2 border-dashed !border-accent px-12 py-12 text-center">
              <CubeArt className="size-16 text-accent" />
              <div className="font-display text-[32px] font-semibold tracking-tight">Drop to convert</div>
              <div className="text-[14px] text-fg-3">.obj, plus its .mtl for colors</div>
            </div>
          </motion.div>
        </AnimatePresence>
      </Dropzone.FullScreen>

      <input
        ref={mtlInput}
        type="file"
        accept=".mtl"
        hidden
        onChange={async (e) => {
          const f = e.target.files?.[0];
          e.target.value = "";
          if (!f) return;
          await engine.setMtl(f);
          setFile((prev) => (prev ? { ...prev, mtl: f.name } : prev));
          void run(settings);
        }}
      />
      <input
        ref={input}
        type="file"
        multiple
        accept=".obj,.mtl"
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
