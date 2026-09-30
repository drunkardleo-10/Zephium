<script lang="ts">
  import Character, { type Mood } from "../Character.svelte";
  import Orb from "../Orb.svelte";
  import Shimmer from "../Shimmer.svelte";
  import type { CharacterKind, LeadLook } from "../shapes";

  const helpers: CharacterKind[] = ["browser", "research", "computer", "connection"];
  const looks: LeadLook[] = ["pearl", "orb", "drop", "prism", "gem", "egg"];
  const moods: Mood[] = [
    "rest",
    "thinking",
    "reading",
    "searching",
    "working",
    "waiting",
    "done",
    "stopped",
  ];
  const lines = ["Thinking", "Searching flights", "Reading airbnb.com", "Waiting for you"];
</script>

<div class="sheet">
  <div class="grid" style:--columns={moods.length}>
    <span></span>
    {#each moods as mood (mood)}<span class="head">{mood}</span>{/each}
    {#each looks as look (look)}
      <span class="name">Lead · {look}</span>
      {#each moods as mood (mood)}<span class="cell"
          ><Character kind="lead" {look} {mood} size={56} /></span
        >{/each}
    {/each}
    {#each helpers as kind (kind)}
      <span class="name">{kind}</span>
      {#each moods as mood (mood)}<span class="cell"><Character {kind} {mood} size={56} /></span
        >{/each}
    {/each}
  </div>
  <div class="sizes">
    {#each [14, 18, 22, 28] as size (size)}
      <div class="row">
        {#each looks as look (look)}<Character kind="lead" {look} mood="thinking" {size} />{/each}
        {#each helpers as kind (kind)}<Character {kind} mood="thinking" {size} />{/each}
        <span class="caption">{size}px</span>
      </div>
    {/each}
  </div>
  <div class="orbs">
    {#each lines as words (words)}
      <div class="pill"><Orb size={18} /><Shimmer text={words} /></div>
    {/each}
  </div>
  <div class="orbs zoom">
    {#each lines.slice(0, 2) as words (words)}
      <div class="pill"><Orb size={18} /><Shimmer text={words} /></div>
    {/each}
    <Character kind="lead" look="orb" mood="thinking" size={18} />
    <Character kind="browser" mood="reading" size={18} />
    <Character kind="lead" look="prism" mood="done" size={18} />
  </div>
  <div class="orbs large">
    {#each [14, 18, 24, 48, 96] as size (size)}<Orb {size} />{/each}
  </div>
</div>

<style>
  .sheet {
    display: flex;
    flex-direction: column;
    gap: 36px;
    padding: 36px;
    background: var(--color-canvas);
    color: var(--color-text);
    font-size: var(--text-label);
  }

  .grid {
    display: grid;
    grid-template-columns: 110px repeat(var(--columns), 84px);
    align-items: center;
    row-gap: 14px;
  }

  .head,
  .caption,
  .name {
    color: var(--color-faint);
  }

  .cell {
    display: grid;
    place-items: center;
  }

  .sizes {
    display: flex;
    gap: 40px;
  }

  .row {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 12px;
  }

  .orbs {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 14px;
  }

  .zoom {
    zoom: 3;
  }

  .large {
    gap: 40px;
  }

  .pill {
    display: flex;
    align-items: center;
    gap: 9px;
    block-size: 36px;
    padding: 0 16px 0 10px;
    border-radius: var(--radius-capsule);
    background: var(--color-menu);
    box-shadow: var(--shadow-menu);
    font-size: var(--text-body);
  }
</style>
