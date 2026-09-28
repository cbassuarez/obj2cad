export const fmt = (n: number) => n.toLocaleString("en-US");

export function bytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1048576) return `${(n / 1024).toFixed(1)} KB`;
  return `${(n / 1048576).toFixed(1)} MB`;
}

/** Dimension text: up to 3 decimals, trailing zeros kept for a drafting look. */
export const dim = (n: number) => n.toFixed(3);

export const cap = (s: string) => s.charAt(0).toUpperCase() + s.slice(1);
