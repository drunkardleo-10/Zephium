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
  let hasChecks = $derived(entries.some((entry) => entry.kind === "item" && entry.checked));
</script>

<DropdownMenu.Root>
  <DropdownMenu.Trigger class={triggerClass} aria-label={label}>
    {@render trigger()}
  </DropdownMenu.Trigger>
  <DropdownMenu.Portal>
    <DropdownMenu.Content class="ui-menu" {side} {align} sideOffset={6}>
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
            {#if entry.hint}<span class="ui-menu-hint" aria-hidden="true"
                >{#each entry.hint.split(" ") as key, i (i)}<kbd>{key}</kbd>{/each}</span
              >{/if}
            {#if hasChecks}
              <span class="ui-menu-check" aria-hidden="true"
                >{#if entry.checked}<svg
                    viewBox="0 0 12 12"
                    width="12"
                    height="12"
                    fill="none"
                    stroke="currentColor"
                    ><path
                      d="M2.5 6.5l2.5 2.5 4.5-5"
                      stroke-width="1.8"
                      stroke-linecap="round"
                      stroke-linejoin="round"
                    /></svg
                  >{/if}</span
              >
            {/if}
          </DropdownMenu.Item>
        {/if}
      {/each}
    </DropdownMenu.Content>
  </DropdownMenu.Portal>
</DropdownMenu.Root>
