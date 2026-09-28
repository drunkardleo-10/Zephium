<script lang="ts">
  import Icon from "$shared/ui/Icon";
  import * as m from "$shared/i18n/messages";
  import AskReceipt from "./AskReceipt.svelte";
  import AskShell from "./AskShell.svelte";
  import type { QuestionAsk } from "./asks";
  import { ArrowUp02Icon } from "./icons";
  import AgentOrb from "../cards/AgentOrb.svelte";

  /** The agent's own question: its options answer in one press, or the person says it in words. */
  let {
    ask,
    placement = "canvas",
    busy = false,
    onanswer,
    seed = 0,
  }: {
    ask: QuestionAsk;
    /** The asking agent's orb. */
    seed?: number;
    placement?: "canvas" | "island";
    busy?: boolean;
    onanswer: (answer: string) => void;
  } = $props();
  const id = $props.id();
  let text = $state("");
</script>

{#if ask.state === "open"}
  <AskShell
    {placement}
    {busy}
    fill
    label={ask.prompt}
    where={m.work_ask_question()}
    title={ask.prompt}
  >
    {#snippet mark()}<AgentOrb {seed} size={18} />{/snippet}
    {#if ask.options.length}
      <div class="options">
        {#each ask.options as option, index (index)}<button
            type="button"
            class="option"
            disabled={busy}
            onclick={() => onanswer(option)}>{option}</button
          >{/each}
      </div>
    {/if}
    {#snippet actions()}
      <form
        class="words"
        onsubmit={(event) => {
          event.preventDefault();
          const answer = text.trim();
          if (answer) onanswer(answer);
        }}
      >
        <label class="sr-only" for={`${id}-answer`}>{m.work_ask_answer_label()}</label>
        <input
          id={`${id}-answer`}
          maxlength="8192"
          placeholder={ask.options.length ? m.work_ask_or_say() : m.work_ask_say()}
          bind:value={text}
          disabled={busy}
        />
        <button
          type="submit"
          class="send"
          aria-label={m.work_line_send_answer()}
          disabled={busy || !text.trim()}
          ><Icon icon={ArrowUp02Icon} size={14} strokeWidth={2} /></button
        >
      </form>
    {/snippet}
  </AskShell>
{:else if ask.answer}
  <AskReceipt tone="done" status={ask.answer} text={ask.prompt} />
{/if}

<style>
  .options {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  /* Rows, not chips: an option can be a sentence and still read whole. */
  .option {
    box-sizing: border-box;
    inline-size: 100%;
    min-block-size: 34px;
    padding: 8px 12px;
    border: 0;
    border-radius: var(--radius-row);
    background: var(--color-fill);
    color: var(--color-text);
    font: inherit;
    font-size: var(--text-body);
    line-height: 18px;
    text-align: start;
    overflow-wrap: anywhere;
    cursor: default;
    transition: background-color var(--motion-fast) var(--ease-out);
  }

  .option:focus-visible {
    outline: 2px solid var(--color-ring);
    outline-offset: -2px;
  }

  .option:disabled {
    color: var(--color-faint);
  }

  .option:hover:not(:disabled) {
    background: var(--color-fill-hover);
  }

  .option:active:not(:disabled) {
    background: var(--color-fill-pressed);
  }

  .words {
    display: flex;
    align-items: center;
    gap: 6px;
    inline-size: 100%;
    margin: 0;
  }

  input {
    flex: 1;
    min-inline-size: 0;
    box-sizing: border-box;
    block-size: 30px;
    padding: 0 10px;
    border: 0;
    border-radius: var(--radius-control);
    background: var(--color-field);
    color: var(--color-text);
    font: inherit;
    font-size: var(--text-body);
    outline: none;
  }

  input::placeholder {
    color: var(--color-faint);
  }

  input:focus-visible {
    box-shadow: var(--shadow-field-focus);
  }

  .send {
    display: grid;
    flex: none;
    place-items: center;
    inline-size: 30px;
    block-size: 30px;
    border: 0;
    border-radius: var(--radius-capsule);
    background: var(--color-lit);
    color: var(--color-on-lit);
    cursor: default;
  }

  .send:disabled {
    background: var(--color-control);
    color: var(--color-faint);
  }

  .send:focus-visible {
    outline: 2px solid var(--color-ring);
    outline-offset: 2px;
  }
</style>
