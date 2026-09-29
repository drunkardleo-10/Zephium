<script lang="ts">
  import { tick } from "svelte";
  import { commands } from "$shared/ipc/bindings";
  import Button from "$shared/ui/Button";
  import Icon from "$shared/ui/Icon";
  import SegmentedControl from "$shared/ui/SegmentedControl";
  import * as m from "$shared/i18n/messages";
  import { FolderAddIcon } from "../../lib/icons";
  import {
    PERSONAS,
    WORKFLOWS,
    composed,
    recentWorkflows,
    type Persona,
    type RecentWork,
    type Workflow,
  } from "../../lib/start/workflows";
  import Vignette from "./Vignette.svelte";

  /**
   * A new work's first screen: workflows by what the person does. A tap
   * writes the workflow's request into the ask bar and asks for its one
   * input; nothing runs until the person sends it.
   */
  let {
    value = $bindable(""),
    field,
    works = [],
    profile,
    disabled = false,
    inset = 0,
  }: {
    /** The ask bar's text. */
    value?: string;
    /** The ask bar, whose field takes the focus. */
    field?: HTMLElement;
    works?: readonly RecentWork[];
    profile: string;
    disabled?: boolean;
    /** Room the ask bar takes at the bottom, in px. */
    inset?: number;
  } = $props();

  const recent = $derived(recentWorkflows(works));
  let chosen = $state<Persona | null>(null);
  const persona = $derived(chosen ?? recent[0]?.persona ?? "developer");
  const flows = $derived(WORKFLOWS.filter((flow) => flow.persona === persona));
  let picked = $state.raw<Workflow | null>(null);
  let folderPending = $state(false);

  const lower = (text: string) => text.trim().toLocaleLowerCase();
  /** The workflow whose request still leads the bar; typing it away lets it go. */
  const active = $derived(
    picked && lower(value).startsWith(lower(picked.request())) ? picked : null,
  );
  /**
   * What the person already wrote, which becomes the next workflow's input:
   * their own words, or another workflow's input of the same kind.
   */
  function ownWords(next: Workflow): string {
    const text = value.trim();
    const leading = WORKFLOWS.find((flow) => lower(text).startsWith(lower(flow.request())));
    if (!leading) return text;
    if (leading.input !== next.input) return "";
    return text
      .slice(leading.request().length)
      .trim()
      .replace(/^[:.]\s*/u, "");
  }

  async function place(text: string) {
    value = text;
    await tick();
    const area = field?.querySelector<HTMLTextAreaElement>("textarea");
    if (!area) return;
    area.focus();
    area.setSelectionRange(area.value.length, area.value.length);
  }

  function pick(flow: Workflow) {
    if (disabled) return;
    const words = ownWords(flow);
    picked = flow;
    chosen = flow.persona;
    void place(words || flow.input === "none" ? composed(flow, words) : `${flow.request()} `);
  }

  async function chooseFolder(flow: Workflow) {
    if (folderPending || disabled) return;
    folderPending = true;
    try {
      const chosenFolder = await commands.workPickFolder(profile).catch(() => null);
      if (chosenFolder?.status !== "ok" || chosenFolder.data?.kind !== "admitted") return;
      picked = flow;
      await place(composed(flow, chosenFolder.data.path));
    } finally {
      folderPending = false;
    }
  }

  const places = (flow: Workflow) => m.work_start_reads({ places: flow.reads.join(", ") });
</script>

<div class="start" style:--inset="{inset}px">
  <section class="panel" aria-labelledby="work-start-title">
    <header>
      <h2 id="work-start-title">{m.work_start_title()}</h2>
      <p>{m.work_start_hint()}</p>
    </header>

    {#if recent.length}<div class="for-you">
        <span class="label">{m.work_start_for_you()}</span>
        {#each recent as flow (flow.skill)}<button
            type="button"
            class="recent hue-{flow.hue}"
            aria-pressed={active === flow}
            {disabled}
            onclick={() => pick(flow)}
          >
            <span class="swatch" aria-hidden="true"></span>{flow.title()}
          </button>{/each}
      </div>{/if}

    <SegmentedControl
      label={m.work_start_roles()}
      size="compact"
      options={PERSONAS.map((option) => ({ value: option.id, label: option.label() }))}
      value={persona}
      onchange={(next) => (chosen = next as Persona)}
    />

    {#key persona}<div class="flows" style:--count={flows.length}>
        {#each flows as flow (flow.skill)}<button
            type="button"
            class="flow hue-{flow.hue}"
            aria-pressed={active === flow}
            aria-describedby="work-start-{flow.skill}"
            {disabled}
            onclick={() => pick(flow)}
          >
            <span class="art"><Vignette shape={flow.shape} /></span>
            <span class="words">
              <span class="title">{flow.title()}</span>
              <span class="line" id="work-start-{flow.skill}">{flow.line()}</span>
              {#if flow.reads.length}<span class="reads">{places(flow)}</span>{/if}
            </span>
          </button>{/each}
      </div>{/key}

    <footer aria-live="polite">
      {#if active}<p class="ask">
          {active.input === "none" ? m.work_start_ready() : active.ask()}
        </p>
        {#if active.input === "folder"}<Button
            size="compact"
            variant="ghost"
            pending={folderPending}
            {disabled}
            onclick={() => void chooseFolder(active)}
            ><Icon icon={FolderAddIcon} size={14} />{m.work_start_choose_folder()}</Button
          >{/if}{/if}
    </footer>
  </section>
</div>

<style>
  .start {
    position: absolute;
    inset: 0 0 var(--inset) 0;
    display: grid;
    place-items: center;
    padding: 64px 24px 16px;
    overflow: hidden;
    pointer-events: none;
  }

  .panel {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 20px;
    inline-size: min(100%, 900px);
    pointer-events: auto;
    animation: arrive var(--motion-page) var(--ease-out) both;
  }

  header {
    display: grid;
    gap: 6px;
    text-align: center;
  }

  h2 {
    margin: 0;
    color: var(--color-text);
    font-size: var(--text-title);
    font-weight: 600;
    letter-spacing: -0.01em;
    line-height: 1.2;
  }

  header p {
    margin: 0;
    color: var(--color-muted);
    font-size: var(--text-body);
  }

  .for-you {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: center;
    gap: 6px;
  }

  .label {
    margin-inline-end: 4px;
    color: var(--color-faint);
    font-size: var(--text-label);
    font-weight: 500;
  }

  .recent {
    display: inline-flex;
    align-items: center;
    gap: 7px;
    block-size: var(--control-compact);
    padding-inline: 8px 10px;
    border: 0;
    border-radius: var(--radius-control-compact);
    background: var(--color-fill);
    color: var(--color-label-secondary);
    font: inherit;
    font-size: var(--text-label);
    font-weight: 500;
    cursor: default;
    transition: background-color var(--motion-instant) var(--ease-out);
  }

  .recent:hover:not(:disabled) {
    background: var(--color-fill-hover);
  }

  .recent[aria-pressed="true"] {
    background: var(--hue-wash);
    color: var(--color-text);
  }

  .swatch {
    inline-size: 8px;
    block-size: 8px;
    border-radius: var(--radius-capsule);
    background: var(--hue);
  }

  .flows {
    display: grid;
    grid-template-columns: repeat(var(--count), minmax(0, 212px));
    justify-content: center;
    gap: 12px;
    inline-size: 100%;
    animation: settle var(--motion-base) var(--ease-out) both;
  }

  .flow {
    display: flex;
    flex-direction: column;
    min-inline-size: 0;
    padding: 0;
    overflow: hidden;
    border: 0;
    border-radius: var(--radius-card);
    outline: 1px solid transparent;
    outline-offset: -1px;
    background: var(--color-fill);
    color: inherit;
    font: inherit;
    text-align: start;
    cursor: default;
    transition:
      background-color var(--motion-instant) var(--ease-out),
      outline-color var(--motion-fast) var(--ease-out);
  }

  .flow:focus-visible {
    outline: 2px solid var(--color-ring);
    outline-offset: 2px;
  }

  .flow:hover:not(:disabled) {
    background: var(--color-fill-hover);
  }

  .flow[aria-pressed="true"] {
    outline-color: var(--hue-ink);
  }

  .art {
    display: block;
    aspect-ratio: 2 / 1;
    padding: 6px 8px 2px;
    background: var(--hue-wash);
  }

  .words {
    display: grid;
    gap: 4px;
    padding: 12px 14px 14px;
  }

  .title {
    color: var(--color-text);
    font-size: var(--text-body);
    font-weight: 600;
    line-height: 1.3;
  }

  .line {
    min-block-size: calc(2 * 1.4em);
    color: var(--color-muted);
    font-size: var(--text-label);
    line-height: 1.4;
    text-wrap: pretty;
  }

  .reads {
    margin-block-start: 4px;
    color: var(--color-faint);
    font-size: var(--text-caption);
  }

  footer {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 8px;
    min-block-size: var(--control-compact);
  }

  .ask {
    margin: 0;
    color: var(--color-muted);
    font-size: var(--text-body);
  }

  .hue-sky {
    --hue: var(--color-soft-sky);
    --hue-wash: var(--color-soft-sky-wash);
    --hue-edge: var(--color-soft-sky-edge);
    --hue-ink: var(--color-soft-sky-ink);
  }

  .hue-mint {
    --hue: var(--color-soft-mint);
    --hue-wash: var(--color-soft-mint-wash);
    --hue-edge: var(--color-soft-mint-edge);
    --hue-ink: var(--color-soft-mint-ink);
  }

  .hue-lemon {
    --hue: var(--color-soft-lemon);
    --hue-wash: var(--color-soft-lemon-wash);
    --hue-edge: var(--color-soft-lemon-edge);
    --hue-ink: var(--color-soft-lemon-ink);
  }

  .hue-peach {
    --hue: var(--color-soft-peach);
    --hue-wash: var(--color-soft-peach-wash);
    --hue-edge: var(--color-soft-peach-edge);
    --hue-ink: var(--color-soft-peach-ink);
  }

  .hue-rose {
    --hue: var(--color-soft-rose);
    --hue-wash: var(--color-soft-rose-wash);
    --hue-edge: var(--color-soft-rose-edge);
    --hue-ink: var(--color-soft-rose-ink);
  }

  .hue-lilac {
    --hue: var(--color-soft-lilac);
    --hue-wash: var(--color-soft-lilac-wash);
    --hue-edge: var(--color-soft-lilac-edge);
    --hue-ink: var(--color-soft-lilac-ink);
  }

  @keyframes arrive {
    from {
      opacity: 0;
      transform: translateY(8px);
    }
  }

  @keyframes settle {
    from {
      opacity: 0;
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .panel,
    .flows {
      animation: none;
    }
  }
</style>
