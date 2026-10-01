<script lang="ts">
  import { onMount } from "svelte";
  import * as m from "$shared/i18n/messages";

  let { name = $bindable(), onsubmit }: { name: string; onsubmit: () => void } = $props();

  let field = $state<HTMLInputElement>();
  // Focus waits for the scene to arrive, so the caret never blinks into a
  // field that is still moving.
  onMount(() => {
    const timer = setTimeout(() => field?.focus({ preventScroll: true }), 700);
    return () => clearTimeout(timer);
  });
</script>

<label class="name">
  <span class="sr-only">{m.onb_name_label()}</span>
  <input
    bind:this={field}
    bind:value={name}
    type="text"
    autocomplete="off"
    data-1p-ignore
    data-lpignore="true"
    spellcheck="false"
    maxlength="64"
    placeholder={m.onb_name_label()}
    onkeydown={(event) => {
      if (event.key !== "Enter" || event.isComposing) return;
      event.preventDefault();
      event.stopPropagation();
      onsubmit();
    }}
  />
</label>

<style>
  .name {
    position: relative;
    display: block;
    inline-size: 360px;
  }

  input {
    inline-size: 100%;
    padding: 0 0 14px;
    border: 0;
    outline: none;
    background: transparent;
    color: var(--color-text);
    caret-color: var(--color-text);
    font: inherit;
    font-size: 30px;
    font-weight: 400;
    letter-spacing: -0.02em;
    text-align: center;
    user-select: text;
  }

  input::placeholder {
    color: var(--color-faint);
    opacity: 0.55;
  }

  /* The field is only its line: faint at rest, brighter in the middle while
     there is something to type. */
  .name::after {
    content: "";
    position: absolute;
    inset-inline: 0;
    inset-block-end: 0;
    block-size: 1px;
    background: linear-gradient(
      90deg,
      transparent,
      var(--color-border-strong) 25%,
      var(--color-border-strong) 75%,
      transparent
    );
    transition: opacity var(--motion-slow) var(--ease-out);
  }

  .name:focus-within::after {
    background: linear-gradient(
      90deg,
      transparent,
      var(--color-muted) 30%,
      var(--color-muted) 70%,
      transparent
    );
  }
</style>
