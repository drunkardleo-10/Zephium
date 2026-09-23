<script lang="ts" module>
  import type { IconSvgElement } from "@hugeicons/svelte";
  export type MenuEntry =
    | {
        kind: "item";
        id: string;
        label: string;
        icon?: IconSvgElement;
        hint?: string;
        checked?: boolean;
        danger?: boolean;
        disabled?: boolean;
      }
    | { kind: "separator" }
    | { kind: "heading"; label: string };
</script>

<script lang="ts">
  import { DropdownMenu } from "bits-ui";
  import type { Snippet } from "svelte";
  import Icon from "../Icon/Icon.svelte";
  import Mark from "./Mark.svelte";
  import "./popover.css";
  let {
    label,
    entries,
    trigger,
    triggerClass = "",
    side = "bottom",
    align = "start",
    onselect,
  }: {
    label: string;
    entries: MenuEntry[];
    trigger: Snippet;
    triggerClass?: string;
    side?: "top" | "bottom" | "left" | "right";
    align?: "start" | "center" | "end";
    onselect: (id: string) => void;
  } = $props();
  // A menu that can express a choice keeps the mark column on every row, so
  // the labels do not step sideways as the choice moves.
  let choice = $derived(entries.some((entry) => entry.kind === "item" && "checked" in entry));
</script>

<DropdownMenu.Root>
  <DropdownMenu.Trigger class={triggerClass} aria-label={label}>
    {@render trigger()}
  </DropdownMenu.Trigger>
  <DropdownMenu.Portal>
    <DropdownMenu.Content
      class="ui-menu ui-menu-scroll"
      {side}
      {align}
      sideOffset={6}
      collisionPadding={10}
    >
      {#each entries as entry, index (index)}
        {#if entry.kind === "separator"}
          <DropdownMenu.Separator class="ui-menu-separator" />
        {:else if entry.kind === "heading"}
          <div class="ui-menu-heading" role="presentation">{entry.label}</div>
        {:else}
          <DropdownMenu.Item
            class="ui-menu-item"
            textValue={entry.label}
            disabled={entry.disabled}
            data-danger={entry.danger || undefined}
            onSelect={() => onselect(entry.id)}
          >
            {#if entry.icon}
              <span class="ui-menu-icon" aria-hidden="true"
                ><Icon icon={entry.icon} size={15} /></span
              >
            {/if}
            {entry.label}
            {#if entry.hint}<span class="ui-menu-hint" aria-hidden="true">{entry.hint}</span>{/if}
            {#if choice}<Mark on={entry.checked === true} />{/if}
          </DropdownMenu.Item>
        {/if}
      {/each}
    </DropdownMenu.Content>
  </DropdownMenu.Portal>
</DropdownMenu.Root>
