<script lang="ts">
  import Button from "$shared/ui/Button";
  import * as m from "$shared/i18n/messages";
  import HostGlyph from "../cards/HostGlyph.svelte";
  import AskReceipt from "./AskReceipt.svelte";
  import AskShell from "./AskShell.svelte";
  import { registrableSite, siteName, type SignInAsk } from "./asks";

  /** A page the agent can't go past until the person signs in; it carries on by itself after. */
  let {
    ask,
    placement = "canvas",
    busy = false,
    onopen,
    onsignedin,
  }: {
    ask: SignInAsk;
    placement?: "canvas" | "island";
    busy?: boolean;
    onopen?: () => void;
    onsignedin?: () => void;
  } = $props();
</script>

{#if ask.state === "open"}
  <AskShell
    {placement}
    {busy}
    label={m.work_ask_sign_in_title({ host: siteName(ask.host) })}
    where={registrableSite(ask.host)}
    title={m.work_ask_sign_in_title({ host: siteName(ask.host) })}
  >
    {#snippet mark()}<HostGlyph host={ask.host} size={18} initial={false} />{/snippet}
    <p class="how">{m.work_ask_sign_in_how()}</p>
    {#snippet actions()}
      {#if onsignedin && ask.page.can_continue}<Button
          variant={onopen ? "secondary" : "primary"}
          pending={busy}
          onclick={() => onsignedin?.()}>{m.work_ask_signed_in()}</Button
        >{/if}
      {#if onopen}<Button variant="primary" disabled={busy} onclick={() => onopen?.()}
          >{m.work_ask_sign_in()}</Button
        >{/if}
    {/snippet}
  </AskShell>
{:else}
  <AskReceipt tone="working" status={m.work_ask_signed_in_short()} text={m.work_ask_continuing()} />
{/if}

<style>
  .how {
    margin: 0;
    color: var(--color-muted);
    font-size: var(--text-body);
    line-height: 18px;
  }
</style>
