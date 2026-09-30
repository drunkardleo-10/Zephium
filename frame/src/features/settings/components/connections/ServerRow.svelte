<script lang="ts">
  import type { WorkServerDraftV1, WorkServerRowV1 } from "$shared/ipc/bindings";
  import { type ConnectionsSession, serverKey, serviceMark } from "$domain/connections";
  import Button from "$shared/ui/Button";
  import Icon from "$shared/ui/Icon";
  import Menu from "$shared/ui/Menu";
  import MoreHorizontalIcon from "@hugeicons/core-free-icons/MoreHorizontalIcon";
  import * as m from "$shared/i18n/messages";
  import { serverAddress } from "../../lib/connections";
  import ServerEditor from "./ServerEditor.svelte";

  let {
    session,
    row,
    taken,
  }: { session: ConnectionsSession; row: WorkServerRowV1; taken: readonly string[] } = $props();

  const server = $derived(row.server);
  const id = $derived(server.id);
  const check = $derived(session.checks[id]);
  const checking = $derived(session.busy === `check:${id}`);
  const signing = $derived(session.busy === `sign-in:${id}`);
  let editing = $state(false);

  const oauth = $derived(server.transport.kind === "http" && server.transport.auth === "oauth");
  const needsSignIn = $derived(
    check?.outcome === "sign_in" ||
      check?.outcome === "cancelled" ||
      (oauth && !row.signed_in && !check),
  );

  const status = $derived.by(() => {
    if (!server.enabled) return { tone: "quiet", text: m.connections_off() };
    if (signing) return { tone: "quiet", text: m.connections_signing_in() };
    if (checking) return { tone: "quiet", text: m.connections_checking() };
    if (check?.outcome === "cancelled")
      return { tone: "attention", text: m.connections_sign_in_cancelled() };
    if (needsSignIn) return { tone: "attention", text: m.connections_sign_in_needed() };
    if (!check) return null;
    switch (check.outcome) {
      case "ready": {
        const asking = check.tools.filter((tool) => tool.asks).length;
        const tools =
          check.tools.length === 1
            ? m.connections_tools_one()
            : m.connections_tools({ count: String(check.tools.length) });
        return {
          tone: "good",
          text: asking
            ? `${tools} · ${m.connections_tools_asking({ count: String(asking) })}`
            : tools,
        };
      }
      case "not_found":
        return {
          tone: "bad",
          text:
            server.transport.kind === "stdio"
              ? m.connections_not_found({ command: server.transport.command })
              : m.connections_failed(),
        };
      case "timeout":
        return { tone: "bad", text: m.connections_timeout() };
      default:
        return { tone: "bad", text: m.connections_failed() };
    }
  });

  async function save(draft: WorkServerDraftV1) {
    if (await session.save(draft)) editing = false;
  }
</script>

<div class="server-row" data-off={!server.enabled}>
  <div class="head">
    <span class="tile" aria-hidden="true"
      ><Icon icon={serviceMark(serverKey(server))} size={17} strokeWidth={1.5} /></span
    >
    <div class="copy">
      <h3>{server.name}</h3>
      <p>
        {#if status}<span class="status" data-tone={status.tone}
            ><span class="dot" aria-hidden="true"></span>{status.text}</span
          >{:else}<span class="address">{serverAddress(server)}</span>{/if}
      </p>
    </div>
    {#if !editing}
      <div class="actions">
        {#if signing}<Button size="compact" onclick={() => void session.cancelSignIn(id)}
            >{m.connections_cancel()}</Button
          >{/if}
        {#if needsSignIn && server.enabled}<Button
            size="compact"
            pending={signing}
            disabled={!!session.busy && !signing}
            onclick={() => void session.signIn(id)}>{m.connections_sign_in()}</Button
          >{:else if server.enabled}<Button
            size="compact"
            variant="ghost"
            pending={checking}
            disabled={!!session.busy && !checking}
            onclick={() => void session.check(id)}
            >{check && check.outcome !== "ready"
              ? m.connections_retry()
              : m.connections_reconnect()}</Button
          >{/if}
        <Menu
          label={m.connections_options({ name: server.name })}
          triggerClass="server-row-more"
          align="end"
          entries={[
            { kind: "item", id: "edit", label: m.connections_edit() },
            {
              kind: "item",
              id: "enable",
              label: server.enabled ? m.connections_turn_off() : m.connections_turn_on(),
            },
            { kind: "separator" },
            { kind: "item", id: "remove", label: m.connections_remove(), danger: true },
          ]}
          onselect={(choice) => {
            if (choice === "edit") editing = true;
            else if (choice === "enable") void session.enable(row, !server.enabled);
            else void session.remove(id);
          }}
        >
          {#snippet trigger()}<Icon icon={MoreHorizontalIcon} size={16} />{/snippet}
        </Menu>
      </div>
    {/if}
  </div>
  {#if editing}
    <ServerEditor
      editing={row}
      {taken}
      saving={session.busy === "save"}
      onpreview={(draft) => session.preview(draft)}
      onsave={(draft) => void save(draft)}
      oncancel={() => {
        void session.cancelPreview();
        editing = false;
      }}
    />
  {/if}
</div>

<style>
  .server-row {
    position: relative;
    box-sizing: border-box;
    padding: 12px 18px 12px 14px;
  }

  :global(.server-row) + .server-row::before,
  :global(.server-row) + :global(.server-add)::before {
    content: "";
    position: absolute;
    inset-inline: 58px 0;
    top: 0;
    height: 1px;
    background: var(--color-border);
  }

  .head {
    display: flex;
    align-items: center;
    gap: 12px;
    min-height: calc(var(--row-page) - 24px);
  }

  .tile {
    display: grid;
    flex: none;
    inline-size: 32px;
    block-size: 32px;
    border-radius: var(--radius-control);
    background: var(--color-fill);
    color: var(--color-text);
    place-items: center;
  }

  .copy {
    flex: 1;
    min-width: 0;
  }

  h3 {
    margin: 0;
    color: var(--color-text);
    font-size: var(--text-page-title);
    font-weight: 500;
    line-height: 19px;
    letter-spacing: -0.008em;
  }

  [data-off="true"] .tile,
  [data-off="true"] h3 {
    color: var(--color-muted);
  }

  p {
    margin: 2px 0 0;
    overflow: hidden;
    color: var(--color-muted);
    font-size: var(--text-label);
    line-height: 1.5;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .address {
    font-family: var(--font-mono);
    font-size: var(--text-caption);
  }

  .status {
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }

  .dot {
    flex: none;
    inline-size: 6px;
    block-size: 6px;
    border-radius: var(--radius-capsule);
    background: var(--color-faint);
  }

  .status[data-tone="good"] .dot {
    background: var(--color-success);
  }

  .status[data-tone="attention"] .dot {
    background: var(--color-warning);
  }

  .status[data-tone="bad"] {
    color: var(--color-danger);
  }

  .status[data-tone="bad"] .dot {
    background: var(--color-danger);
  }

  .actions {
    display: flex;
    flex: none;
    align-items: center;
    gap: 4px;
  }

  :global(.server-row-more) {
    display: grid;
    inline-size: 28px;
    block-size: 28px;
    padding: 0;
    border: 0;
    border-radius: var(--radius-control-compact);
    background: transparent;
    color: var(--color-muted);
    place-items: center;
    transition: background-color var(--motion-instant) var(--ease-smooth);
  }

  :global(.server-row-more:hover),
  :global(.server-row-more[data-state="open"]) {
    background: var(--color-fill-hover);
    color: var(--color-text);
  }

  :global(.server-row-more:focus-visible) {
    outline: 2px solid var(--color-ring);
    outline-offset: 1px;
  }
</style>
