<script lang="ts">
  import { HugeiconsIcon, type IconSvgElement } from "@hugeicons/svelte";
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
</script>

<HugeiconsIcon
  {icon}
  {size}
  {strokeWidth}
  color="currentColor"
  class={className}
  {style}
  role={label ? "img" : undefined}
  aria-label={label}
  aria-hidden={label ? undefined : "true"}
  focusable="false"
/>
