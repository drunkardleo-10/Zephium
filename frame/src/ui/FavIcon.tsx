import { createEffect, createSignal, on, Show } from "solid-js";

const PREFIX = "rgba32:";
const SIDE = 32;
const BYTE_LENGTH = SIDE * SIDE * 4;
const BASE64_LENGTH = Math.ceil(BYTE_LENGTH / 3) * 4;

function drawRgba(canvas: HTMLCanvasElement, value: string): boolean {
  if (!value.startsWith(PREFIX)) return false;
  const encoded = value.slice(PREFIX.length);
  if (encoded.length !== BASE64_LENGTH) return false;

  try {
    const binary = atob(encoded);
    if (binary.length !== BYTE_LENGTH) return false;
    const bytes = new Uint8ClampedArray(BYTE_LENGTH);
    for (let index = 0; index < BYTE_LENGTH; index += 1) {
      bytes[index] = binary.charCodeAt(index);
    }
    const context = canvas.getContext("2d");
    if (!context) return false;
    context.putImageData(new ImageData(bytes, SIDE, SIDE), 0, 0);
    return true;
  } catch {
    return false;
  }
}

// Site-controlled image formats are decoded in the sandboxed page renderer.
// Privileged chrome receives only one fixed-size RGBA buffer and paints it to
// canvas, avoiding an <img> decoder or custom protocol in this process.
export function FavIcon(props: { favicon: string | null; loading?: boolean }) {
  const [failed, setFailed] = createSignal(false);
  let canvas: HTMLCanvasElement | undefined;

  const paint = (value: string | null) => {
    if (canvas && value && !drawRgba(canvas, value)) setFailed(true);
  };

  createEffect(
    on(
      () => props.favicon,
      (value) => {
        setFailed(false);
        queueMicrotask(() => {
          if (props.favicon === value) paint(value);
        });
      },
    ),
  );

  return (
    <Show
      when={props.favicon && !failed()}
      fallback={
        <span
          class="h-3.5 w-3.5 shrink-0 rounded-full bg-faint/40"
          classList={{ "animate-pulse bg-accent/70": props.loading }}
        />
      }
    >
      <canvas
        ref={(element) => {
          canvas = element;
          paint(props.favicon);
        }}
        width={SIDE}
        height={SIDE}
        class="h-3.5 w-3.5 shrink-0 rounded"
        classList={{ "animate-pulse opacity-60": props.loading }}
      />
    </Show>
  );
}
