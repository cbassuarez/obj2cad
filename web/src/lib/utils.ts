import { clsx, type ClassValue } from "clsx";
import { twMerge } from "tailwind-merge";

/** shadcn's class combiner: later Tailwind classes win over earlier ones. */
export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}

const mac = typeof navigator !== "undefined" && /Mac|iPhone|iPad/.test(navigator.platform);

/** A keyboard shortcut as this platform writes it: "⌘S" or "Ctrl+S". */
export const shortcut = (key: string) => (mac ? `⌘${key}` : `Ctrl+${key}`);
