<script lang="ts">
  import { PuzzleIcon } from "@hugeicons/core-free-icons";
  import Icon from "$shared/ui/Icon";

  const RGBA_BYTES = 32 * 32 * 4;
  const BASE64_BYTES = 5_464;

  let { rgba }: { rgba: string | null } = $props();
  let canvas = $state<HTMLCanvasElement>();

  function decode(value: string | null): Uint8ClampedArray<ArrayBuffer> | null {
    if (value === null || value.length !== BASE64_BYTES) return null;
    try {
      const raw = atob(value);
      if (raw.length !== RGBA_BYTES) return null;
      const bytes = new Uint8ClampedArray(new ArrayBuffer(RGBA_BYTES));
      for (let index = 0; index < raw.length; index += 1) bytes[index] = raw.charCodeAt(index);
      return bytes;
    } catch {
      return null;
    }
  }

  let pixels = $derived(decode(rgba));

  $effect(() => {
    if (pixels === null || canvas === undefined) return;
    const context = canvas.getContext("2d", { alpha: true });
    context?.putImageData(new ImageData(pixels, 32, 32), 0, 0);
  });
</script>

{#if pixels === null}
  <Icon icon={PuzzleIcon} size={16} />
{:else}
  <canvas bind:this={canvas} width="32" height="32" class="h-[18px] w-[18px]" aria-hidden="true"
  ></canvas>
{/if}
