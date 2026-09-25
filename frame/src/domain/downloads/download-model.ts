import type { DownloadView } from "$shared/ipc/bindings";

export function downloadProgress(entry: DownloadView): number | undefined {
  const received = Number(entry.received);
  const total = entry.total === null ? NaN : Number(entry.total);
  return Number.isFinite(total) && total > 0 && Number.isFinite(received)
    ? Math.max(0, Math.min(1, received / total))
    : undefined;
}

const integers = new Intl.NumberFormat(undefined, { maximumFractionDigits: 0 });
const fractions = new Intl.NumberFormat(undefined, { maximumFractionDigits: 1 });

export function formatDownloadBytes(value: string | null): string {
  if (value === null) return "—";
  const bytes = Number(value);
  if (!Number.isFinite(bytes) || bytes < 0) return "—";
  const units = ["B", "KB", "MB", "GB", "TB"];
  const at = Math.min(4, Math.max(0, Math.floor(Math.log2(Math.max(1, bytes)) / 10)));
  return `${(at === 0 ? integers : fractions).format(bytes / 1024 ** at)} ${units[at]}`;
}
