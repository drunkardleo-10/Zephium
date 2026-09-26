<script lang="ts">
  import type { WorkSession } from "$domain/work";
  import Button from "$shared/ui/Button";
  import HostGlyph from "./cards/HostGlyph.svelte";
  import AccountBadge from "./cards/AccountBadge.svelte";
  import * as m from "$shared/i18n/messages";
  let { session }: { session: WorkSession } = $props();
  const grant = $derived(session.grantDraft);
  const host = $derived.by(() => {
    if (!grant) return "";
    try {
      return new URL(grant.origin).host;
    } catch {
      return grant.origin;
    }
  });
  const work = $derived(session.projection?.work);
  const blocked = $derived(
    !work ||
      !!session.pending ||
      session.operations.busy(work.id) ||
      !["ready", "rejected"].includes(session.delivery),
  );
</script>

<!-- The approval is the attestation: the account is the one this tab is signed in as, never a name we detect.
     It stands in the agent line's place while the question is open. -->
{#if grant}
  <section class="grant" aria-label={m.work_account_read_origin({ host })}>
    <div class="line">
      <span class="mark"
        ><HostGlyph {host} url={grant.origin} size={18} initial={false} /><AccountBadge
          {host}
          size={12}
        /></span
      >
      <span class="words">
        <strong>{m.work_account_grant_question({ host })}</strong>
        <span class="caption">{m.work_account_grant_pages({ pages: grant.pages })}</span>
      </span>
      <span class="actions">
        <Button size="compact" disabled={blocked} onclick={() => void session.declineGrant()}
          >{m.work_account_grant_decline()}</Button
        >
        <Button
          size="compact"
          variant="primary"
          disabled={blocked}
          onclick={() => void session.allowGrant()}>{m.work_account_grant_allow()}</Button
        >
      </span>
    </div>
  </section>
{:else if session.grantDeclined}
  <section class="grant" aria-live="polite">
    <p class="line declined">{m.work_account_grant_declined()}</p>
  </section>
{/if}

<style>
  .grant {
    box-sizing: border-box;
    inline-size: 100%;
    border-radius: var(--radius-panel);
    background: var(--color-float);
    box-shadow: var(--shadow-popover);
  }

  .line {
    display: flex;
    align-items: center;
    gap: 10px;
    box-sizing: border-box;
    min-block-size: 36px;
    margin: 0;
    padding: 6px 6px 6px 9px;
  }

  .declined {
    padding-inline-start: 14px;
    color: var(--color-muted);
    font-size: var(--text-label);
  }

  .mark {
    position: relative;
    display: inline-grid;
    flex: none;
    place-items: center;
    inline-size: 24px;
    block-size: 24px;
  }

  .mark :global(.account-badge) {
    position: absolute;
    inset-block-end: -2px;
    inset-inline-end: -2px;
    box-shadow: 0 0 0 2px var(--color-float);
  }

  .words {
    display: flex;
    flex: 1;
    flex-direction: column;
    min-inline-size: 0;
  }

  .words strong,
  .caption {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .words strong {
    font-size: var(--text-label);
    font-weight: 500;
  }

  .caption {
    color: var(--color-muted);
    font-size: var(--text-caption);
  }

  .actions {
    display: flex;
    flex: none;
    gap: 6px;
  }
</style>
