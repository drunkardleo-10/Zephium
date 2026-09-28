<script lang="ts">
  import Button from "$shared/ui/Button";
  import Icon from "$shared/ui/Icon";
  import * as m from "$shared/i18n/messages";
  import HostGlyph from "../cards/HostGlyph.svelte";
  import AskReceipt from "./AskReceipt.svelte";
  import AskShell from "./AskShell.svelte";
  import type { ConnectionAsk } from "./asks";
  import { ComputerTerminal01Icon } from "./icons";

  /** A service the agent can reach directly, or through its website as the fallback. */
  let {
    ask,
    placement = "canvas",
    busy = false,
    onanswer,
  }: {
    ask: ConnectionAsk;
    placement?: "canvas" | "island";
    busy?: boolean;
    onanswer: (answer: string) => void;
  } = $props();

  const title = $derived(
    ask.tool
      ? m.work_ask_connection_tool({ service: ask.service, tool: ask.tool })
      : m.work_ask_connection({ service: ask.service }),
  );
</script>

{#if ask.state === "open"}
  <AskShell {placement} {busy} label={title} where={ask.service} {title}>
    {#snippet mark()}{#if ask.host}<HostGlyph
          host={ask.host}
          size={18}
          initial={false}
        />{:else}<Icon icon={ComputerTerminal01Icon} size={16} />{/if}{/snippet}
    <p class="how">
      {ask.tool
        ? m.work_ask_connection_tool_how({ tool: ask.tool })
        : m.work_ask_connection_how({ service: ask.service })}
    </p>
    {#snippet actions()}
      <Button variant="ghost" disabled={busy} onclick={() => onanswer(ask.web)}
        >{m.work_ask_use_website()}</Button
      >
      <Button variant="primary" pending={busy} onclick={() => onanswer(ask.use)}>{ask.use}</Button>
    {/snippet}
  </AskShell>
{:else if ask.answer}
  <AskReceipt
    tone={ask.answer === ask.use ? "done" : "declined"}
    status={ask.answer === ask.use
      ? m.work_ask_using({ service: ask.service })
      : m.work_ask_using_website()}
    text={ask.service}
  />
{/if}

<style>
  .how {
    margin: 0;
    color: var(--color-muted);
    font-size: var(--text-body);
    line-height: 18px;
  }
</style>
