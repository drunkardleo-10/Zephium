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

<!-- The approval is the attestation: the account is the one this tab is signed in as, never a name we detect. -->
{#if grant}
  <section class="grant" aria-label={m.work_account_read_origin({ host })}>
    <div class="who">
      <span class="mark"
        ><HostGlyph {host} url={grant.origin} size={20} initial={false} /><AccountBadge
          {host}
          size={14}
        /></span
      >
      <span class="titles">
        <strong>{host}</strong>
        <span>{m.work_account_grant_as()}</span>
      </span>
    </div>
    <p class="scope">{m.work_account_grant_pages({ pages: grant.pages })}</p>
    <p class="note">{m.work_account_grant_note()}</p>
    <div class="actions">
      <Button size="compact" onclick={() => session.declineGrant()}
        >{m.work_account_grant_decline()}</Button
      >
      <Button
        size="compact"
        variant="primary"
        disabled={blocked}
        onclick={() => void session.allowGrant()}>{m.work_account_grant_allow()}</Button
      >
    </div>
  </section>
{/if}

<style>
  .grant {
    display: flex;
    flex-direction: column;
    gap: 8px;
    box-sizing: border-box;
    inline-size: 100%;
    padding: 12px;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-control);
    background: var(--color-surface);
    font-size: var(--text-caption);
  }

  .who {
    display: flex;
    align-items: center;
    gap: 8px;
    min-inline-size: 0;
  }

  .mark {
    position: relative;
    display: inline-grid;
    flex: none;
    place-items: center;
    inline-size: 28px;
    block-size: 28px;
  }

  .mark :global(.account-badge) {
    position: absolute;
    inset-block-end: -2px;
    inset-inline-end: -2px;
    box-shadow: 0 0 0 2px var(--color-surface);
  }

  .titles {
    display: flex;
    flex-direction: column;
    min-inline-size: 0;
  }

  .titles strong,
  .titles span {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .titles strong {
    font-size: var(--text-body);
    font-weight: 600;
  }

  .titles span,
  .note {
    color: var(--color-muted);
  }

  p {
    margin: 0;
  }

  .scope {
    color: var(--color-text);
    font-weight: 500;
  }

  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
  }
</style>
