<script lang="ts">
  let {
    seed = 0,
    size = 36,
    active = false,
  }: { seed?: number; size?: number; active?: boolean } = $props();
  const hue = $derived((seed * 137.508) % 360);
  const hue2 = $derived((hue + 48) % 360);
</script>

<svg
  class="avatar"
  class:active
  width={size}
  height={size}
  viewBox="0 0 40 40"
  aria-hidden="true"
  style:--hue={String(hue)}
  style:--hue2={String(hue2)}
>
  <defs>
    <radialGradient id={`orb-${seed}`} cx="35%" cy="30%" r="75%">
      <stop offset="0%" stop-color={`hsl(${hue2} 80% 78%)`} />
      <stop offset="60%" stop-color={`hsl(${hue} 60% 52%)`} />
      <stop offset="100%" stop-color={`hsl(${hue} 55% 30%)`} />
    </radialGradient>
  </defs>
  <circle cx="20" cy="20" r="18" fill={`url(#orb-${seed})`} />
  <circle cx="14.5" cy="15" r="6" fill="white" fill-opacity="0.28" />
  <circle cx="26" cy="24" r="3.5" fill="white" fill-opacity="0.18" />
</svg>

<style>
  .avatar {
    display: block;
    border-radius: 50%;
  }

  .avatar.active {
    animation: orb-breathe 2.4s var(--ease-in-out) infinite alternate;
  }

  @keyframes orb-breathe {
    from {
      transform: scale(0.97);
      filter: saturate(0.9);
    }

    to {
      transform: scale(1.03);
      filter: saturate(1.15);
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .avatar.active {
      animation: none;
    }
  }
</style>
