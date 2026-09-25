<script lang="ts">
  import { Spring } from "svelte/motion";
  let {
    label,
    description,
    labelHidden = false,
    checked = false,
    disabled = false,
    onchange,
  }: {
    label: string;
    description?: string;
    labelHidden?: boolean;
    checked?: boolean;
    disabled?: boolean;
    onchange?: (checked: boolean) => void;
  } = $props();
  const uid = $props.id();

  // Apple's proportions: a long, low track with the thumb at four fifths of
  // its height and half again as wide as it is tall. Ours was too square in
  // both places, which is what made it read as a web toggle.
  const track = { width: 45, height: 20, padding: 2 };
  const thumb = { width: 26, height: 16, stretchX: 6, stretchY: 0 };
  const travel = track.width - thumb.width - track.padding * 2;

  // Two springs: where the thumb is, and how hard it is being held. Both
  // only tick while the user is interacting, then settle and go idle.
  // svelte-ignore state_referenced_locally
  const position = new Spring(checked ? travel : 0, { stiffness: 0.18, damping: 0.82 });
  const grab = new Spring(0, { stiffness: 0.16, damping: 0.75 });

  let pointer: number | null = null;
  let startX = 0;
  let startPosition = 0;
  let dragging = false;
  let suppressClick = false;

  const reduce = () =>
    window.matchMedia("(prefers-reduced-motion: reduce)").matches ||
    document.documentElement.dataset.reduceMotion === "true";

  $effect(() => {
    if (pointer === null) void position.set(checked ? travel : 0, { instant: reduce() });
  });

  let progress = $derived(Math.min(1, Math.max(0, position.current / travel)));
  let width = $derived(thumb.width + thumb.stretchX * grab.current);
  let height = $derived(thumb.height + thumb.stretchY * grab.current);
  // The stretch grows around the thumb's centre but never past the track:
  // at either end the thumb elongates inward, the way the native one does.
  let offset = $derived(
    Math.min(
      Math.max(0, position.current - (width - thumb.width) / 2),
      track.width - track.padding * 2 - width,
    ),
  );

  function commit(next: boolean) {
    if (next !== checked) {
      checked = next;
      onchange?.(next);
    }
    void position.set(next ? travel : 0, { instant: reduce() });
  }
  function down(event: PointerEvent) {
    if (disabled || (event.pointerType === "mouse" && event.button !== 0)) return;
    (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
    pointer = event.pointerId;
    startX = event.clientX;
    startPosition = position.current;
    dragging = false;
    void grab.set(1);
  }
  function move(event: PointerEvent) {
    if (pointer !== event.pointerId) return;
    const delta = event.clientX - startX;
    if (Math.abs(delta) > 3) dragging = true;
    if (!dragging) return;
    event.preventDefault();
    const rtl = getComputedStyle(event.currentTarget as HTMLElement).direction === "rtl";
    void position.set(Math.min(travel, Math.max(0, startPosition + (rtl ? -delta : delta))));
  }
  function up(event: PointerEvent) {
    if (pointer !== event.pointerId) return;
    pointer = null;
    void grab.set(0);
    if (!dragging) return;
    dragging = false;
    suppressClick = true;
    commit(position.target >= travel / 2);
  }
  function cancel() {
    pointer = null;
    dragging = false;
    void grab.set(0);
    void position.set(checked ? travel : 0);
  }
  function click() {
    if (disabled) return;
    if (suppressClick) {
      suppressClick = false;
      return;
    }
    commit(!checked);
  }
</script>

<div class="control">
  <button
    id={uid}
    type="button"
    class="switch"
    role="switch"
    aria-checked={checked}
    aria-label={label}
    aria-describedby={description ? `${uid}-help` : undefined}
    {disabled}
    data-held={grab.current > 0.05}
    style:--progress={progress}
    style:width={`${track.width}px`}
    style:height={`${track.height}px`}
    onpointerdown={down}
    onpointermove={move}
    onpointerup={up}
    onpointercancel={cancel}
    onlostpointercapture={cancel}
    onclick={click}
  >
    <span class="on" aria-hidden="true"></span>
    <span
      class="thumb"
      aria-hidden="true"
      style:--held={grab.current}
      style:width={`${width}px`}
      style:height={`${height}px`}
      style:translate={`${offset}px 0`}
      style:inset-inline-start={`${track.padding}px`}
    ></span>
  </button>
  <div class="copy" class:sr-only={labelHidden}>
    <label for={uid}>{label}</label>{#if description}<p id={`${uid}-help`} class="help">
        {description}
      </p>{/if}
  </div>
</div>

<style>
  /* The switch is physical. The thumb is a capsule on a spring: it follows
     the finger, swells while held, and settles with a small overshoot. The
     on-color fades in with the thumb's progress rather than flipping. Spring
     values are written as inline transforms and sizes each frame; nothing
     here relies on CSS transitions except the color fades. */
  .control {
    display: flex;
    align-items: center;
    gap: 10px;
  }

  .copy label {
    font-size: var(--text-body);
    line-height: 20px;
  }

  .help {
    display: block;
    margin: 0;
    font-size: var(--text-label);
    line-height: 1.6;
    color: var(--color-muted);
  }

  .switch {
    position: relative;
    flex: none;
    box-sizing: border-box;
    padding: 0;
    border: 0;
    border-radius: var(--radius-capsule);
    background: var(--color-track);
    box-shadow: var(--shadow-switch-track);
    cursor: default;
    touch-action: pan-y;
    /* stylelint-disable-next-line property-no-vendor-prefix */
    -webkit-user-select: none;
    user-select: none;
    contain: layout;
    overflow: hidden;
    transition: background-color var(--motion-fast) var(--ease-smooth);
  }

  .switch:disabled {
    opacity: 0.5;
  }

  .switch:focus-visible {
    outline: 2px solid var(--color-ring);
    outline-offset: 3px;
  }

  .switch:hover:not(:disabled) {
    background: color-mix(in srgb, var(--color-track), var(--color-text) 5%);
  }

  .on {
    position: absolute;
    inset: 0;
    border-radius: inherit;
    background: var(--color-track-on);
    opacity: var(--progress);
  }

  /* The thumb is a light capsule at rest. While held it turns to glass: the
     fill thins so the track shows through, and a bright rim catches the
     light around the edge, the way the native switch refracts. Both are
     driven by the grab spring through --held, so they bloom and settle on
     the same physics as the stretch. */
  .thumb {
    position: absolute;
    top: 50%;
    border-radius: var(--radius-capsule);
    background: color-mix(
      in srgb,
      var(--color-switch-thumb) calc(100% - var(--held) * 62%),
      var(--color-switch-glass)
    );

    --rim: color-mix(in srgb, var(--color-switch-rim) calc(var(--held) * 100%), transparent);
    --glow: color-mix(in srgb, var(--color-switch-rim) calc(var(--held) * 40%), transparent);

    box-shadow:
      var(--shadow-thumb),
      inset 0 0 0 1px var(--rim),
      inset 0 0 3px 1px var(--glow),
      0 0 0 0.5px var(--glow);
    transform: translateY(-50%);
  }

  .switch[data-held="true"] .thumb {
    will-change: translate, width, height;
  }

  .switch[aria-checked="false"] .thumb {
    background: color-mix(
      in srgb,
      var(--color-switch-thumb-off) calc(100% - var(--held) * 62%),
      var(--color-switch-glass)
    );
  }

  @media (forced-colors: active) {
    .switch {
      border: 1px solid ButtonText;
    }

    .on {
      background: Highlight;
    }

    .thumb {
      background: ButtonText;
    }
  }
</style>
