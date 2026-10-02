<script lang="ts">
  import { onMount } from "svelte";
  import * as m from "$shared/i18n/messages";
  import {
    browserImport,
    type ImportKind,
    type ImportProblem,
    type ImportSource,
  } from "$domain/browser-import";
  import SettingsRow from "$shared/ui/SettingsRow";
  import Select from "$shared/ui/Select";
  import Checkbox from "$shared/ui/Checkbox";
  import Button from "$shared/ui/Button";

  let found = $derived(browserImport.found());
  let job = $derived(browserImport.current());
  let chosen = $state<string | null>(null);
  let profile = $state<string | null>(null);
  let kinds = $state<ImportKind[]>(["bookmarks", "history"]);

  let source = $derived<ImportSource | null>(
    found?.find((candidate) => candidate.id === chosen) ?? found?.[0] ?? null,
  );
  let profileId = $derived(
    source?.profiles.find((candidate) => candidate.id === profile)?.id ??
      source?.profiles[0]?.id ??
      null,
  );
  let offered = $derived(kinds.filter((kind) => source?.kinds.includes(kind)));
  let running = $derived(browserImport.busy());
  let jobName = $derived(found?.find((candidate) => candidate.id === job?.source)?.name ?? "");

  onMount(() => {
    void browserImport.detect();
  });

  function toggle(kind: ImportKind, on: boolean) {
    kinds = on ? [...kinds, kind] : kinds.filter((candidate) => candidate !== kind);
  }

  function problemText(problem: ImportProblem | null | undefined, browser: string) {
    switch (problem) {
      case "busy":
        return m.onb_import_quit({ browser });
      case "permission":
        return m.onb_import_permission({ browser });
      case "storage":
        return m.onb_import_storage();
      default:
        return m.onb_import_failed({ browser });
    }
  }

  let status = $derived.by(() => {
    if (!job) {
      if (found === null) return m.import_looking();
      if (found.length === 0) return m.onb_import_none();
      if (source?.needsPermission) return m.onb_import_permission({ browser: source.name });
      return m.pref_general_import_help();
    }
    const done = job.kinds.reduce((sum, entry) => sum + entry.done, 0);
    if (!job.finished) return m.onb_import_reading({ count: done });
    const failed = job.kinds.find((entry) => entry.state === "failed");
    if (failed) return problemText(failed.problem, jobName);
    if (job.cancelled) return m.import_stopped();
    return m.import_finished({ count: done, browser: jobName });
  });
</script>

<!-- Access granted in System Settings tells nobody; look again on return. -->
<svelte:window
  onfocus={() => {
    if (!running && found?.some((candidate) => candidate.needsPermission))
      void browserImport.detect();
  }}
/>

<SettingsRow settingId="general.import" title={m.pref_general_import()} description={status}>
  <div class="import">
    {#if running}
      <Button size="compact" variant="ghost" onclick={() => void browserImport.cancel()}
        >{m.onb_import_stop()}</Button
      >
    {:else if source}
      <Select
        label={m.preview_import_source()}
        labelHidden
        value={source.id}
        options={(found ?? []).map((candidate) => ({ value: candidate.id, label: candidate.name }))}
        onchange={(value) => {
          chosen = value;
          profile = null;
        }}
      />
      {#if source.profiles.length > 1 && profileId}
        <Select
          label={m.import_profile()}
          labelHidden
          value={profileId}
          options={source.profiles.map((candidate) => ({
            value: candidate.id,
            label: candidate.name,
          }))}
          onchange={(value) => (profile = value)}
        />
      {/if}
      {#if source.needsPermission}
        <Button size="compact" onclick={() => void browserImport.openPermissionSettings(source.id)}
          >{m.onb_import_allow()}</Button
        >
      {:else}
        <Button
          size="compact"
          variant="primary"
          disabled={offered.length === 0 || profileId === null}
          onclick={() => profileId && void browserImport.start(source.id, profileId, offered)}
          >{m.onb_import_action()}</Button
        >
      {/if}
    {/if}
  </div>
</SettingsRow>
{#if source && !running && !source.needsPermission}
  <SettingsRow title={m.import_what()}>
    <div class="kinds">
      {#each ["bookmarks", "history"] as const as kind (kind)}
        <Checkbox
          label={kind === "bookmarks" ? m.onb_import_bookmarks() : m.onb_import_history()}
          checked={kinds.includes(kind) && source.kinds.includes(kind)}
          disabled={!source.kinds.includes(kind)}
          onchange={(on) => toggle(kind, on)}
        />
      {/each}
    </div>
  </SettingsRow>
{/if}

<style>
  .import,
  .kinds {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: flex-end;
    gap: 8px;
  }

  .kinds {
    gap: 16px;
  }
</style>
