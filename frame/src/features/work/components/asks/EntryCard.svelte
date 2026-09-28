<script lang="ts">
  import Button from "$shared/ui/Button";
  import * as m from "$shared/i18n/messages";
  import HostGlyph from "../cards/HostGlyph.svelte";
  import AskReceipt from "./AskReceipt.svelte";
  import AskShell from "./AskShell.svelte";
  import { ASK_WORDS, registrableSite, type EntryAsk } from "./asks";

  /** The one question before the agent works in the person's session on a site. */
  let {
    ask,
    placement = "canvas",
    busy = false,
    onanswer,
  }: {
    ask: EntryAsk;
    placement?: "canvas" | "island";
    busy?: boolean;
    /** Rust's own option words go back as they came. */
    onanswer: (answer: string) => void;
  } = $props();

  const answered = $derived(
    ask.answer === ASK_WORDS.allow
      ? m.work_ask_allowed()
      : ask.answer === ask.always
        ? m.work_ask_always_short()
        : m.work_ask_not_now(),
  );
</script>

{#if ask.state === "open"}
  <AskShell
    {placement}
    {busy}
    label={m.work_ask_entry_title({ name: ask.name })}
    note={m.work_ask_entry_note()}
    where={ask.host ? registrableSite(ask.host) : ask.name}
    title={m.work_ask_entry_title({ name: ask.name })}
  >
    {#snippet mark()}<HostGlyph host={ask.host ?? ""} size={18} initial={false} />{/snippet}
    {#if ask.plan}
      <p class="plan"><span class="label">{m.work_ask_plan()}</span>{ask.plan}</p>
    {/if}
    {#snippet actions()}
      <Button variant="ghost" disabled={busy} onclick={() => onanswer(ASK_WORDS.notNow)}
        >{m.work_ask_not_now()}</Button
      >
      <Button disabled={busy} onclick={() => onanswer(ask.always)}>{m.work_ask_always()}</Button>
      <Button variant="primary" pending={busy} onclick={() => onanswer(ASK_WORDS.allow)}
        >{m.work_ask_allow()}</Button
      >
    {/snippet}
  </AskShell>
{:else if ask.answer}
  <AskReceipt
    tone={ask.answer === ASK_WORDS.notNow ? "declined" : "done"}
    status={answered}
    text={m.work_ask_entry_receipt({ name: ask.name })}
  />
{/if}

<style>
  .plan {
    margin: 0;
    color: var(--color-muted);
    font-size: var(--text-body);
    line-height: 18px;
    overflow-wrap: anywhere;
  }

  .label {
    margin-inline-end: 6px;
    color: var(--color-faint);
    font-size: var(--text-label);
  }
</style>
