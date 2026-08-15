<script lang="ts">
  import {
    Cancel01Icon,
    Delete02Icon,
    PuzzleIcon,
    Refresh01Icon,
  } from "@hugeicons/core-free-icons";
  import { untrack } from "svelte";
  import type {
    ExtensionInstallCandidateView,
    ExtensionManagementEntryView,
    ExtensionManagementLimitationView,
    ExtensionManagementRuntimeView,
  } from "../../../shared/ipc/bindings";
  import * as extensions from "../../../domain/extensions/extensions.svelte";
  import {
    apiPermissionLabel,
    compatibilityLimitationLabel,
    hostPermissionLabel,
  } from "../../../domain/extensions/permission-labels";
  import * as tabs from "../../../domain/tabs/tabs.svelte";
  import Icon from "../../../shared/ui/Icon.svelte";

  let { compact = false }: { compact?: boolean } = $props();

  let root = $state<HTMLDivElement>();
  let trigger = $state<HTMLButtonElement>();
  let panel = $state<HTMLDivElement>();
  let closeButton = $state<HTMLButtonElement>();
  let open = $state(false);
  let requestFailed = $state(false);
  let subscribedProfile = $state<string | null>(null);
  let confirming = $state<string | null>(null);
  let reviewingCandidate = $state<number | null>(null);
  let selectedOptionalApi = $state.raw<number[]>([]);
  let selectedOptionalHosts = $state.raw<number[]>([]);
  let showAllRequiredHosts = $state(false);
  let allowFileAccess = $state(false);
  let allowPrivateAccess = $state(false);

  let profileId = $derived(tabs.profile()?.id ?? null);
  let management = $derived(extensions.management(profileId));
  let mutation = $derived(extensions.activeManagementMutation());
  let notice = $derived(extensions.managementFailure());
  let catalogRevision = $derived(
    management?.phase === "ready" ? management.catalog_revision : null,
  );

  async function load() {
    requestFailed = !(await extensions.setManagementVisible(true));
  }

  $effect(() => {
    if (!open) return;
    const currentProfile = profileId;
    if (currentProfile === null) {
      subscribedProfile = null;
      requestFailed = true;
      untrack(() => void extensions.setManagementVisible(false));
      return;
    }
    if (subscribedProfile === currentProfile) return;
    subscribedProfile = currentProfile;
    confirming = null;
    reviewingCandidate = null;
    selectedOptionalApi = [];
    selectedOptionalHosts = [];
    showAllRequiredHosts = false;
    untrack(() => void load());
  });

  function show() {
    if (open) {
      hide(true);
      return;
    }
    requestFailed = false;
    subscribedProfile = null;
    open = true;
    queueMicrotask(() => closeButton?.focus());
  }

  function hide(returnFocus = false) {
    if (!open) return;
    open = false;
    subscribedProfile = null;
    confirming = null;
    reviewingCandidate = null;
    selectedOptionalApi = [];
    selectedOptionalHosts = [];
    showAllRequiredHosts = false;
    void extensions.setManagementVisible(false);
    if (returnFocus) queueMicrotask(() => trigger?.focus());
  }

  function retry() {
    if (profileId === null) {
      requestFailed = true;
      return;
    }
    requestFailed = false;
    subscribedProfile = profileId;
    void load();
  }

  function handleWindowPointerDown(event: PointerEvent) {
    if (!open || !(event.target instanceof Node) || root?.contains(event.target) === true) return;
    hide();
  }

  function handleWindowKeydown(event: KeyboardEvent) {
    if (!open) return;
    if (event.key === "Escape") {
      event.preventDefault();
      hide(true);
      return;
    }
    if (event.key !== "Tab" || panel === undefined) return;
    const focusable = [
      ...panel.querySelectorAll<HTMLElement>("button:not([disabled]), input:not([disabled])"),
    ];
    if (focusable.length === 0) return;
    const first = focusable.at(0);
    const last = focusable.at(-1);
    if (first === undefined || last === undefined) return;
    if (event.shiftKey && document.activeElement === first) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && document.activeElement === last) {
      event.preventDefault();
      first.focus();
    }
  }

  function toggle(entry: ExtensionManagementEntryView) {
    if (catalogRevision === null || mutation !== null) return;
    extensions.setEnabled(entry, catalogRevision, entry.runtime === "disabled");
  }

  function remove(entry: ExtensionManagementEntryView) {
    if (catalogRevision === null || mutation !== null) return;
    extensions.uninstall(entry, catalogRevision);
    confirming = null;
  }

  function reviewInstall(candidate: ExtensionInstallCandidateView) {
    if (mutation !== null) return;
    confirming = null;
    reviewingCandidate = candidate.candidate_index;
    selectedOptionalApi = [];
    selectedOptionalHosts = [];
    showAllRequiredHosts = false;
    allowFileAccess = false;
    allowPrivateAccess = false;
  }

  function install(candidate: ExtensionInstallCandidateView) {
    if (catalogRevision === null || mutation !== null) return;
    extensions.install(
      candidate,
      catalogRevision,
      selectedOptionalApi,
      selectedOptionalHosts,
      allowFileAccess,
      allowPrivateAccess,
    );
    reviewingCandidate = null;
  }

  function selectOptionalApi(index: number, selected: boolean) {
    selectedOptionalApi = selected
      ? [...selectedOptionalApi, index].sort((left, right) => left - right)
      : selectedOptionalApi.filter((entry) => entry !== index);
  }

  function selectOptionalHost(
    candidate: ExtensionInstallCandidateView,
    index: number,
    selected: boolean,
  ) {
    selectedOptionalHosts = selected
      ? [...selectedOptionalHosts, index].sort((left, right) => left - right)
      : selectedOptionalHosts.filter((entry) => entry !== index);
    if (!fileAccessAvailable(candidate)) allowFileAccess = false;
  }

  const COLLAPSED_REQUIRED_HOST_COUNT = 6;

  const patternIncludesFiles = (pattern: string) =>
    pattern === "<all_urls>" || pattern.startsWith("file://");

  const fileAccessAvailable = (candidate: ExtensionInstallCandidateView) =>
    candidate.required_hosts.some(patternIncludesFiles) ||
    selectedOptionalHosts.some((index) => {
      const pattern = candidate.optional_hosts[index];
      return pattern !== undefined && patternIncludesFiles(pattern);
    });

  const runtimeLabel = (runtime: ExtensionManagementRuntimeView) => {
    switch (runtime) {
      case "active":
        return "Active";
      case "pending_activation":
        return "Waiting to activate";
      case "disabled":
        return "Disabled";
    }
  };

  const limitationKey = (limitation: ExtensionManagementLimitationView) =>
    limitation.type === "api_permission"
      ? `${limitation.type}:${limitation.name}`
      : limitation.type;
</script>

<svelte:window onkeydown={handleWindowKeydown} onpointerdown={handleWindowPointerDown} />

<div bind:this={root} class="relative flex shrink-0">
  <button
    bind:this={trigger}
    type="button"
    aria-label="Manage extensions"
    aria-haspopup="dialog"
    aria-expanded={open}
    aria-controls="extension-manager"
    title="Manage extensions"
    class="icon-button"
    class:bg-fill={open}
    class:text-text={open}
    style:--icon-button-size="28px"
    onclick={show}
  >
    <Icon icon={PuzzleIcon} size={16} />
  </button>

  {#if open}
    <div
      bind:this={panel}
      id="extension-manager"
      role="dialog"
      aria-modal="true"
      aria-labelledby="extension-manager-title"
      class="absolute z-30 max-h-[min(560px,calc(100vh-24px))] w-[min(320px,calc(100vw-16px))] overflow-y-auto rounded-lg border border-border-strong bg-raised p-2.5 text-start shadow-[var(--shadow-overlay)]"
      class:top-full={!compact}
      class:right-0={!compact}
      class:mt-1.5={!compact}
      class:top-0={compact}
      class:left-full={compact}
      class:ml-1.5={compact}
    >
      <div class="mb-2 flex items-center justify-between gap-2">
        <div class="min-w-0">
          <h2 id="extension-manager-title" class="text-[13px] leading-4 font-medium text-text">
            Extensions
          </h2>
          <p class="mt-0.5 text-[10.5px] leading-4 text-faint">Current profile</p>
        </div>
        <button
          bind:this={closeButton}
          type="button"
          aria-label="Close extension manager"
          class="icon-button shrink-0"
          style:--icon-button-size="24px"
          onclick={() => hide(true)}
        >
          <Icon icon={Cancel01Icon} size={14} />
        </button>
      </div>

      {#if requestFailed || management === null || management.phase !== "ready"}
        <div class="rounded-md bg-fill px-2.5 py-3 text-[11.5px] leading-4 text-muted">
          {#if !requestFailed && (management === null || management.phase === "loading")}
            <p role="status">Loading installed extensions…</p>
          {:else}
            <p role="alert">
              {#if management?.phase === "rejected"}
                The installed extension catalog could not be authenticated.
              {:else if management?.phase === "failed_closed"}
                Extension management stopped to protect this profile.
              {:else}
                Extension management is unavailable right now.
              {/if}
            </p>
            <button
              type="button"
              class="hover:bg-fill-strong mt-2 inline-flex h-7 items-center gap-1.5 rounded-md px-2 text-[11.5px] font-medium text-text"
              onclick={retry}
            >
              <Icon icon={Refresh01Icon} size={13} />
              Retry
            </button>
          {/if}
        </div>
      {:else}
        {#if management.entries.length === 0}
          <p class="rounded-md bg-fill px-2.5 py-3 text-[11.5px] leading-4 text-muted">
            No extensions are installed in this profile.
          </p>
        {:else}
          <div class="space-y-1.5">
            {#each management.entries as entry (entry.install_id)}
              <article class="rounded-md bg-fill px-2.5 py-2">
                <div class="flex items-start gap-2">
                  <span
                    class="mt-0.5 flex h-7 w-7 shrink-0 items-center justify-center rounded-md bg-raised text-muted"
                    aria-hidden="true"
                  >
                    <Icon icon={PuzzleIcon} size={15} />
                  </span>
                  <div class="min-w-0 flex-1">
                    <div class="flex items-start justify-between gap-2">
                      <div class="min-w-0">
                        <h3 class="truncate text-[12.5px] leading-4 font-medium text-text">
                          {entry.name}
                        </h3>
                        <p class="truncate text-[10.5px] leading-4 text-faint">
                          {entry.version} · {runtimeLabel(entry.runtime)}
                        </p>
                      </div>
                      <button
                        type="button"
                        role="switch"
                        aria-label={`${entry.runtime === "disabled" ? "Enable" : "Disable"} ${entry.name}`}
                        aria-checked={entry.runtime !== "disabled"}
                        disabled={mutation !== null}
                        class="relative mt-0.5 h-[18px] w-8 shrink-0 rounded-full bg-border-strong transition-colors disabled:opacity-45"
                        class:bg-accent={entry.runtime !== "disabled"}
                        onclick={() => toggle(entry)}
                      >
                        <span
                          aria-hidden="true"
                          class="absolute top-[2px] left-[2px] h-3.5 w-3.5 rounded-full bg-white shadow-sm transition-transform"
                          class:translate-x-3.5={entry.runtime !== "disabled"}
                        ></span>
                      </button>
                    </div>

                    {#if entry.compatibility === "degraded"}
                      <ul class="mt-1 text-[10.5px] leading-4 text-warning">
                        {#each entry.limitations.slice(0, 3) as limitation (limitationKey(limitation))}
                          <li>• {compatibilityLimitationLabel(limitation)}</li>
                        {/each}
                        {#if entry.limitations.length > 3}
                          <li>• {entry.limitations.length - 3} more limitations</li>
                        {/if}
                      </ul>
                    {/if}
                    {#if entry.grants.initialized}
                      <p class="mt-1 text-[10.5px] leading-4 text-muted">
                        {entry.grants.api_grants} API · {entry.grants.host_grants} site
                        {entry.grants.host_grants === 1 ? "permission" : "permissions"}
                        {#if entry.grants.file_access}
                          · File access{/if}
                        {#if entry.grants.private_access}
                          · Private windows{/if}
                      </p>
                    {:else}
                      <p class="mt-1 text-[10.5px] leading-4 text-muted">No permissions granted</p>
                    {/if}
                  </div>
                </div>

                <div class="mt-1.5 flex min-h-7 items-center justify-end gap-1">
                  {#if confirming === entry.install_id}
                    <span class="mr-auto text-[10.5px] leading-4 text-muted">Remove extension?</span
                    >
                    <button
                      type="button"
                      class="hover:bg-fill-strong h-7 rounded-md px-2 text-[11px] text-muted hover:text-text"
                      disabled={mutation !== null}
                      onclick={() => (confirming = null)}
                    >
                      Cancel
                    </button>
                    <button
                      type="button"
                      class="hover:bg-fill-strong h-7 rounded-md px-2 text-[11px] font-medium text-warning"
                      disabled={mutation !== null}
                      onclick={() => remove(entry)}
                    >
                      Remove
                    </button>
                  {:else}
                    <button
                      type="button"
                      aria-label={`Remove ${entry.name}`}
                      title={`Remove ${entry.name}`}
                      disabled={mutation !== null}
                      class="icon-button text-faint hover:text-warning"
                      style:--icon-button-size="26px"
                      onclick={() => (confirming = entry.install_id)}
                    >
                      <Icon icon={Delete02Icon} size={13} />
                    </button>
                  {/if}
                </div>
              </article>
            {/each}
          </div>
        {/if}

        {#if management.candidates.length > 0}
          <div class="mt-3 border-t border-border pt-2.5">
            <h3 class="mb-1.5 text-[10.5px] leading-4 font-medium text-faint">Available</h3>
            <div class="space-y-1.5">
              {#each management.candidates as candidate (candidate.candidate_index)}
                <article class="rounded-md bg-fill px-2.5 py-2">
                  <div class="flex items-start gap-2">
                    <span
                      class="mt-0.5 flex h-7 w-7 shrink-0 items-center justify-center rounded-md bg-raised text-muted"
                      aria-hidden="true"
                    >
                      <Icon icon={PuzzleIcon} size={15} />
                    </span>
                    <div class="min-w-0 flex-1">
                      <h4 class="truncate text-[12.5px] leading-4 font-medium text-text">
                        {candidate.name}
                      </h4>
                      <p class="truncate text-[10.5px] leading-4 text-faint">
                        {candidate.version}{candidate.author === null
                          ? ""
                          : ` · ${candidate.author}`}
                      </p>
                      {#if candidate.description !== null}
                        <p class="mt-1 text-[10.5px] leading-4 text-muted">
                          {candidate.description}
                        </p>
                      {/if}
                      {#if candidate.compatibility === "degraded"}
                        <p class="mt-1 text-[10.5px] leading-4 text-warning">
                          Review platform limitations before installing.
                        </p>
                      {/if}
                    </div>
                  </div>

                  {#if reviewingCandidate === candidate.candidate_index}
                    <div class="mt-2 border-t border-border pt-2">
                      {#if candidate.limitations.length > 0}
                        <div class="mb-2 rounded-md bg-warning/10 px-2 py-1.5">
                          <p class="text-[11px] leading-4 font-medium text-warning">
                            Platform limitations
                          </p>
                          <ul class="mt-1 space-y-0.5 text-[10.5px] leading-4 text-warning">
                            {#each candidate.limitations as limitation (limitationKey(limitation))}
                              <li>• {compatibilityLimitationLabel(limitation)}</li>
                            {/each}
                          </ul>
                        </div>
                      {/if}
                      <p class="text-[11px] leading-4 font-medium text-text">Required access</p>
                      {#if candidate.required_api.length === 0 && candidate.required_hosts.length === 0}
                        <p class="mt-1 text-[10.5px] leading-4 text-muted">
                          No additional site or browser access.
                        </p>
                      {:else}
                        <ul class="mt-1 space-y-1 text-[10.5px] leading-4 text-muted">
                          {#each candidate.required_api as permission (permission)}
                            <li>• {apiPermissionLabel(permission)}</li>
                          {/each}
                          {#each showAllRequiredHosts ? candidate.required_hosts : candidate.required_hosts.slice(0, COLLAPSED_REQUIRED_HOST_COUNT) as pattern (pattern)}
                            <li>• {hostPermissionLabel(pattern)}</li>
                          {/each}
                        </ul>
                        {#if candidate.required_hosts.length > COLLAPSED_REQUIRED_HOST_COUNT}
                          <button
                            type="button"
                            class="hover:bg-fill-strong mt-1 h-6 rounded-md px-1.5 text-[10.5px] font-medium text-muted hover:text-text"
                            onclick={() => (showAllRequiredHosts = !showAllRequiredHosts)}
                          >
                            {showAllRequiredHosts
                              ? "Show fewer sites"
                              : `Show ${candidate.required_hosts.length - COLLAPSED_REQUIRED_HOST_COUNT} more sites`}
                          </button>
                        {/if}
                      {/if}
                      {#if candidate.optional_api.length > 0 || candidate.optional_hosts.length > 0}
                        <fieldset class="mt-2 border-0 p-0">
                          <legend class="text-[11px] leading-4 font-medium text-text">
                            Optional access
                          </legend>
                          <div class="mt-1 space-y-1.5">
                            {#each candidate.optional_api as permission, index (permission)}
                              <label
                                class="flex cursor-pointer items-start gap-2 text-[10.5px] leading-4 text-muted"
                              >
                                <input
                                  type="checkbox"
                                  class="mt-0.5 accent-accent"
                                  checked={selectedOptionalApi.includes(index)}
                                  disabled={mutation !== null}
                                  onchange={(event) =>
                                    selectOptionalApi(index, event.currentTarget.checked)}
                                />
                                <span>{apiPermissionLabel(permission)}</span>
                              </label>
                            {/each}
                            {#each candidate.optional_hosts as pattern, index (pattern)}
                              <label
                                class="flex cursor-pointer items-start gap-2 text-[10.5px] leading-4 text-muted"
                              >
                                <input
                                  type="checkbox"
                                  class="mt-0.5 accent-accent"
                                  checked={selectedOptionalHosts.includes(index)}
                                  disabled={mutation !== null}
                                  onchange={(event) =>
                                    selectOptionalHost(
                                      candidate,
                                      index,
                                      event.currentTarget.checked,
                                    )}
                                />
                                <span>{hostPermissionLabel(pattern)}</span>
                              </label>
                            {/each}
                          </div>
                        </fieldset>
                      {/if}
                      {#if candidate.supports_file_access}
                        <label
                          class="mt-2 flex cursor-pointer items-start gap-2 text-[10.5px] leading-4 text-muted"
                          class:cursor-not-allowed={!fileAccessAvailable(candidate)}
                          class:opacity-55={!fileAccessAvailable(candidate)}
                        >
                          <input
                            type="checkbox"
                            class="mt-0.5 accent-accent"
                            bind:checked={allowFileAccess}
                            disabled={mutation !== null || !fileAccessAvailable(candidate)}
                          />
                          <span>Allow access to local file URLs</span>
                        </label>
                      {/if}
                      <label
                        class="mt-1.5 flex cursor-pointer items-start gap-2 text-[10.5px] leading-4 text-muted"
                      >
                        <input
                          type="checkbox"
                          class="mt-0.5 accent-accent"
                          bind:checked={allowPrivateAccess}
                          disabled={mutation !== null}
                        />
                        <span>Allow in private windows</span>
                      </label>
                      <div class="mt-2 flex justify-end gap-1">
                        <button
                          type="button"
                          class="hover:bg-fill-strong h-7 rounded-md px-2 text-[11px] text-muted hover:text-text"
                          disabled={mutation !== null}
                          onclick={() => (reviewingCandidate = null)}
                        >
                          Cancel
                        </button>
                        <button
                          type="button"
                          class="h-7 rounded-md bg-accent px-2.5 text-[11px] font-medium text-white disabled:opacity-45"
                          disabled={mutation !== null}
                          onclick={() => install(candidate)}
                        >
                          Install
                        </button>
                      </div>
                    </div>
                  {:else}
                    <div class="mt-1.5 flex justify-end">
                      <button
                        type="button"
                        class="hover:bg-fill-strong h-7 rounded-md px-2 text-[11px] font-medium text-text"
                        disabled={mutation !== null}
                        onclick={() => reviewInstall(candidate)}
                      >
                        Install…
                      </button>
                    </div>
                  {/if}
                </article>
              {/each}
            </div>
          </div>
        {/if}
      {/if}

      {#if mutation !== null}
        <p class="mt-2 text-[10.5px] leading-4 text-muted" role="status" aria-live="polite">
          {mutation.kind === "install"
            ? "Installing extension…"
            : mutation.kind === "uninstall"
              ? "Removing extension…"
              : "Applying extension change…"}
        </p>
      {:else if notice !== null}
        <p class="mt-2 text-[10.5px] leading-4 text-warning" role="status" aria-live="polite">
          {notice}
        </p>
      {/if}
    </div>
  {/if}
</div>
