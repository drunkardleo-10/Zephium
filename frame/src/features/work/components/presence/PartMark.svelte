<script lang="ts">
  import Icon from "$shared/ui/Icon";
  import { serviceKey, serviceMark } from "$domain/connections";
  import type { PartView } from "../../lib/canvas-model";
  import { BrowserIcon, ComputerTerminal01Icon, Search01Icon } from "../../lib/icons";
  import HostGlyph from "../cards/HostGlyph.svelte";

  /** A part's mark is its service: the site, the search, the Mac or the connection it works through. */
  let { part }: { part: Pick<PartView, "helper" | "host" | "connection" | "title"> } = $props();

  const site = $derived(
    part.helper === "browser" || (part.helper === "lead" && !!part.host) ? part.host : undefined,
  );
</script>

<span class="part-mark" class:plain={!site}>
  {#if site}<HostGlyph host={site} size={16} initial={false} />
  {:else}<Icon
      icon={part.helper === "research"
        ? Search01Icon
        : part.helper === "computer"
          ? ComputerTerminal01Icon
          : part.helper === "lead"
            ? BrowserIcon
            : serviceMark(serviceKey(part.connection, part.title))}
      size={14}
    />{/if}
</span>

<style>
  .part-mark {
    display: grid;
    flex: none;
    place-items: center;
    inline-size: 16px;
    block-size: 18px;
  }

  .plain {
    color: var(--color-muted);
  }
</style>
