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
