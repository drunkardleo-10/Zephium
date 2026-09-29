<script lang="ts">
  import Character, { type Mood } from "../Character.svelte";
  import Orb from "../Orb.svelte";
  import type { CharacterKind } from "../shapes";
  import type { OrbKind } from "../orb";

  const kinds: CharacterKind[] = ["lead", "browser", "research", "computer", "connection"];
  const names = ["Lead", "Browser", "Research", "Computer", "Connection"];
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
  const orbs: [OrbKind, string][] = [
    ["thinking", "Thinking"],
    ["searching", "Searching the web"],
    ["reading", "Reading airbnb.com"],
    ["planning", "Planning the trip"],
    ["working", "Working"],
    ["waiting", "Waiting for you"],
  ];
</script>

<div class="sheet">
  <div class="grid" style:--columns={moods.length}>
    <span></span>
    {#each moods as mood (mood)}<span class="head">{mood}</span>{/each}
    {#each kinds as kind, index (kind)}
      <span class="name">{names[index]}</span>
      {#each moods as mood (mood)}<span class="cell"><Character {kind} {mood} size={56} /></span
        >{/each}
    {/each}
  </div>
  <div class="sizes">
    {#each [14, 16, 20, 24, 32] as size (size)}
      <div class="row">
        {#each kinds as kind (kind)}<Character {kind} mood="thinking" {size} />{/each}
        <span class="caption">{size}px</span>
      </div>
    {/each}
    <div class="row">
      {#each kinds as kind (kind)}<Character {kind} mood="working" size={32} grounded />{/each}
      <span class="caption">grounded</span>
    </div>
  </div>
  <div class="orbs">
    {#each orbs as [kind, words] (kind)}
      <div class="pill"><Orb {kind} size={20} /><span>{words}</span></div>
    {/each}
  </div>
  <div class="orbs large">
    {#each orbs as [kind] (kind)}<Orb {kind} size={64} />{/each}
  </div>
  <div class="orbs">
    {#each orbs as [kind, words] (kind)}
      <div class="line"><Orb {kind} size={14} /><span>{words}</span></div>
    {/each}
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
    grid-template-columns: 96px repeat(var(--columns), 88px);
    align-items: center;
    row-gap: 18px;
  }

  .head,
  .caption {
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
    gap: 14px;
  }

  .orbs {
    display: flex;
    flex-wrap: wrap;
    gap: 14px;
  }

  .large {
    gap: 40px;
  }

  .pill {
    display: flex;
    align-items: center;
    gap: 10px;
    block-size: 36px;
    padding: 0 16px 0 10px;
    border-radius: var(--radius-capsule);
    background: var(--color-menu);
    box-shadow: var(--shadow-menu);
    font-size: var(--text-body);
  }

  .line {
    display: flex;
    align-items: center;
    gap: 8px;
    min-inline-size: 150px;
    color: var(--color-muted);
  }
</style>
