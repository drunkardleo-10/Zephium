<script lang="ts" module>
  import type { IconSvgElement } from "@hugeicons/svelte";

  type Part = { tag: string; attrs: Record<string, string> };

  const kebab = (name: string) => name.replace(/([a-z0-9])([A-Z])/gu, "$1-$2").toLowerCase();
  // Icon data is module constants, so a list of a few hundred rows prepares
  // each glyph once rather than once per row.
  const prepared = new WeakMap<IconSvgElement, Record<number, Part[]>>();

  function partsOf(icon: IconSvgElement, strokeWidth: number): Part[] {
    let widths = prepared.get(icon);
    if (!widths) prepared.set(icon, (widths = {}));
    let parts = widths[strokeWidth];
    if (parts) return parts;
    parts = icon.map(([tag, attrs]) => {
      const drawn: Record<string, string> = {};
      for (const [name, value] of Object.entries(attrs)) {
        if (name !== "key") drawn[kebab(name)] = String(value);
      }
      // Matches the library: a stroke width applies to every element it draws,
      // except a shape that declares itself solid with `stroke: none`.
      if (drawn.stroke === "none") return { tag, attrs: drawn };
      drawn["stroke-width"] = String(strokeWidth);
      drawn.stroke = "currentColor";
      return { tag, attrs: drawn };
    });
    widths[strokeWidth] = parts;
    return parts;
  }
</script>

<script lang="ts">
  import type { SVGAttributes } from "svelte/elements";

  type Props = {
    icon: IconSvgElement;
    size?: number | string;
    strokeWidth?: number;
    class?: string;
    style?: SVGAttributes<SVGSVGElement>["style"];
    label?: string;
  };

  let { icon, size = 16, strokeWidth = 1.7, class: className, style, label }: Props = $props();

  // Drawn as markup rather than through the library's imperative renderer, which
  // rebuilt each icon with innerHTML on mount and never redrew when `icon`
  // changed, so a glyph that follows state went stale.
  let parts = $derived(partsOf(icon, strokeWidth));
</script>

<svg
  xmlns="http://www.w3.org/2000/svg"
  width={size}
  height={size}
  viewBox="0 0 24 24"
  fill="none"
  color="currentColor"
  class={className}
  {style}
  role={label ? "img" : undefined}
  aria-label={label}
  aria-hidden={label ? undefined : "true"}
  focusable="false"
  >{#each parts as part, index (index)}<svelte:element
      this={part.tag}
      xmlns="http://www.w3.org/2000/svg"
      {...part.attrs}
    />{/each}</svg
>
