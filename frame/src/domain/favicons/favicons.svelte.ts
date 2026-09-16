import { SvelteMap } from "svelte/reactivity";
import type { FaviconEntry, IconRef } from "$shared/ipc/bindings";
import { events } from "$shared/ipc/native-events";
import { createLifecycle, listenAll, type Unlisten } from "$shared/lib/lifecycle";

const SIDE = 32;
const BYTE_LENGTH = SIDE * SIDE * 4;
const BASE64_LENGTH = Math.ceil(BYTE_LENGTH / 3) * 4;

// Native tracks which rasters this surface holds and sends only what it has
// not delivered. Evicting below its own bound would strand an origin whose
// pixels are never resent, so this capacity must not drop below it.
const CAPACITY = 512;

type Entry = { revision: string; image: ImageData };

const rasters = new SvelteMap<string, Entry>();
const lifecycle = createLifecycle();
let initializing: Promise<void> | null = null;
let unlisten: Unlisten | null = null;

/** Site renderers decode page-controlled image formats. This surface accepts
 *  one fixed-size RGBA buffer and paints it directly. */
function decode(encoded: string): ImageData | null {
  if (encoded.length !== BASE64_LENGTH || typeof ImageData === "undefined") return null;
  try {
    const binary = atob(encoded);
    if (binary.length !== BYTE_LENGTH) return null;
    const bytes = new Uint8ClampedArray(BYTE_LENGTH);
    for (let index = 0; index < BYTE_LENGTH; index += 1) bytes[index] = binary.charCodeAt(index);
    return new ImageData(bytes, SIDE, SIDE);
  } catch {
    return null;
  }
}

function accept(entries: readonly FaviconEntry[]) {
  for (const entry of entries) {
    const image = decode(entry.rgba);
    if (!image) continue;
    rasters.delete(entry.origin);
    rasters.set(entry.origin, { revision: entry.revision, image });
  }
  while (rasters.size > CAPACITY) {
    const oldest = rasters.keys().next();
    if (oldest.done) break;
    rasters.delete(oldest.value);
  }
}

/** The pixels behind a projected reference, or null until they arrive. */
export function image(ref: IconRef | null | undefined): ImageData | null {
  if (!ref) return null;
  const entry = rasters.get(ref.origin);
  return entry?.revision === ref.revision ? entry.image : null;
}

export function init(): Promise<void> {
  if (initializing) return initializing;
  const generation = lifecycle.begin();
  initializing = listenAll([
    events.favicons.listen((event) => {
      if (lifecycle.isCurrent(generation)) accept(event.payload.entries);
    }),
  ])
    .then((listeners) => {
      if (!lifecycle.isCurrent(generation)) {
        for (const stop of listeners) stop();
        return;
      }
      unlisten = () => {
        for (const stop of listeners) stop();
      };
    })
    .catch((error: unknown) => {
      initializing = null;
      throw error;
    });
  return initializing;
}

export function dispose() {
  lifecycle.end();
  initializing = null;
  unlisten?.();
  unlisten = null;
  rasters.clear();
}
