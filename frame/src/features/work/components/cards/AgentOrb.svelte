<script lang="ts">
  /** The agent's orb: its seed's gradient, still. A lit ring says it is at work. */
  let {
    seed = 0,
    size = 24,
    ring = false,
  }: { seed?: number; size?: number; ring?: boolean } = $props();
  const id = $props.id();
  const hue = $derived((seed * 137.508) % 360);
  const hue2 = $derived((hue + 48) % 360);
</script>

<svg class="orb" class:ring width={size} height={size} viewBox="0 0 40 40" aria-hidden="true">
  <defs>
    <radialGradient id={`orb-${id}`} cx="35%" cy="30%" r="75%">
      <stop offset="0%" stop-color={`hsl(${hue2} 80% 78%)`} />
      <stop offset="60%" stop-color={`hsl(${hue} 60% 52%)`} />
      <stop offset="100%" stop-color={`hsl(${hue} 55% 30%)`} />
    </radialGradient>
  </defs>
  <circle cx="20" cy="20" r="20" fill={`url(#orb-${id})`} />
  <circle cx="14.5" cy="15" r="6.5" fill="white" fill-opacity="0.28" />
  <circle cx="26.5" cy="24.5" r="3.8" fill="white" fill-opacity="0.18" />
</svg>

<style>
  .orb {
    display: block;
    flex: none;
    border-radius: var(--radius-capsule);
    transition: box-shadow var(--motion-fast) var(--ease-out);
  }

  .ring {
    box-shadow: 0 0 0 1px var(--color-lit);
  }
</style>
