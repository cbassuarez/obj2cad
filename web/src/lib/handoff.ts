// "Open in obj2cad": an obj2cad:// link the command-line version opens once it is set up
// (`obj2cad setup`, see crates/obj2cad-cli/src/handoff.rs). A page can't tell anything
// where a file lives, so the link names the file and its size, and the settings.
import type { Job } from "@/lib/files";
import { baseName } from "@/lib/files";
import { REPO } from "@/lib/errors";
import { unitSymbol, type Prefs } from "@/lib/settings";

declare const __APP_VERSION__: string;

/** What the person gave the app, as the command-line version can find it on disk: the
 *  .zip or the file (with its size), or the folder. */
export function handoffItem(job: Job): { name: string; size?: number } {
  const files = new Set(job.sources.map((s) => s.file));
  if (files.size === 1) {
    const [f] = files;
    return { name: f.name, size: f.size };
  }
  const folders = new Set(job.sources.map((s) => s.path.split(/[\\/]/)).filter((p) => p.length > 1).map((p) => p[0]));
  if (folders.size === 1) return { name: [...folders][0] };
  const model = job.sources.find((s) => /\.(obj|xyz)$/i.test(s.path)) ?? job.sources[0];
  return { name: baseName(model.path), size: model.file.size };
}

/** The link for `job` with the remembered preferences. */
export function handoffLink(job: Job, prefs: Prefs): string {
  const item = handoffItem(job);
  const q = new URLSearchParams({ v: "1", name: item.name });
  if (item.size !== undefined) q.set("size", String(item.size));
  q.set("format", prefs.format);
  q.set("layers", prefs.layerMode);
  const house = prefs.houseUnits && unitSymbol(prefs.houseUnits);
  if (house) q.set("default-units", house);
  if (prefs.curves) q.set("curves", "1");
  return `obj2cad://convert?${q.toString()}`;
}

/** Ask the operating system to open `link` (it asks the person first). */
export function openLink(link: string) {
  const a = document.createElement("a");
  a.href = link;
  a.click();
}

export interface Download {
  label: string;
  url: string;
}

const asset = (target: string, ext: string): string => `${REPO}/releases/download/obj2cad-v${__APP_VERSION__}/obj2cad-cli-${__APP_VERSION__}-${target}.${ext}`;

/** The command-line version for this computer (both Mac builds: a browser can't tell
 *  Apple silicon from Intel). Empty when the system isn't one it is built for. */
export function downloadsFor(userAgent: string): Download[] {
  if (/Windows/i.test(userAgent)) return [{ label: "Windows", url: asset("x86_64-pc-windows-msvc", "zip") }];
  if (/Mac OS X|Macintosh/i.test(userAgent) && !/iPhone|iPad/i.test(userAgent))
    return [
      { label: "Mac (Apple silicon)", url: asset("aarch64-apple-darwin", "tar.gz") },
      { label: "Mac (Intel)", url: asset("x86_64-apple-darwin", "tar.gz") },
    ];
  if (/Linux/i.test(userAgent) && !/Android/i.test(userAgent)) return [{ label: "Linux", url: asset("x86_64-unknown-linux-gnu", "tar.gz") }];
  return [];
}
