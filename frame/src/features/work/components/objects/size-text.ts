/** A file's size as the system writes it: decimal units, one figure after the point below 10. */
export function size(bytes: number): string {
  const units = ["bytes", "KB", "MB", "GB", "TB"];
  let value = bytes;
  let unit = 0;
  while (value >= 1000 && unit < units.length - 1) {
    value /= 1000;
    unit += 1;
  }
  const digits = unit === 0 || value >= 10 ? 0 : 1;
  return `${new Intl.NumberFormat(undefined, { maximumFractionDigits: digits }).format(value)} ${units[unit]}`;
}
