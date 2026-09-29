/**
 * Small copies of page frames for a canvas seen from far away. A frame is
 * decoded once at full size, drawn down to a thumbnail, and let go; what the
 * canvas keeps is the thumbnail, so a whole day of pages surveyed at 25%
 * holds a few megabytes, not one decoded page per window.
 */
const KEEP = 200;
const kept = new Map<string, Promise<ImageBitmap | null>>();

async function shrink(url: string, width: number): Promise<ImageBitmap | null> {
  try {
    const image = new Image();
    image.decoding = "async";
    image.src = url;
    await image.decode();
    if (!image.naturalWidth) return null;
    const canvas = document.createElement("canvas");
    const scale = Math.min(1, width / image.naturalWidth);
    canvas.width = Math.max(1, Math.round(image.naturalWidth * scale));
    canvas.height = Math.max(1, Math.round(image.naturalHeight * scale));
    const context = canvas.getContext("2d");
    if (!context) return null;
    context.imageSmoothingQuality = "high";
    context.drawImage(image, 0, 0, canvas.width, canvas.height);
    image.src = "";
    return await createImageBitmap(canvas);
  } catch {
    return null;
  }
}

/** The frame's thumbnail at a width, made once and kept among the most recently used. */
function frameThumbnail(url: string, width: number): Promise<ImageBitmap | null> {
  const key = `${width}|${url}`;
  const hit = kept.get(key);
  if (hit) {
    kept.delete(key);
    kept.set(key, hit);
    return hit;
  }
  const made = shrink(url, width);
  kept.set(key, made);
  if (kept.size > KEEP) {
    const [oldest, leaving] = kept.entries().next().value!;
    kept.delete(oldest);
    void leaving.then((bitmap) => bitmap?.close());
  }
  return made;
}

/** Draws a frame's thumbnail into a canvas, and the next one when the frame changes. */
export function thumbnail(canvas: HTMLCanvasElement, frame: { url: string; width: number }) {
  let current = frame.url;
  let width = frame.width;
  const draw = ({ url, width }: { url: string; width: number }) =>
    void frameThumbnail(url, width).then((bitmap) => {
      if (!bitmap || current !== url) return;
      try {
        canvas.width = bitmap.width;
        canvas.height = bitmap.height;
        canvas.getContext("2d")?.drawImage(bitmap, 0, 0);
      } catch {
        // A copy let go before it was drawn: the window keeps its sheet.
      }
    });
  draw(frame);
  return {
    update(next: { url: string; width: number }) {
      if (next.url === current && next.width === width) return;
      current = next.url;
      width = next.width;
      draw(next);
    },
  };
}
