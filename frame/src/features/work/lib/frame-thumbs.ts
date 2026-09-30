/**
 * Small copies of page frames and photos, at the size the canvas shows them.
 * A picture is decoded once at full size, drawn down, and let go; what the
 * canvas keeps is the copy, within a byte budget, so a whole day of pages
 * and photos holds a few megabytes, not one full decoded picture per card.
 */
const BUDGET = 24 * 1024 * 1024;
type Kept = { bitmap: Promise<ImageBitmap | null>; bytes: number };
const kept = new Map<string, Kept>();
let held = 0;

/**
 * Drawn in memory, not on the GPU: an accelerated canvas is a compositing
 * layer of its own, and one on the canvas turns the whole transformed canvas
 * into a layer with a backing store the size of everything on it.
 */
const SOFT: CanvasRenderingContext2DSettings = { willReadFrequently: true };

/** Lets go of a canvas's pixels now rather than whenever the collector gets to it. */
function drop(canvas: HTMLCanvasElement) {
  canvas.width = 0;
  canvas.height = 0;
}

async function shrink(url: string, width: number): Promise<ImageBitmap | null> {
  const image = new Image();
  image.decoding = "async";
  image.src = url;
  let scratch: HTMLCanvasElement | null = null;
  try {
    await image.decode();
    if (!image.naturalWidth) return null;
    const scale = Math.min(1, width / image.naturalWidth);
    scratch = document.createElement("canvas");
    scratch.width = Math.max(1, Math.round(image.naturalWidth * scale));
    scratch.height = Math.max(1, Math.round(image.naturalHeight * scale));
    const context = scratch.getContext("2d", SOFT);
    if (!context) return null;
    context.imageSmoothingQuality = "high";
    context.drawImage(image, 0, 0, scratch.width, scratch.height);
    return await createImageBitmap(scratch);
  } catch {
    return null;
  } finally {
    image.src = "";
    if (scratch) drop(scratch);
  }
}

function evict() {
  for (const [key, entry] of kept) {
    if (held <= BUDGET) return;
    kept.delete(key);
    held -= entry.bytes;
    void entry.bitmap.then((bitmap) => bitmap?.close());
  }
}

/** The frame's thumbnail at a width, made once and kept among the most recently used. */
function frameThumbnail(url: string, width: number): Promise<ImageBitmap | null> {
  const key = `${width}|${url}`;
  const hit = kept.get(key);
  if (hit) {
    kept.delete(key);
    kept.set(key, hit);
    return hit.bitmap;
  }
  const entry: Kept = { bitmap: shrink(url, width), bytes: 0 };
  kept.set(key, entry);
  void entry.bitmap.then((bitmap) => {
    if (kept.get(key) !== entry) return;
    if (!bitmap) {
      kept.delete(key);
      return;
    }
    entry.bytes = bitmap.width * bitmap.height * 4;
    held += entry.bytes;
    evict();
  });
  return entry.bitmap;
}

type Thumb = { url: string; width: number; onmissing?: () => void };
/**
 * Draws a picture's small copy into a canvas, and the next one when the
 * picture changes; one that won't load says so. The canvas lets its pixels
 * go as it leaves the page.
 */
export function thumbnail(canvas: HTMLCanvasElement, frame: Thumb) {
  let current = frame.url;
  let width = frame.width;
  let missing = frame.onmissing;
  let gone = false;
  const draw = ({ url, width }: Thumb) =>
    void frameThumbnail(url, width).then((bitmap) => {
      if (gone || current !== url) return;
      if (!bitmap) return missing?.();
      try {
        canvas.width = bitmap.width;
        canvas.height = bitmap.height;
        canvas.getContext("2d", SOFT)?.drawImage(bitmap, 0, 0);
      } catch {
        // A copy let go before it was drawn: the window keeps its sheet.
      }
    });
  draw(frame);
  return {
    update(next: Thumb) {
      missing = next.onmissing;
      if (next.url === current && next.width === width) return;
      current = next.url;
      width = next.width;
      draw(next);
    },
    destroy() {
      gone = true;
      // After any leaving motion has drawn from it.
      setTimeout(() => {
        if (!canvas.isConnected) drop(canvas);
      }, 1000);
    },
  };
}
