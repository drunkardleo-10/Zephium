<script lang="ts">
  import * as m from "$shared/i18n/messages";
  import type { WorkModelEntry, WorkModelRole } from "$shared/ipc/bindings";
  import { tabs } from "$domain/tabs";
  import {
    ModelsSession,
    entryOf,
    keyedProviders,
    providerMark,
    providerName,
    usable,
  } from "$domain/ai";
  import Button from "$shared/ui/Button";
  import Icon from "$shared/ui/Icon";
  import Select from "$shared/ui/Select";
  import SettingsGroup from "$shared/ui/SettingsGroup";
  import SettingsRow from "$shared/ui/SettingsRow";
  import * as settings from "../../lib/settings-state.svelte";
  import AiProviderRow from "../AiProviderRow.svelte";

  let profile = $derived(tabs.profile());
  let session = $state.raw<ModelsSession | null>(null);
  $effect(() => {
    const id = profile && profile.kind !== "incognito" ? profile.id : null;
    if (!id) return;
    const owner = new ModelsSession(id);
    session = owner;
    void owner.start();
    return () => {
      owner.dispose();
      if (session === owner) session = null;
    };
  });

  const models = $derived(session?.models ?? null);
  const AUTO = "auto";
  const roles: {
    role: Exclude<WorkModelRole, "decision">;
    title: () => string;
    description: () => string;
  }[] = [
    { role: "lead", title: m.ai_role_lead, description: m.ai_role_lead_desc },
    { role: "page", title: m.ai_role_page, description: m.ai_role_page_desc },
    { role: "light", title: m.ai_role_light, description: m.ai_role_light_desc },
  ];
  const anyUsable = $derived(
    !!models &&
      (models.cloud.signed_in || keyedProviders.some((provider) => usable(models, provider))),
  );

  function label(entry: WorkModelEntry) {
    const provider = entry.model.provider;
    return provider === "anthropic" || provider === "deep_seek" || provider === "cloud"
      ? entry.display_name
      : `${entry.display_name} · ${providerName(provider)}`;
  }

  function options(role: Exclude<WorkModelRole, "decision">) {
    const effective = entryOf(models, models?.effective[role]);
    const automatic = {
      value: AUTO,
      label: effective ? m.ai_role_auto({ model: effective.display_name }) : m.ai_role_auto_none(),
    };
    const chosen = entryOf(models, models?.chosen[role]);
    const offered = (models?.entries ?? []).filter(
      (entry) =>
        entry.roles.includes(role) &&
        usable(models, entry.model.provider) &&
        (entry.recommended || entry.id === chosen?.id),
    );
    return [automatic, ...offered.map((entry) => ({ value: entry.id, label: label(entry) }))];
  }
</script>

{#if !profile || profile.kind === "incognito"}<p class="ai-note">{m.work_regular_profile()}</p>
{:else if session?.unavailable && !models}<p class="ai-note" role="alert">{m.ai_unavailable()}</p>
{:else}
  <SettingsGroup title={m.ai_cloud_group()}>
    <div class="cloud">
      <span class="tile" aria-hidden="true"
        ><Icon icon={providerMark("cloud")} size={17} strokeWidth={1.5} /></span
      >
      <div class="copy">
        <h3>{m.ai_cloud_title()}</h3>
        <p>
          {#if models?.cloud.signed_in}{models.cloud.plan
              ? m.ai_cloud_signed_in_plan({ plan: models.cloud.plan })
              : m.ai_cloud_signed_in()}{:else}{m.ai_cloud_signed_out()}{/if}
        </p>
      </div>
      <Button size="compact" onclick={() => settings.select("account")}
        >{m.ai_cloud_account()}</Button
      >
    </div>
  </SettingsGroup>

  <SettingsGroup title={m.ai_keys_group()} description={m.ai_keys_note()}>
    {#if session}{#each keyedProviders as provider (provider)}<AiProviderRow
          {session}
          {provider}
        />{/each}{/if}
  </SettingsGroup>

  <SettingsGroup
    title={m.ai_roles_group()}
    description={anyUsable ? m.ai_roles_note() : m.ai_roles_none()}
  >
    {#each roles as row (row.role)}
      <SettingsRow title={row.title()} description={row.description()}
        ><Select
          label={row.title()}
          labelHidden
          options={options(row.role)}
          value={models?.chosen[row.role] ?? AUTO}
          disabled={!anyUsable || !!session?.busy}
          onchange={(value) => void session?.choose(row.role, value === AUTO ? null : value)}
        /></SettingsRow
      >
    {/each}
  </SettingsGroup>
{/if}

<style>
  .ai-note {
    margin: 0 16px;
    font-size: var(--text-label);
    line-height: 1.5;
    color: var(--color-muted);
  }

  .cloud {
    display: flex;
    align-items: center;
    gap: 12px;
    box-sizing: border-box;
    min-height: var(--row-page);
    padding: 12px 18px 12px 14px;
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

  p {
    margin: 2px 0 0;
    color: var(--color-muted);
    font-size: var(--text-label);
    line-height: 1.5;
  }
</style>
