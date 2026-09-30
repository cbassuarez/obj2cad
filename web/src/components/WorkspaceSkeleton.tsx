// The workspace's layout before it has its content: the layers pane, the result card and
// the viewport in their places, so opening a large model never shows a blank page.

/** "Preparing the 3D view" over the viewport. Its spinner is a CSS animation, which keeps
 *  turning while the page is busy building the model. */
export function PreparingView() {
  return (
    <div className="pointer-events-none absolute inset-0 grid place-items-center" role="status" aria-label="Preparing the 3D view">
      <div className="panel flex items-center gap-2.5 !rounded-[4px] px-4 py-2 text-[13px] text-fg-2">
        <span className="size-3.5 animate-spin rounded-full border-2 border-line border-t-accent" />
        Preparing the 3D view…
      </div>
    </div>
  );
}

const Bar = ({ w, h = "h-3" }: { w: string; h?: string }) => <div className={`${h} ${w} animate-pulse rounded-[3px] bg-panel-2`} />;

/** The result card before there is a result (the file is still being read). */
export function ResultSkeleton() {
  return (
    <div className="panel flex w-full flex-col gap-4 p-5" aria-hidden="true">
      <div className="flex items-center gap-3">
        <div className="size-9 animate-pulse rounded-[4px] bg-panel-2" />
        <Bar w="w-32" h="h-4" />
      </div>
      {["w-40", "w-28", "w-36", "w-24"].map((w, i) => (
        <div key={i} className="flex items-center gap-6">
          <Bar w="w-16" />
          <Bar w={w} />
        </div>
      ))}
      <div className="h-12 animate-pulse rounded-[4px] bg-panel-2" />
    </div>
  );
}

/** The layers pane before the layers are known. */
export function LayersSkeleton() {
  return (
    <div className="panel flex w-full flex-col gap-3 p-4" aria-hidden="true">
      <Bar w="w-24" />
      {["w-36", "w-28", "w-40", "w-32", "w-24"].map((w, i) => (
        <div key={i} className="flex items-center gap-2.5">
          <div className="size-4 animate-pulse rounded-[3px] bg-panel-2" />
          <Bar w={w} />
        </div>
      ))}
    </div>
  );
}

export function WorkspaceSkeleton() {
  return (
    <main className="paper relative flex h-dvh flex-col overflow-hidden lg:block" aria-busy="true">
      <div className="relative min-h-[200px] flex-1 lg:absolute lg:inset-0" />
      <div className="flex max-h-[58%] shrink-0 flex-col gap-3 overflow-hidden border-t border-line bg-bg p-3 lg:contents" aria-hidden="true">
        <div className="lg:absolute lg:top-[76px] lg:right-4 lg:w-[364px]">
          <ResultSkeleton />
        </div>
        <div className="hidden lg:absolute lg:top-[76px] lg:left-4 lg:block lg:w-[268px]">
          <LayersSkeleton />
        </div>
      </div>
    </main>
  );
}
