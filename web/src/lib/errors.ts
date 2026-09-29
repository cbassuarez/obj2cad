// What to tell someone whose file didn't convert: what is wrong, where, and what to do.
import type { Failure } from "@/lib/engine";
import { fmt } from "@/lib/format";

declare const __APP_VERSION__: string;

export const REPO = "https://github.com/cbassuarez/obj2cad";
export const CLI_URL = `${REPO}/releases/latest`;

export interface Explained {
  title: string;
  /** What to do next. */
  action: string;
  /** Line-by-line problems from the engine (parse errors). */
  lines: string[];
  /** More problems exist than listed. */
  more: boolean;
  /** The command-line version is the way forward. */
  cli?: boolean;
  /** Worth reporting: the problem is obj2cad's, not the file's. */
  report?: boolean;
}

const EXPORT_AGAIN = "Export the file again from your 3D app.";

export function explain(f: Failure | { kind: "empty" }): Explained {
  const base = { lines: [] as string[], more: false };
  switch (f.kind) {
    case "empty":
      return { ...base, title: "No faces, lines or points in this file", action: "Check that the right file was exported." };
    case "parse": {
      const p = f.parse;
      const at = `Line ${fmt(p.line)}`;
      const lines = p.issues.map((i) => `Line ${fmt(i.line)}: ${i.message}`);
      const withLines = { lines, more: p.truncated };
      switch (p.kind) {
        case "comma_decimal":
          return { ...withLines, title: `${at} uses a comma as the decimal point`, action: "Export again with a period (.) as the decimal separator." };
        case "invalid_number":
          return { ...withLines, title: `${at} has a number that can't be read`, action: EXPORT_AGAIN };
        case "non_finite":
          return { ...withLines, title: `${at} has a coordinate that isn't a number`, action: EXPORT_AGAIN };
        case "wrong_arity":
        case "element_too_short":
          return { ...withLines, title: `${at} is incomplete`, action: EXPORT_AGAIN };
        case "index_zero":
        case "index_out_of_range":
        case "invalid_index":
        case "malformed_reference":
          return { ...withLines, title: `${at} refers to a vertex that doesn't exist`, action: EXPORT_AGAIN };
        case "hidden_characters":
          return { ...withLines, title: `${at} has invisible characters`, action: "Export again, or delete them in a text editor." };
        case "encoding":
          return { ...withLines, title: "The file is saved as UTF-16 text", action: "Save or export it as UTF-8." };
        case "ambiguous_columns":
          return { ...withLines, title: `${at} has columns that could mean two things`, action: "Export the point cloud as x y z, or x y z r g b." };
        case "too_large":
          return { ...withLines, title: "Too large for the browser version", action: "Use the command-line version.", cli: true };
        default:
          return { ...withLines, title: `${at} can't be read`, action: EXPORT_AGAIN };
      }
    }
    case "read":
      return { ...base, title: "The file couldn't be read", action: "Check that it isn't open in another app, then try again.", lines: [f.message] };
    case "engine":
      return { ...base, title: "obj2cad can't start in this browser", action: "Reload the page, or try a current Chrome, Edge, Firefox or Safari.", lines: [f.message], report: true };
    case "crash":
      return { ...base, title: "obj2cad stopped on this file", action: "Try again. If it happens again, report it.", lines: [f.message], report: true };
    default:
      return { ...base, title: "Something went wrong", action: "Try again. If it happens again, report it.", lines: [f.message], report: true };
  }
}

/** A prefilled GitHub issue: version, browser and the error text. Never file contents. */
export function problemUrl(title: string, detail: string): string {
  const body = [
    "**What happened**",
    "",
    "<!-- What were you doing? Attach the file only if you can share it. -->",
    "",
    "**Error**",
    "```",
    detail.slice(0, 1500),
    "```",
    "",
    `obj2cad ${__APP_VERSION__} · ${typeof navigator === "undefined" ? "" : navigator.userAgent}`,
  ].join("\n");
  return `${REPO}/issues/new?title=${encodeURIComponent(title)}&body=${encodeURIComponent(body)}`;
}
