<script lang="ts">
  import type { IconSvgElement } from "@hugeicons/svelte";
  import Icon from "../Icon/Icon.svelte";

  const PREFIX = "rgba32:";
  const SIDE = 32;
  const BYTE_LENGTH = SIDE * SIDE * 4;
  const BASE64_LENGTH = Math.ceil(BYTE_LENGTH / 3) * 4;

  type Props = {
    favicon: string | null;
    loading?: boolean;
    size?: number;
    /** Resting rows dim and desaturate; the active row shows true color. */
    lit?: boolean;
    /** Drawn when a site supplies no icon, or has not supplied one yet. */
    fallback?: IconSvgElement;
  };

  let { favicon, loading = false, size = 17, lit = false, fallback }: Props = $props();
  let canvas: HTMLCanvasElement | undefined = $state();
  let failed = $state(false);
  let paintRevision = 0;

  function drawRgba(target: HTMLCanvasElement, value: string): boolean {
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

      const context = target.getContext("2d");
      if (!context) return false;

      context.putImageData(new ImageData(bytes, SIDE, SIDE), 0, 0);
      return true;
    } catch {
      return false;
    }
  }

  $effect(() => {
    const value = favicon;
    const revision = ++paintRevision;
    failed = false;

    // Let a newly selected canvas bind before painting it. The revision and
    // value checks prevent an older microtask from painting a newer favicon.
    queueMicrotask(() => {
      if (revision !== paintRevision || favicon !== value || !value || !canvas) return;
      failed = !drawRgba(canvas, value);
    });
  });
</script>

<!--
  Site-controlled image formats are decoded in the sandboxed page renderer.
  Privileged chrome receives one fixed-size RGBA buffer and paints it directly;
  this process never exposes an image decoder or a custom favicon protocol.
-->
{#if favicon && !failed}
  <canvas
    bind:this={canvas}
    width={SIDE}
    height={SIDE}
    style:width={`${size}px`}
    style:height={`${size}px`}
    class:favicon-plate-lit={lit}
    class:animate-pulse={loading}
    class="favicon-plate shrink-0 rounded-[4px]"
    aria-hidden="true"
  ></canvas>
{:else if fallback}
  <span
    style:width={`${size}px`}
    style:height={`${size}px`}
    class="flex shrink-0 items-center justify-center text-faint"
    class:animate-pulse={loading}
    aria-hidden="true"
  >
    <Icon icon={fallback} size={Math.round(size * 0.85)} />
  </span>
{:else}
  <span
    style:width={`${size}px`}
    style:height={`${size}px`}
    class={`shrink-0 rounded-full ${loading ? "animate-pulse bg-accent/70" : "bg-faint/30"}`}
    aria-hidden="true"
  ></span>
{/if}
