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

  // A plated mark is inset from its ground, the way a sticker leaves a margin.
  // The outer box keeps its size so plating never shifts a row.
  let plated = $derived(tone === "dark" || tone === "light");
  let inset = $derived(plated ? Math.max(1, Math.round(size * 0.12)) : 0);
  let radius = $derived(Math.max(4, Math.round(size * 0.28)));

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
  <span
    data-tone={tone}
    style:width={`${size}px`}
    style:height={`${size}px`}
    style:padding={`${inset}px`}
    style:border-radius={`${radius}px`}
    class:favicon-plate-lit={lit}
    class:animate-pulse={loading}
    class="favicon-plate shrink-0"
    aria-hidden="true"
    ><canvas
      bind:this={canvas}
      width={image.width}
      height={image.height}
      style:width={`${size - inset * 2}px`}
      style:height={`${size - inset * 2}px`}
      style:border-radius={`${Math.max(3, radius - inset)}px`}
      class="block"
    ></canvas></span
  >
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
