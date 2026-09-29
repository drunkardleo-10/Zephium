<script lang="ts">
  import Icon from "$shared/ui/Icon";
  import { Character, type Mood } from "$shared/ui/presence";
  import { serviceKey, serviceMark } from "$domain/connections";
  import type { PartView } from "../../lib/canvas-model";
  import { ComputerTerminal01Icon, Search01Icon } from "../../lib/icons";
  import HostGlyph from "../cards/HostGlyph.svelte";

  /**
   * A part's mark is its service; while the part works its helper stands just
   * before it, where the line arrives, and says so when it is done.
   */
  let { part }: { part: Pick<PartView, "helper" | "state" | "host" | "connection" | "title"> } =
    $props();

  const DOING: Record<PartView["helper"], Mood> = {
    browser: "reading",
    research: "searching",
    computer: "working",
    connection: "working",
  };
  const present = $derived(part.state === "running" || part.state === "waiting");
  /** A helper seen finishing stays a moment to say so, then gives its place back to the mark. */
  let parting = $state(false);
  let was: PartView["state"] | undefined;
  $effect(() => {
    const now = part.state;
    const finished = (was === "running" || was === "waiting") && now === "done";
    was = now;
    if (!finished) return;
    parting = true;
    const timer = setTimeout(() => (parting = false), 1400);
    return () => clearTimeout(timer);
  });
  const mood = $derived<Mood>(
    part.state === "waiting" ? "waiting" : present ? DOING[part.helper] : "done",
  );
</script>

{#snippet service(size: number)}
  {#if part.helper === "browser"}<HostGlyph host={part.host ?? ""} {size} initial={false} />
  {:else}<Icon
      icon={part.helper === "research"
        ? Search01Icon
        : part.helper === "computer"
          ? ComputerTerminal01Icon
          : serviceMark(serviceKey(part.connection, part.title))}
      {size}
    />{/if}
{/snippet}

<span class="part-mark" class:plain={part.helper !== "browser"}>
  <span class="service">{@render service(part.helper === "browser" ? 16 : 14)}</span>
  {#if present || parting}<span class="helper" class:parting={!present}
      ><Character kind={part.helper} {mood} size={18} /></span
    >{/if}
</span>

<style>
  .part-mark {
    position: relative;
    display: grid;
    flex: none;
    place-items: center;
    inline-size: 16px;
    block-size: 18px;
  }

  .service {
    display: grid;
    place-items: center;
  }

  .plain .service {
    color: var(--color-muted);
  }

  /* The helper stands where its part's line arrives, holding the end of it. */
  .helper {
    position: absolute;
    inset-block-start: 50%;
    inset-inline-end: calc(100% + 3px);
    margin-block-start: -9px;
    animation: helper-in var(--motion-slow) var(--ease-spring);
  }

  .helper.parting {
    opacity: 0;
    transform: scale(0.6);
    transition:
      opacity var(--motion-slow) var(--ease-exit) 1s,
      transform var(--motion-slow) var(--ease-exit) 1s;
  }

  @keyframes helper-in {
    from {
      opacity: 0;
      transform: scale(0.4);
    }
  }
</style>
