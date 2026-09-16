<script lang="ts">
  import type { IconSvgElement } from "@hugeicons/svelte";
  import Icon from "../Icon/Icon.svelte";

  type Props = {
    /** Decoded 32x32 RGBA pixels, or null until they arrive. */
    image: ImageData | null;
    /** How the mark sits against a surface, so a near-neutral icon the colour
     *  of the chrome behind it can be given its own ground. */
    tone?: "dark" | "light" | "mid";
    loading?: boolean;
    size?: number;
    /** Resting rows dim and desaturate; the active row shows true color. */
    lit?: boolean;
    /** Drawn when a site supplies no icon, or has not supplied one yet. */
    fallback?: IconSvgElement;
  };

  let { image, tone = "mid", loading = false, size = 17, lit = false, fallback }: Props = $props();
  let canvas: HTMLCanvasElement | undefined = $state();

  $effect(() => {
    const pixels = image;
    if (!pixels || !canvas) return;
    canvas.getContext("2d")?.putImageData(pixels, 0, 0);
  });
</script>

<!--
  Site-controlled image formats are decoded in the sandboxed page renderer.
  Privileged chrome receives one fixed-size RGBA buffer and paints it directly;
  this process never exposes an image decoder or a custom favicon protocol.
-->
{#if image}
  <canvas
    bind:this={canvas}
    data-tone={tone}
    width={image.width}
    height={image.height}
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
