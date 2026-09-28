<script lang="ts">
  import Button from "$shared/ui/Button";
  import Icon from "$shared/ui/Icon";
  import * as m from "$shared/i18n/messages";
  import AskReceipt from "./AskReceipt.svelte";
  import AskShell from "./AskShell.svelte";
  import { ASK_WORDS, type ContextAsk, type ContextSource } from "./asks";
  import { BrowserIcon, Clock04Icon, Note01Icon } from "./icons";

  /** Whether the agent may look through something of the person's, asked once per work. */
  let {
    ask,
    placement = "canvas",
    busy = false,
    onanswer,
  }: {
    ask: ContextAsk;
    placement?: "canvas" | "island";
    busy?: boolean;
    onanswer: (answer: string) => void;
  } = $props();

  const ICONS = { history: Clock04Icon, notes: Note01Icon, tabs: BrowserIcon } as const;
  const TITLES: Record<ContextSource, () => string> = {
    history: m.work_ask_history_title,
    notes: m.work_ask_notes_title,
    tabs: m.work_ask_tabs_title,
  };
  const SEES: Record<ContextSource, () => string> = {
    history: m.work_ask_history_sees,
    notes: m.work_ask_notes_sees,
    tabs: m.work_ask_tabs_sees,
  };
  const NAMES: Record<ContextSource, () => string> = {
    history: m.work_ask_history_name,
    notes: m.work_ask_notes_name,
    tabs: m.work_ask_tabs_name,
  };
</script>

{#if ask.state === "open"}
  <AskShell
    {placement}
    {busy}
    label={TITLES[ask.source]()}
    where={m.work_ask_this_work()}
    title={TITLES[ask.source]()}
  >
    {#snippet mark()}<Icon icon={ICONS[ask.source]} size={16} />{/snippet}
    {#if ask.reason}<p class="reason">{ask.reason}</p>{/if}
    <p class="sees">{SEES[ask.source]()}</p>
    {#snippet actions()}
      <Button variant="ghost" disabled={busy} onclick={() => onanswer(ASK_WORDS.notNow)}
        >{m.work_ask_not_now()}</Button
      >
      <Button variant="primary" pending={busy} onclick={() => onanswer(ASK_WORDS.allow)}
        >{m.work_ask_allow()}</Button
      >
    {/snippet}
  </AskShell>
{:else if ask.answer}
  <AskReceipt
    tone={ask.answer === ASK_WORDS.allow ? "done" : "declined"}
    status={ask.answer === ASK_WORDS.allow ? m.work_ask_allowed() : m.work_ask_not_now()}
    text={NAMES[ask.source]()}
  />
{/if}

<style>
  .reason {
    margin: 0;
    color: var(--color-text);
    font-size: var(--text-body);
    line-height: 18px;
    overflow-wrap: anywhere;
  }

  .sees {
    margin: 0;
    color: var(--color-muted);
    font-size: var(--text-label);
    line-height: 16px;
  }
</style>
