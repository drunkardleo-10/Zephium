<script module lang="ts">
  /** Whether a name is Zephium itself, which has no site mark to borrow. */
  export const isZephium = (name: string) => /^zephium\b/iu.test(name.trim());
</script>

<script lang="ts">
  /** Zephium's own mark: two lanes and the point between them, on its dark tile. */
  let { size = 16 }: { size?: number } = $props();
</script>

<svg class="zephium" width={size} height={size} viewBox="0 0 24 24" aria-hidden="true">
  <rect
    x="0.5"
    y="0.5"
    width="23"
    height="23"
    rx="5.25"
    fill="#2c2c2e"
    stroke="#fff"
    stroke-opacity="0.16"
  />
  <g fill="none" stroke="#fff" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round">
    <path d="M5 11.2h2.6c.9 0 1.7-.4 2.3-1.1l1.1-1.2c.6-.7 1.5-1 2.4-1H19" />
    <path d="M5 16.2h6.6c.9 0 1.7-.4 2.3-1.1l1.1-1.2c.6-.7 1.5-1 2.4-1H19" />
  </g>
  <rect
    x="10.7"
    y="10.8"
    width="3"
    height="3"
    rx="0.7"
    fill="#fff"
    transform="rotate(45 12.2 12.3)"
  />
</svg>

<style>
  .zephium {
    flex: none;
    border-radius: var(--radius-inset);
  }
</style>
