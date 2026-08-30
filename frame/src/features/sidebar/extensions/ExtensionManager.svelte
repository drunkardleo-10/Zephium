<script lang="ts">
  import {
    Cancel01Icon,
    Delete02Icon,
    Key01Icon,
    PuzzleIcon,
    Refresh01Icon,
  } from "@hugeicons/core-free-icons";
  import { onDestroy, untrack } from "svelte";
  import type {
    ExtensionInstallCandidateView,
    ExtensionManagementEntryView,
    ExtensionManagementLimitationView,
    ExtensionManagementRuntimeView,
    ExtensionManagementSourceView,
  } from "../../../shared/ipc/bindings";
  import { browserPasskeyStatus } from "../../../domain/credentials/browser-credentials-model";
  import * as extensions from "../../../domain/extensions/extensions.svelte";
  import * as browserCredentials from "../../../domain/credentials/browser-credentials.svelte";
  import {
    apiPermissionLabel,
    compatibilityLimitationLabel,
    hostPermissionLabel,
  } from "../../../domain/extensions/permission-labels";
  import {
    adjacentExtensionCenterSection,
    extensionProvenanceHost,
    extensionSourcePresentation,
    initialExtensionCenterSection,
    type ExtensionCenterSection,
  } from "../../../domain/extensions/extension-presentation";
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
  let expandedPermissions = $state<string | null>(null);
  let reviewingCandidate = $state<number | null>(null);
  let selectedOptionalApi = $state.raw<number[]>([]);
  let selectedOptionalHosts = $state.raw<number[]>([]);
  let showAllRequiredHosts = $state(false);
  let allowFileAccess = $state(false);
  let allowPrivateAccess = $state(false);
  let section = $state<ExtensionCenterSection>("installed");
  let sectionChosen = $state(false);
  let catalogSyncAttemptedProfile = $state<string | null>(null);

  let profileId = $derived(tabs.profile()?.id ?? null);
  let management = $derived(extensions.management(profileId));
  let managementAvailability = $derived(extensions.managementAvailability());
  let distribution = $derived(extensions.distribution());
  let distributionNotice = $derived(extensions.distributionNotice());
  let distributionRefreshBusy = $derived(extensions.distributionRefreshBusy());
  let distributionRefreshFailure = $derived(extensions.distributionRefreshFailure());
  let distributionRefreshVisible = $derived(
    distribution !== null && distribution.state.phase !== "shutdown",
  );
  let distributionRefreshDisabled = $derived(
    distributionRefreshBusy || distribution?.state.phase === "quarantined",
  );
  let mutation = $derived(extensions.activeManagementMutation());
  let notice = $derived(extensions.managementFailure());
  let catalogRevision = $derived(
    management?.phase === "ready" ? management.catalog_revision : null,
  );
  let pendingUpdate = $derived(
    management?.phase === "update_consent_required" ? management.pending_update : null,
  );
  let profilePolicy = $derived(management?.phase === "ready" ? management.profile_policy : null);
  let credentialCapability = $derived(browserCredentials.current());
  let passkeyRequestBusy = $derived(browserCredentials.busy());

  onDestroy(() => browserCredentials.deactivate());

  async function load() {
    requestFailed = !(await extensions.setManagementVisible(true));
  }

  $effect(() => {
    if (managementAvailability !== "configured" && open) {
      untrack(() => hide());
    }
  });

  $effect(() => {
    if (
      !open ||
      profileId === null ||
      management?.phase !== "catalog_not_synchronized" ||
      !distributionRefreshVisible ||
      distributionRefreshDisabled ||
      catalogSyncAttemptedProfile === profileId
    ) {
      return;
    }
    catalogSyncAttemptedProfile = profileId;
    untrack(() => void extensions.refreshDistribution());
  });

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
    expandedPermissions = null;
    reviewingCandidate = null;
    selectedOptionalApi = [];
    selectedOptionalHosts = [];
    showAllRequiredHosts = false;
    sectionChosen = false;
    untrack(() => void load());
  });

  $effect(() => {
    if (!open || sectionChosen || management?.phase !== "ready") {
      return;
    }
    section = initialExtensionCenterSection(
      management.entries.length,
      management.candidates.length,
    );
    sectionChosen = true;
  });

  function show() {
    if (open) {
      hide(true);
      return;
    }
    requestFailed = false;
    subscribedProfile = null;
    catalogSyncAttemptedProfile = null;
    section = "installed";
    sectionChosen = false;
    open = true;
    void browserCredentials.activate();
    queueMicrotask(() => closeButton?.focus());
  }

  function hide(returnFocus = false) {
    if (!open) return;
    open = false;
    subscribedProfile = null;
    catalogSyncAttemptedProfile = null;
    confirming = null;
    expandedPermissions = null;
    reviewingCandidate = null;
    selectedOptionalApi = [];
    selectedOptionalHosts = [];
    showAllRequiredHosts = false;
    section = "installed";
    sectionChosen = false;
    browserCredentials.deactivate();
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

  function refreshDistribution() {
    void extensions.refreshDistribution();
  }

  function selectSection(next: ExtensionCenterSection) {
    section = next;
    sectionChosen = true;
    confirming = null;
    expandedPermissions = null;
    reviewingCandidate = null;
    selectedOptionalApi = [];
    selectedOptionalHosts = [];
    showAllRequiredHosts = false;
    allowFileAccess = false;
    allowPrivateAccess = false;
  }

  function handleSectionKeydown(event: KeyboardEvent) {
    if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;
    event.preventDefault();
    const next = adjacentExtensionCenterSection(section);
    selectSection(next);
    queueMicrotask(() => document.getElementById(`extensions-${next}-tab`)?.focus());
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
      ...panel.querySelectorAll<HTMLElement>(
        'button:not([disabled]):not([tabindex="-1"]), input:not([disabled])',
      ),
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
    expandedPermissions = null;
  }

  function openOptions(entry: ExtensionManagementEntryView) {
    if (catalogRevision === null || mutation !== null) return;
    extensions.openOptions(entry, catalogRevision);
  }

  function editOptionalGrant(
    entry: ExtensionManagementEntryView,
    kind: "api" | "host",
    index: number,
    granted: boolean,
  ) {
    if (catalogRevision === null || mutation !== null || !entry.grants.initialized) return;
    extensions.editOptionalGrant(entry, catalogRevision, kind, index, granted);
  }

  function setProfilePaused(paused: boolean) {
    if (profilePolicy === null || mutation !== null) return;
    extensions.setProfilePaused(profilePolicy.revision, paused);
  }

  function setCurrentSiteEnabled(enabled: boolean) {
    if (profilePolicy === null || !profilePolicy.current_site_available || mutation !== null)
      return;
    extensions.setCurrentSiteEnabled(profilePolicy.revision, enabled);
  }

  const requiredApiPermissions = (entry: ExtensionManagementEntryView) =>
    entry.grants.api_permissions.filter((permission) => !entry.optional_api.includes(permission));

  const requiredHostPermissions = (entry: ExtensionManagementEntryView) =>
    entry.grants.host_permissions.filter(
      (permission) => !entry.optional_hosts.includes(permission),
    );

  function reviewInstall(candidate: ExtensionInstallCandidateView) {
    if (mutation !== null) return;
    confirming = null;
    expandedPermissions = null;
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
      allowFileAccess && candidate.file_access_available,
      allowPrivateAccess && candidate.private_access_available,
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
    if (!fileScopeSelected(candidate)) allowFileAccess = false;
  }

  const COLLAPSED_REQUIRED_HOST_COUNT = 6;

  const patternIncludesFiles = (pattern: string) =>
    pattern === "<all_urls>" || pattern.startsWith("file://");

  const fileScopeSelected = (candidate: ExtensionInstallCandidateView) =>
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
      case "profile_paused":
        return "Paused by profile";
      case "disabled":
        return "Disabled";
    }
  };

  const verifiedDateFormatter = new Intl.DateTimeFormat(undefined, {
    dateStyle: "medium",
    timeZone: "UTC",
  });

  const sourceLabel = (source: ExtensionManagementSourceView, verifiedUnix: string | null) => {
    const presentation = extensionSourcePresentation(source, verifiedUnix);
    return presentation.verifiedAt === null
      ? presentation.label
      : `${presentation.label} · ${verifiedDateFormatter.format(presentation.verifiedAt)}`;
  };

  const limitationKey = (limitation: ExtensionManagementLimitationView) =>
    limitation.type === "api_permission"
      ? `${limitation.type}:${limitation.name}`
      : limitation.type;

  const candidateSectionIsVerified = () =>
    management !== null &&
    management.candidates.length > 0 &&
    management.candidates.every((candidate) => candidate.source === "zephium_verified");
</script>

<svelte:window onkeydown={handleWindowKeydown} onpointerdown={handleWindowPointerDown} />

<div
  bind:this={root}
  class="relative flex shrink-0"
  class:hidden={managementAvailability !== "configured"}
  data-compact={compact}
>
  <button
    bind:this={trigger}
    type="button"
    aria-label="Open Extensions Center"
    aria-haspopup="dialog"
    aria-expanded={open}
    aria-controls="extension-manager"
    disabled={managementAvailability !== "configured"}
    title="Extensions Center"
    class="icon-button"
    class:bg-fill={open}
    class:text-text={open}
    style:--icon-button-size="28px"
    onclick={show}
  >
    <Icon icon={PuzzleIcon} size={16} />
  </button>

  {#if open}
    <button
      type="button"
      tabindex="-1"
      aria-label="Close Extensions Center"
      class="fixed inset-0 z-40 cursor-default bg-canvas/80"
      onclick={() => hide(true)}
    ></button>
    <div
      bind:this={panel}
      id="extension-manager"
      role="dialog"
      aria-modal="true"
      aria-labelledby="extension-manager-title"
      class="fixed top-1/2 left-1/2 z-50 max-h-[min(680px,calc(100vh-24px))] w-[min(760px,calc(100vw-24px))] -translate-x-1/2 -translate-y-1/2 overflow-y-auto rounded-xl border border-border-strong bg-raised p-4 text-start shadow-[var(--shadow-overlay)]"
      class:min-h-[min(520px,calc(100vh-24px))]={management?.phase === "ready"}
    >
      <div class="mb-3 flex items-center justify-between gap-3 border-b border-border pb-3">
        <div class="min-w-0">
          <h2 id="extension-manager-title" class="text-[15px] leading-5 font-semibold text-text">
            Extensions Center
          </h2>
          <p class="mt-0.5 text-[11px] leading-4 text-faint">Extensions for the current profile</p>
        </div>
        <div class="flex shrink-0 items-center gap-1">
          {#if distributionRefreshVisible}
            <button
              type="button"
              aria-label="Check for extension updates"
              title="Check for extension updates"
              disabled={distributionRefreshDisabled}
              class="hover:bg-fill-strong inline-flex h-8 items-center gap-1.5 rounded-md px-2.5 text-[11px] font-medium text-muted hover:text-text disabled:opacity-45"
              onclick={refreshDistribution}
            >
              <Icon
                icon={Refresh01Icon}
                size={12}
                class={distributionRefreshBusy ? "animate-spin" : undefined}
              />
              {distributionRefreshBusy ? "Checking…" : "Check updates"}
            </button>
          {/if}
          <button
            bind:this={closeButton}
            type="button"
            aria-label="Close Extensions Center"
            class="icon-button shrink-0"
            style:--icon-button-size="28px"
            onclick={() => hide(true)}
          >
            <Icon icon={Cancel01Icon} size={14} />
          </button>
        </div>
      </div>

      {#if distributionNotice !== null}
        <p
          class="mb-2 rounded-md bg-fill px-2.5 py-2 text-[10.5px] leading-4 text-muted"
          class:text-warning={distributionNotice.tone === "warning"}
          role={distributionNotice.tone === "warning" ? "alert" : "status"}
          aria-live={distributionNotice.tone === "warning" ? "assertive" : "polite"}
        >
          {distributionNotice.message}
        </p>
      {:else if distributionRefreshFailure !== null}
        <p
          class="mb-2 rounded-md bg-fill px-2.5 py-2 text-[10.5px] leading-4 text-warning"
          role="alert"
          aria-live="assertive"
        >
          {distributionRefreshFailure}
        </p>
      {/if}

      {#if management?.phase === "ready"}
        <div
          class="mb-3 flex w-fit items-center gap-1 rounded-lg bg-fill p-1"
          role="tablist"
          aria-label="Extension sections"
        >
          <button
            type="button"
            role="tab"
            id="extensions-installed-tab"
            aria-controls="extensions-installed-panel"
            aria-selected={section === "installed"}
            tabindex={section === "installed" ? 0 : -1}
            class="h-7 rounded-md px-2.5 text-[11px] font-medium text-muted transition-colors hover:text-text"
            class:bg-raised={section === "installed"}
            class:text-text={section === "installed"}
            onclick={() => selectSection("installed")}
            onkeydown={handleSectionKeydown}
          >
            Installed · {management.entries.length}
          </button>
          <button
            type="button"
            role="tab"
            id="extensions-verified-tab"
            aria-controls="extensions-verified-panel"
            aria-selected={section === "verified"}
            tabindex={section === "verified" ? 0 : -1}
            class="h-7 rounded-md px-2.5 text-[11px] font-medium text-muted transition-colors hover:text-text"
            class:bg-raised={section === "verified"}
            class:text-text={section === "verified"}
            onclick={() => selectSection("verified")}
            onkeydown={handleSectionKeydown}
          >
            {candidateSectionIsVerified() ? "Verified" : "Available"} · {management.candidates
              .length}
          </button>
        </div>
        {#if profilePolicy !== null}
          <div class="mb-3 grid gap-1 rounded-lg bg-fill px-2.5 py-2 text-[10.5px] leading-4">
            <div class="flex min-h-7 items-center justify-between gap-3">
              <div class="min-w-0">
                <p class="font-medium text-text">Pause extensions</p>
                <p class="text-faint">
                  Stops every extension in this profile without uninstalling.
                </p>
              </div>
              <button
                type="button"
                role="switch"
                aria-label="Pause extensions in this profile"
                aria-checked={profilePolicy.paused}
                disabled={mutation !== null}
                class="relative h-[18px] w-8 shrink-0 rounded-full bg-border-strong transition-colors disabled:opacity-45"
                class:bg-accent={profilePolicy.paused}
                onclick={() => setProfilePaused(!profilePolicy.paused)}
              >
                <span
                  aria-hidden="true"
                  class="absolute top-[2px] left-[2px] h-3.5 w-3.5 rounded-full bg-white shadow-sm transition-transform"
                  class:translate-x-3.5={profilePolicy.paused}
                ></span>
              </button>
            </div>
            {#if profilePolicy.current_site_available}
              <div
                class="flex min-h-7 items-center justify-between gap-3 border-t border-border pt-1.5"
              >
                <div class="min-w-0">
                  <p class="font-medium text-text">Extensions on this site</p>
                  <p class="text-faint">
                    {profilePolicy.current_site_denied
                      ? "Paused for the current site."
                      : "Allowed for the current site."}
                  </p>
                </div>
                <button
                  type="button"
                  role="switch"
                  aria-label="Allow extensions on the current site"
                  aria-checked={!profilePolicy.current_site_denied}
                  disabled={mutation !== null || profilePolicy.paused}
                  class="relative h-[18px] w-8 shrink-0 rounded-full bg-border-strong transition-colors disabled:opacity-45"
                  class:bg-accent={!profilePolicy.current_site_denied}
                  onclick={() => setCurrentSiteEnabled(profilePolicy.current_site_denied)}
                >
                  <span
                    aria-hidden="true"
                    class="absolute top-[2px] left-[2px] h-3.5 w-3.5 rounded-full bg-white shadow-sm transition-transform"
                    class:translate-x-3.5={!profilePolicy.current_site_denied}
                  ></span>
                </button>
              </div>
            {/if}
            {#if profilePolicy.denied_site_count > 0}
              <p class="text-faint">
                {profilePolicy.denied_site_count} site{profilePolicy.denied_site_count === 1
                  ? ""
                  : "s"} paused
              </p>
            {/if}
          </div>
        {/if}
      {/if}

      {#if !requestFailed && pendingUpdate !== null}
        <article class="rounded-md bg-fill px-3 py-3">
          <div class="flex items-start gap-2.5">
            <span
              class="mt-0.5 flex h-8 w-8 shrink-0 items-center justify-center rounded-md bg-raised text-muted"
              aria-hidden="true"
            >
              <Icon icon={PuzzleIcon} size={16} />
            </span>
            <div class="min-w-0 flex-1">
              <h3 class="text-[12.5px] leading-4 font-medium text-text">
                {pendingUpdate.name} update needs review
              </h3>
              <p class="mt-0.5 text-[10.5px] leading-4 text-faint">
                Review version {pendingUpdate.version} before updating. Your current version remains installed
                and unchanged until you approve.
              </p>
              <span
                class="mt-1 inline-flex rounded-full bg-raised px-1.5 py-0.5 text-[9.5px] leading-3 font-medium text-accent"
              >
                {sourceLabel(pendingUpdate.source, pendingUpdate.verified_catalog_unix)}
              </span>
              {#if pendingUpdate.provenance !== null}
                <p class="mt-1 text-[10.5px] leading-4 text-muted">
                  {pendingUpdate.provenance.attribution} ·
                  {pendingUpdate.provenance.license_expression}
                </p>
              {/if}
            </div>
          </div>

          {#if pendingUpdate.added_required_api.length + pendingUpdate.added_required_hosts.length > 0}
            <div class="mt-3 rounded-md bg-raised px-2.5 py-2">
              <p class="text-[10.5px] leading-4 font-medium text-text">New required access</p>
              <ul class="mt-1 text-[10.5px] leading-4 text-muted">
                {#each pendingUpdate.added_required_api as permission (permission)}
                  <li>• {apiPermissionLabel(permission)}</li>
                {/each}
                {#each pendingUpdate.added_required_hosts as pattern (pattern)}
                  <li>• {hostPermissionLabel(pattern)}</li>
                {/each}
              </ul>
            </div>
          {/if}

          {#if pendingUpdate.compatibility === "degraded"}
            <div class="mt-2 rounded-md bg-raised px-2.5 py-2 text-[10.5px] leading-4 text-warning">
              <p class="font-medium">New compatibility limitations</p>
              <ul class="mt-1">
                {#each pendingUpdate.limitations as limitation (limitationKey(limitation))}
                  <li>• {compatibilityLimitationLabel(limitation)}</li>
                {/each}
              </ul>
            </div>
          {/if}

          <div class="mt-3 flex items-center justify-between gap-2">
            <p class="text-[10px] leading-4 text-faint">
              Close this window to keep the current version.
            </p>
            <button
              type="button"
              class="h-8 rounded-md bg-accent px-3 text-[11px] font-medium text-white hover:bg-accent/90 disabled:opacity-45"
              disabled={mutation !== null}
              onclick={() => extensions.approveUpdate(pendingUpdate)}
            >
              {mutation?.kind === "update" ? "Updating…" : "Approve and update"}
            </button>
          </div>
        </article>
      {:else if requestFailed || management === null || management.phase !== "ready"}
        <div class="rounded-md bg-fill px-2.5 py-3 text-[11.5px] leading-4 text-muted">
          {#if !requestFailed && (management === null || management.phase === "loading")}
            <p role="status">Loading installed extensions…</p>
          {:else}
            <p role="alert">
              {#if management?.phase === "rejected"}
                The installed extension catalog could not be authenticated.
              {:else if management?.phase === "not_configured"}
                Extensions are not configured in this build.
              {:else if management?.phase === "catalog_not_synchronized"}
                No verified extension catalog has been synchronized yet.
              {:else if management?.phase === "update_consent_required"}
                The extension update review could not be loaded safely.
              {:else if management?.phase === "failed_closed"}
                Extension management stopped to protect this profile.
              {:else}
                Extension management is unavailable right now.
              {/if}
            </p>
            {#if management?.phase !== "not_configured" && management?.phase !== "catalog_not_synchronized" && management?.phase !== "update_consent_required"}
              <button
                type="button"
                class="hover:bg-fill-strong mt-2 inline-flex h-7 items-center gap-1.5 rounded-md px-2 text-[11.5px] font-medium text-text"
                onclick={retry}
              >
                <Icon icon={Refresh01Icon} size={13} />
                Retry
              </button>
            {/if}
          {/if}
        </div>
      {:else}
        <div
          id={section === "installed" ? "extensions-installed-panel" : "extensions-verified-panel"}
          role="tabpanel"
          aria-labelledby={section === "installed"
            ? "extensions-installed-tab"
            : "extensions-verified-tab"}
        >
          {#if section === "installed"}
            {#if credentialCapability !== null && (credentialCapability.system_password_autofill || credentialCapability.passkey_authorization !== "unsupported")}
              <article class="mb-2 rounded-md bg-fill px-2.5 py-2">
                <div class="flex items-start gap-2">
                  <span
                    class="mt-0.5 flex h-7 w-7 shrink-0 items-center justify-center rounded-md bg-raised text-muted"
                    aria-hidden="true"
                  >
                    <Icon icon={Key01Icon} size={15} />
                  </span>
                  <div class="min-w-0 flex-1">
                    <div class="flex items-start justify-between gap-3">
                      <div class="min-w-0">
                        <h3 class="text-[12.5px] leading-4 font-medium text-text">
                          System passwords & passkeys
                        </h3>
                        {#if credentialCapability.system_password_autofill}
                          <p class="mt-0.5 text-[10.5px] leading-4 text-muted">
                            Password AutoFill is available through macOS credential providers.
                          </p>
                        {/if}
                        <p
                          class="mt-0.5 text-[10.5px] leading-4 text-faint"
                          class:text-warning={credentialCapability.passkey_authorization ===
                            "denied" ||
                            credentialCapability.passkey_authorization === "entitlement_required" ||
                            credentialCapability.passkey_authorization === "unknown" ||
                            credentialCapability.passkey_authorization === "unavailable"}
                          role="status"
                        >
                          {browserPasskeyStatus(credentialCapability.passkey_authorization)}
                        </p>
                      </div>
                      {#if credentialCapability.can_request_passkey_authorization}
                        <button
                          type="button"
                          class="hover:bg-fill-strong h-7 shrink-0 rounded-md px-2 text-[11px] font-medium text-text disabled:opacity-45"
                          disabled={passkeyRequestBusy}
                          onclick={() => void browserCredentials.requestPasskeyAuthorization()}
                        >
                          {passkeyRequestBusy ? "Waiting…" : "Enable passkeys"}
                        </button>
                      {/if}
                    </div>
                  </div>
                </div>
              </article>
            {/if}
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
                            <span
                              class="mt-1 inline-flex rounded-full bg-raised px-1.5 py-0.5 text-[9.5px] leading-3 font-medium text-muted"
                              class:text-accent={entry.source === "zephium_verified"}
                              class:text-warning={entry.source === "developer_local"}
                            >
                              {sourceLabel(entry.source, entry.verified_catalog_unix)}
                            </span>
                            {#if entry.provenance !== null}
                              <p class="mt-1 text-[10.5px] leading-4 text-muted">
                                {entry.provenance.attribution} ·
                                {entry.provenance.license_expression}
                              </p>
                              <p
                                class="truncate text-[10px] leading-4 text-faint"
                                title={entry.provenance.source_url}
                              >
                                Source: {extensionProvenanceHost(entry.provenance) ??
                                  "Authenticated catalog"}
                              </p>
                            {/if}
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
                            {entry.grants.api_permissions.length} API ·
                            {entry.grants.host_permissions.length} site
                            {entry.grants.host_permissions.length === 1
                              ? "permission"
                              : "permissions"}
                            {#if entry.grants.file_access}
                              · File access{/if}
                            {#if entry.grants.private_access}
                              · Private windows{/if}
                          </p>
                        {:else}
                          <p class="mt-1 text-[10.5px] leading-4 text-muted">
                            No permissions granted
                          </p>
                        {/if}
                      </div>
                    </div>

                    {#if expandedPermissions === entry.install_id}
                      <div
                        id={`extension-permissions-${entry.install_id}`}
                        class="mt-2 rounded-md bg-raised px-2.5 py-2 text-[10.5px] leading-4"
                      >
                        {#if requiredApiPermissions(entry).length > 0}
                          <p class="font-medium text-text">Required browser access</p>
                          <ul class="mt-0.5 text-muted">
                            {#each requiredApiPermissions(entry) as permission (permission)}
                              <li>• {apiPermissionLabel(permission)}</li>
                            {/each}
                          </ul>
                        {/if}
                        {#if entry.optional_api.length > 0}
                          <p
                            class="font-medium text-text"
                            class:mt-2={requiredApiPermissions(entry).length > 0}
                          >
                            Optional browser access
                          </p>
                          <ul class="mt-1 space-y-1 text-muted">
                            {#each entry.optional_api as permission, index (permission)}
                              {@const granted = entry.grants.api_permissions.includes(permission)}
                              <li class="flex min-h-6 items-center justify-between gap-3">
                                <span>{apiPermissionLabel(permission)}</span>
                                <button
                                  type="button"
                                  role="switch"
                                  aria-label={(granted ? "Revoke " : "Allow ") +
                                    apiPermissionLabel(permission)}
                                  aria-checked={granted}
                                  disabled={mutation !== null}
                                  class="relative h-[18px] w-8 shrink-0 rounded-full bg-border-strong transition-colors disabled:opacity-45"
                                  class:bg-accent={granted}
                                  onclick={() => editOptionalGrant(entry, "api", index, !granted)}
                                >
                                  <span
                                    aria-hidden="true"
                                    class="absolute top-[2px] left-[2px] h-3.5 w-3.5 rounded-full bg-white shadow-sm transition-transform"
                                    class:translate-x-3.5={granted}
                                  ></span>
                                </button>
                              </li>
                            {/each}
                          </ul>
                        {/if}
                        {#if requiredHostPermissions(entry).length > 0}
                          <p
                            class="font-medium text-text"
                            class:mt-2={requiredApiPermissions(entry).length > 0 ||
                              entry.optional_api.length > 0}
                          >
                            Required site access
                          </p>
                          <ul class="mt-0.5 text-muted">
                            {#each requiredHostPermissions(entry) as pattern (pattern)}
                              <li>• {hostPermissionLabel(pattern)}</li>
                            {/each}
                          </ul>
                        {/if}
                        {#if entry.optional_hosts.length > 0}
                          <p
                            class="font-medium text-text"
                            class:mt-2={requiredApiPermissions(entry).length > 0 ||
                              entry.optional_api.length > 0 ||
                              requiredHostPermissions(entry).length > 0}
                          >
                            Optional site access
                          </p>
                          <ul class="mt-1 space-y-1 text-muted">
                            {#each entry.optional_hosts as pattern, index (pattern)}
                              {@const granted = entry.grants.host_permissions.includes(pattern)}
                              <li class="flex min-h-6 items-center justify-between gap-3">
                                <span>{hostPermissionLabel(pattern)}</span>
                                <button
                                  type="button"
                                  role="switch"
                                  aria-label={(granted ? "Revoke " : "Allow ") +
                                    hostPermissionLabel(pattern)}
                                  aria-checked={granted}
                                  disabled={mutation !== null}
                                  class="relative h-[18px] w-8 shrink-0 rounded-full bg-border-strong transition-colors disabled:opacity-45"
                                  class:bg-accent={granted}
                                  onclick={() => editOptionalGrant(entry, "host", index, !granted)}
                                >
                                  <span
                                    aria-hidden="true"
                                    class="absolute top-[2px] left-[2px] h-3.5 w-3.5 rounded-full bg-white shadow-sm transition-transform"
                                    class:translate-x-3.5={granted}
                                  ></span>
                                </button>
                              </li>
                            {/each}
                          </ul>
                        {/if}
                        {#if entry.grants.api_permissions.length === 0 && entry.grants.host_permissions.length === 0 && entry.optional_api.length === 0 && entry.optional_hosts.length === 0}
                          <p class="text-muted">No API or site access is granted.</p>
                        {/if}
                        {#if entry.grants.file_access || entry.grants.private_access}
                          <p class="mt-2 text-muted">
                            {entry.grants.file_access ? "File URL access" : ""}
                            {entry.grants.file_access && entry.grants.private_access ? " · " : ""}
                            {entry.grants.private_access ? "Private window access" : ""}
                          </p>
                        {/if}
                      </div>
                    {/if}

                    <div class="mt-1.5 flex min-h-7 items-center justify-end gap-1">
                      {#if confirming === entry.install_id}
                        <span class="mr-auto text-[10.5px] leading-4 text-muted"
                          >Remove extension?</span
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
                        {#if entry.grants.initialized}
                          <button
                            type="button"
                            aria-expanded={expandedPermissions === entry.install_id}
                            aria-controls={`extension-permissions-${entry.install_id}`}
                            class="hover:bg-fill-strong mr-auto h-7 rounded-md px-2 text-[11px] font-medium text-muted hover:text-text"
                            onclick={() =>
                              (expandedPermissions =
                                expandedPermissions === entry.install_id ? null : entry.install_id)}
                          >
                            Permissions
                          </button>
                        {/if}
                        {#if entry.has_options_page && entry.runtime === "active"}
                          <button
                            type="button"
                            class="hover:bg-fill-strong h-7 rounded-md px-2 text-[11px] font-medium text-muted hover:text-text"
                            disabled={mutation !== null}
                            onclick={() => openOptions(entry)}
                          >
                            Settings
                          </button>
                        {/if}
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
          {:else}
            {#if management.candidates.length === 0}
              <p class="rounded-md bg-fill px-2.5 py-3 text-[11.5px] leading-4 text-muted">
                {management.entries.some((entry) => entry.source === "zephium_verified")
                  ? "All verified extensions in this catalog are installed."
                  : "No compatible extensions are available in this catalog."}
              </p>
            {:else}
              <div>
                <div class="mb-3">
                  <h3 class="text-[13px] leading-4 font-medium text-text">
                    {candidateSectionIsVerified() ? "Zephium Verified" : "Compatibility candidates"}
                  </h3>
                  <p class="mt-1 text-[10.5px] leading-4 text-muted">
                    {candidateSectionIsVerified()
                      ? "Exact packages reviewed for this platform and catalog release."
                      : "Authenticated packages being evaluated for this platform."}
                  </p>
                </div>
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
                          <span
                            class="mt-1 inline-flex rounded-full bg-raised px-1.5 py-0.5 text-[9.5px] leading-3 font-medium text-muted"
                            class:text-accent={candidate.source === "zephium_verified"}
                            class:text-warning={candidate.source === "developer_local"}
                          >
                            {sourceLabel(candidate.source, candidate.verified_catalog_unix)}
                          </span>
                          {#if candidate.provenance !== null}
                            <p class="mt-1 text-[10.5px] leading-4 text-muted">
                              {candidate.provenance.attribution} ·
                              {candidate.provenance.license_expression}
                            </p>
                            <p
                              class="truncate text-[10px] leading-4 text-faint"
                              title={candidate.provenance.source_url}
                            >
                              Source: {extensionProvenanceHost(candidate.provenance) ??
                                "Authenticated catalog"}
                            </p>
                          {/if}
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
                              class:cursor-not-allowed={!candidate.file_access_available ||
                                !fileScopeSelected(candidate)}
                              class:opacity-55={!candidate.file_access_available ||
                                !fileScopeSelected(candidate)}
                            >
                              <input
                                type="checkbox"
                                class="mt-0.5 accent-accent"
                                bind:checked={allowFileAccess}
                                disabled={mutation !== null ||
                                  !candidate.file_access_available ||
                                  !fileScopeSelected(candidate)}
                              />
                              <span>
                                {candidate.file_access_available
                                  ? "Allow access to local file URLs"
                                  : "Local file URL access is unavailable on this platform"}
                              </span>
                            </label>
                          {/if}
                          <label
                            class="mt-1.5 flex cursor-pointer items-start gap-2 text-[10.5px] leading-4 text-muted"
                            class:cursor-not-allowed={!candidate.private_access_available}
                            class:opacity-55={!candidate.private_access_available}
                          >
                            <input
                              type="checkbox"
                              class="mt-0.5 accent-accent"
                              bind:checked={allowPrivateAccess}
                              disabled={mutation !== null || !candidate.private_access_available}
                            />
                            <span>
                              {candidate.private_access_available
                                ? "Allow in private windows"
                                : "Private-window access is unavailable on this platform"}
                            </span>
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
        </div>
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
