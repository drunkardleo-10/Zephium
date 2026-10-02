<script lang="ts">
  import { onMount } from "svelte";
  import { Alert02Icon, Tick02Icon } from "@hugeicons/core-free-icons";
  import * as m from "$shared/i18n/messages";
  import {
    browserImport,
    type ImportJob,
    type ImportKind,
    type ImportProblem,
    type ImportSource,
  } from "$domain/browser-import";
  import SettingsRow from "$shared/ui/SettingsRow";
  import Select from "$shared/ui/Select";
  import Checkbox from "$shared/ui/Checkbox";
  import Button from "$shared/ui/Button";
  import Icon from "$shared/ui/Icon";

  const ALL_KINDS: readonly ImportKind[] = ["essentials", "bookmarks", "history"];

  let found = $derived(browserImport.found());
  let job = $derived(browserImport.current());
  let chosen = $state<string | null>(null);
  let profile = $state<string | null>(null);
  let kinds = $state<ImportKind[]>([...ALL_KINDS]);

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
  const number = new Intl.NumberFormat();

  onMount(() => {
    void browserImport.detect();
  });

  function toggle(kind: ImportKind, on: boolean) {
    kinds = on ? [...kinds, kind] : kinds.filter((candidate) => candidate !== kind);
  }

  function kindName(kind: ImportKind) {
    if (kind === "essentials") return m.import_essentials();
    return kind === "bookmarks" ? m.onb_import_bookmarks() : m.onb_import_history();
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

  function progressText(entry: ImportJob["kinds"][number]) {
    switch (entry.state) {
      case "queued":
        return m.import_kind_waiting();
      case "running":
        return m.import_kind_reading();
      case "skipped":
        return m.import_kind_skipped();
      case "failed":
        return problemText(entry.problem, jobName);
      case "done":
        return entry.done > 0
          ? m.import_kind_added({ count: number.format(entry.done) })
          : m.import_kind_current();
    }
  }

  let status = $derived.by(() => {
    if (found === null) return m.import_looking();
    if (found.length === 0) return m.onb_import_none();
    if (source?.needsPermission) return m.onb_import_permission({ browser: source.name });
    return m.pref_general_import_help();
  });
</script>

<!-- Access granted in System Settings tells nobody; look again on return. -->
<svelte:window
  onfocus={() => {
    if (!running && found?.some((candidate) => candidate.needsPermission))
      void browserImport.detect();
  }}
/>

{#if job}
  <div class="job" data-setting="general.import" aria-live="polite">
    <div class="job-head">
      <strong
        >{job.finished
          ? job.cancelled
            ? m.import_stopped()
            : m.import_imported({ browser: jobName })
          : m.import_importing({ browser: jobName })}</strong
      >
      {#if job.finished}
        <Button size="compact" onclick={browserImport.dismiss}>{m.import_dismiss()}</Button>
      {:else}
        <Button size="compact" variant="ghost" onclick={() => void browserImport.cancel()}
          >{m.onb_import_stop()}</Button
        >
      {/if}
    </div>
    <ul>
      {#each job.kinds as entry (entry.kind)}
        <li data-state={entry.state}>
          <span class="state" aria-hidden="true"
            >{#if entry.state === "done"}<Icon
                icon={Tick02Icon}
                size={14}
              />{:else if entry.state === "failed"}<Icon icon={Alert02Icon} size={14} />{:else}<i
              ></i>{/if}</span
          >
          <span class="kind">{kindName(entry.kind)}</span>
          <span class="detail">{progressText(entry)}</span>
        </li>
      {/each}
    </ul>
  </div>
{:else}
  <SettingsRow settingId="general.import" title={m.pref_general_import()} description={status}>
    <div class="import">
      {#if source}
        <Select
          label={m.preview_import_source()}
          labelHidden
          value={source.id}
          options={(found ?? []).map((candidate) => ({
            value: candidate.id,
            label: candidate.name,
          }))}
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
          <Button
            size="compact"
            onclick={() => void browserImport.openPermissionSettings(source.id)}
            >{m.onb_import_allow()}</Button
          >
        {:else}
          <Button
            size="compact"
            variant="primary"
            disabled={running || offered.length === 0 || profileId === null}
            onclick={() => profileId && void browserImport.start(source.id, profileId, offered)}
            >{m.onb_import_action()}</Button
          >
        {/if}
      {/if}
    </div>
  </SettingsRow>
  {#if source && !source.needsPermission}
    <SettingsRow title={m.import_what()}>
      <div class="kinds">
        {#each ALL_KINDS.filter((kind) => source.kinds.includes(kind)) as kind (kind)}
          <Checkbox
            label={kindName(kind)}
            checked={kinds.includes(kind)}
            onchange={(on) => toggle(kind, on)}
          />
        {/each}
      </div>
    </SettingsRow>
  {/if}
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

  .job {
    display: grid;
    gap: 10px;
    padding: 16px 18px;
  }

  .job-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    min-height: 28px;
  }

  .job-head strong {
    font-size: var(--text-body);
    font-weight: 550;
  }

  ul {
    display: grid;
    gap: 2px;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  li {
    display: grid;
    grid-template-columns: 18px minmax(0, 7rem) minmax(0, 1fr);
    align-items: center;
    gap: 8px;
    min-height: 26px;
    font-size: var(--text-label);
  }

  .state {
    display: grid;
    place-items: center;
    color: var(--color-muted);
  }

  .state i {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--color-faint);
  }

  .kind {
    color: var(--color-text);
  }

  .detail {
    color: var(--color-muted);
    overflow-wrap: anywhere;
  }

  li[data-state="running"] .state i {
    background: var(--color-accent);
    animation: pulse 1.1s var(--ease-smooth) infinite alternate;
  }

  li[data-state="done"] .state {
    color: var(--color-success);
  }

  li[data-state="failed"] .state,
  li[data-state="failed"] .detail {
    color: var(--color-danger);
  }

  @keyframes pulse {
    to {
      opacity: 0.35;
      scale: 0.7;
    }
  }

  @media (prefers-reduced-motion: reduce) {
    li[data-state="running"] .state i {
      animation: none;
    }
  }
</style>
