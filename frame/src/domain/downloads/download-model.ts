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

export type DownloadKind =
  "image" | "video" | "audio" | "archive" | "pdf" | "code" | "app" | "file";

const KINDS: Record<string, DownloadKind> = {};
for (const [kind, extensions] of [
  ["image", "png jpg jpeg gif webp avif heic svg bmp tif tiff ico"],
  ["video", "mp4 mov m4v webm mkv avi"],
  ["audio", "mp3 m4a aac wav flac ogg opus"],
  ["archive", "zip rar 7z tar gz tgz bz2 xz"],
  ["pdf", "pdf"],
  ["code", "js ts json html css xml csv txt md rs py sh"],
  ["app", "dmg pkg exe msi deb rpm appimage apk"],
] as const)
  for (const extension of extensions.split(" ")) KINDS[extension] = kind;

/** What sort of file a name suggests, for its glyph. */
export function downloadKind(filename: string): DownloadKind {
  const dot = filename.lastIndexOf(".");
  return (dot > 0 && KINDS[filename.slice(dot + 1).toLowerCase()]) || "file";
}
