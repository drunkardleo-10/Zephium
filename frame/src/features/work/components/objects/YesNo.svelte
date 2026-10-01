<script lang="ts">
  import * as m from "$shared/i18n/messages";
  /** Yes as a check, no as a dash, partly as a half-filled ring; unknown says nothing. */
  let { value, size = 16 }: { value: "yes" | "no" | "partial" | "unknown"; size?: number } =
    $props();
  const label = $derived(
    value === "yes"
      ? m.work_object_yes()
      : value === "no"
        ? m.work_object_no()
        : value === "partial"
          ? m.work_object_partly()
          : m.work_object_unknown(),
  );
</script>

<svg
  class="mark {value}"
  viewBox="0 0 16 16"
  width={size}
  height={size}
  role="img"
  aria-label={label}
>
  <title>{label}</title>
  {#if value === "yes"}
    <circle cx="8" cy="8" r="7.25" class="disc" />
    <path d="M4.9 8.3 7 10.3 11.2 5.9" class="stroke" />
  {:else if value === "no"}
    <path d="M5.25 8h5.5" class="stroke" />
  {:else if value === "partial"}
    <circle cx="8" cy="8" r="6.5" class="ring" />
    <path d="M8 1.5a6.5 6.5 0 0 1 0 13z" class="half" />
  {:else}
    <circle cx="8" cy="8" r="1.2" class="dot" />
  {/if}
</svg>

<style>
  .mark {
    display: inline-block;
    flex: none;
    vertical-align: middle;
  }

  .disc {
    fill: var(--color-success);
  }

  .yes .stroke {
    fill: none;
    stroke: var(--color-surface);
    stroke-width: 1.8;
    stroke-linecap: round;
    stroke-linejoin: round;
  }

  .no .stroke {
    fill: none;
    stroke: var(--color-faint);
    stroke-width: 1.6;
    stroke-linecap: round;
  }

  .ring {
    fill: none;
    stroke: var(--color-warning);
    stroke-width: 1.5;
  }

  .half {
    fill: var(--color-warning);
  }

  .dot {
    fill: var(--color-faint);
  }
</style>
