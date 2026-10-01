import type { MediaView } from "./board/types";

const VIDEO_ID = /^[\w-]{11}$/u;

/** The video a YouTube address plays: watch, youtu.be and shorts links. */
export function youtubeId(url: string): string | null {
  let parsed: URL;
  try {
    parsed = new URL(url);
  } catch {
    return null;
  }
  if (parsed.protocol !== "https:" && parsed.protocol !== "http:") return null;
  const host = parsed.hostname.toLowerCase().replace(/^(www|m)\./u, "");
  const path = parsed.pathname.split("/").filter(Boolean);
  const id =
    host === "youtu.be"
      ? path[0]
      : host === "youtube.com" && path[0] === "watch"
        ? parsed.searchParams.get("v")
        : host === "youtube.com" && path[0] === "shorts"
          ? path[1]
          : null;
  return id && VIDEO_ID.test(id) ? id : null;
}

/** The public thumbnail Rust admits as the link card's picture. */
export function youtubeThumbnail(url: string): string | null {
  const id = youtubeId(url);
  return id ? `https://i.ytimg.com/vi/${id}/hqdefault.jpg` : null;
}

/** The start a YouTube address asks for: `t=90`, `t=1m30s` or `start=90`, in seconds. */
function startOf(url: string): number | undefined {
  let parsed: URL;
  try {
    parsed = new URL(url);
  } catch {
    return undefined;
  }
  const value = parsed.searchParams.get("t") ?? parsed.searchParams.get("start");
  if (!value) return undefined;
  const parts = /^(?:(\d+)h)?(?:(\d+)m)?(?:(\d+)s?)?$/u.exec(value);
  if (!parts) return undefined;
  const seconds = Number(parts[1] ?? 0) * 3600 + Number(parts[2] ?? 0) * 60 + Number(parts[3] ?? 0);
  return seconds > 0 ? seconds : undefined;
}

/**
 * A link that is a YouTube video, as the video it is: its poster the admitted
 * thumbnail when there is one, its start where the address points. Other links
 * stay links.
 */
export function linkVideo(
  id: string,
  url: string,
  { title, poster }: { title?: string; poster?: MediaView["poster"] } = {},
): MediaView | null {
  if (!youtubeId(url)) return null;
  const start = startOf(url);
  return {
    kind: "media",
    id,
    media: "video",
    url,
    provider: "youtube",
    ...(title ? { title } : {}),
    ...(poster ? { poster } : {}),
    ...(start ? { startSecs: start } : {}),
  };
}
