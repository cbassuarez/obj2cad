export const fmt = (n) => n.toLocaleString("en-US");
export function bytes(n) {
    if (n < 1024)
        return `${n} B`;
    if (n < 1048576)
        return `${(n / 1024).toFixed(1)} KB`;
    if (n < 1073741824)
        return `${(n / 1048576).toFixed(1)} MB`;
    return `${(n / 1073741824).toFixed(2)} GB`;
}
const measureFormat = new Intl.NumberFormat("en-US", { maximumFractionDigits: 3 });
/** A length for display: up to 3 decimals, grouped thousands. */
export const measure = (n) => measureFormat.format(n);
export const cap = (s) => s.charAt(0).toUpperCase() + s.slice(1);
