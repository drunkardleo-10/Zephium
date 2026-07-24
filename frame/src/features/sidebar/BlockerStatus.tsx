import { Shield01Icon } from "@hugeicons/core-free-icons";
import { createSignal, onCleanup, onMount, Show } from "solid-js";
import type { BlockerStatusView } from "../../ipc/bindings";
import type { BlockerMutationResult } from "../../state/blocker";
import * as blocker from "../../state/blocker";
import {
  canRefreshBlockerSources,
  diagnosticLabel,
  hasExactInstalledSourcePolicy,
  protectionLabel,
} from "../../state/blocker-model";
import { Icon } from "../../ui/Icon";

function yesNo(value: boolean | null): string {
  if (value === null) return "—";
  return value ? "Yes" : "No";
}

function shortRevision(value: string | null): string {
  if (value === null) return "—";
  return value.slice(-8);
}

function formatBytes(value: number | null): string {
  if (value === null) return "—";
  if (value < 1024) return `${value} B`;
  if (value < 1024 * 1024) return `${(value / 1024).toFixed(1)} KiB`;
  return `${(value / (1024 * 1024)).toFixed(1)} MiB`;
}

function formatTimestamp(value: string | null): string {
  if (value === null) return "—";
  const seconds = Number(value);
  if (!Number.isSafeInteger(seconds) || seconds < 0) return "—";
  const date = new Date(seconds * 1000);
  return Number.isNaN(date.getTime()) ? "—" : date.toLocaleString();
}

function shortDigest(value: string | null | undefined): string {
  if (value === null || value === undefined) return "—";
  return value.slice(0, 8);
}

function freshness(value: boolean | null): string {
  if (value === null) return "—";
  return value ? "Stale" : "Fresh";
}

function sourceAuthority(value: BlockerStatusView["source_package_provenance"]): string {
  switch (value) {
    case "release_bundle":
      return "Bundled with Zephium";
    case "tuf_repository":
      return "Authenticated update service";
    case null:
      return "—";
  }
}

function mutationResultMessage(result: BlockerMutationResult, success: string): string {
  switch (result.state) {
    case "not_admitted":
      return "Request was not admitted.";
    case "unavailable":
      return "Request result is unavailable; the authoritative status is shown above.";
    case "pending":
      return `Still processing (operation ${result.operation_id.slice(-8)}); status will update above.`;
    case "unknown":
      return `Operation ${result.operation_id.slice(-8)} could not be reconciled; check the status above.`;
    case "processed":
      switch (result.disposition.outcome) {
        case "applied":
          return success;
        case "no_op":
          return "No change was needed.";
        case "rejected":
          return `Request rejected: ${diagnosticLabel(result.disposition.reason)}.`;
        case "native_admission_failed":
          return `Native request failed: ${diagnosticLabel(result.disposition.reason)}.`;
        case "deferred":
          return `Request is continuing: ${diagnosticLabel(result.disposition.reason)}.`;
      }
  }
}

function tone(status: BlockerStatusView): string {
  switch (status.protection) {
    case "active":
      return "bg-emerald-400";
    case "degraded":
      return "bg-amber-400";
    case "unavailable":
      return "bg-red-400";
    case "pending":
      return "bg-blue-400";
    case "disabled":
      return "bg-faint";
  }
}

export function BlockerStatus() {
  const [open, setOpen] = createSignal(false);
  const [submitting, setSubmitting] = createSignal(false);
  const [actionResult, setActionResult] = createSignal<string | null>(null);
  let trigger: HTMLButtonElement | undefined;
  const mutationDisabled = () =>
    submitting() ||
    blocker.status().preference !== "authoritative" ||
    blocker.status().desired_enabled === null;
  const toggleDisabled = () =>
    mutationDisabled() ||
    (!blocker.status().desired_enabled && !hasExactInstalledSourcePolicy(blocker.status()));
  const sourceRefreshDisabled = () => submitting() || !canRefreshBlockerSources(blocker.status());

  async function setEnabled(enabled: boolean) {
    if (mutationDisabled()) return;
    setSubmitting(true);
    setActionResult("Waiting for exact settlement…");
    try {
      const result = await blocker.setEnabled(enabled);
      setActionResult(
        mutationResultMessage(result, enabled ? "Protection enabled." : "Protection disabled."),
      );
    } finally {
      setSubmitting(false);
    }
  }

  async function retry() {
    const generation = blocker.status().desired_generation;
    if (mutationDisabled() || !blocker.status().retryable || generation === null) return;
    setSubmitting(true);
    setActionResult("Waiting for exact settlement…");
    try {
      const result = await blocker.retry(generation);
      setActionResult(mutationResultMessage(result, "Retry completed."));
    } finally {
      setSubmitting(false);
    }
  }

  async function refreshSources() {
    if (sourceRefreshDisabled()) return;
    setSubmitting(true);
    setActionResult("Waiting for exact settlement…");
    try {
      const result = await blocker.refreshSources();
      setActionResult(mutationResultMessage(result, "Filter lists refreshed."));
    } finally {
      setSubmitting(false);
    }
  }

  onMount(() => {
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key !== "Escape" || !open()) return;
      event.preventDefault();
      setOpen(false);
      trigger?.focus();
    };
    document.addEventListener("keydown", closeOnEscape);
    onCleanup(() => document.removeEventListener("keydown", closeOnEscape));
  });

  return (
    <div class="relative px-2.5 pb-1">
      <button
        ref={(element) => {
          trigger = element;
        }}
        type="button"
        aria-expanded={open()}
        aria-controls="blocker-diagnostics"
        data-zephium-blocker-protection={blocker.status().protection}
        data-zephium-blocker-phase={blocker.status().phase}
        data-zephium-blocker-source-phase={blocker.status().source_phase}
        data-zephium-blocker-revision={blocker.status().projection_revision}
        onClick={() => {
          setOpen((value) => !value);
          void blocker.refresh();
        }}
        class="flex h-8 w-full items-center gap-2 rounded-md px-3 text-[11.5px] text-muted hover:bg-hover hover:text-text"
      >
        <Icon icon={Shield01Icon} size={14} />
        <span
          aria-hidden="true"
          class={`h-1.5 w-1.5 shrink-0 rounded-full ${tone(blocker.status())}`}
        />
        <span aria-live="polite" class="min-w-0 flex-1 truncate text-left">
          {protectionLabel(blocker.status().protection)}
        </span>
      </button>

      <Show when={open()}>
        <section
          id="blocker-diagnostics"
          aria-label="Content blocker diagnostics"
          aria-busy={submitting()}
          class="absolute bottom-full left-2.5 right-2.5 z-30 mb-1 max-h-[70vh] overflow-y-auto rounded-lg border border-white/8 bg-elevated p-3 text-[11px] text-muted shadow-xl"
        >
          <div class="mb-2 flex items-center gap-2 text-[12px] font-medium text-text">
            <Icon icon={Shield01Icon} size={14} />
            Content blocker
          </div>
          <dl class="grid grid-cols-[auto_1fr] gap-x-3 gap-y-1">
            <Detail label="Protection" value={protectionLabel(blocker.status().protection)} />
            <Detail label="Phase" value={diagnosticLabel(blocker.status().phase)} />
            <Detail label="Preference" value={diagnosticLabel(blocker.status().preference)} />
            <Detail label="Desired" value={yesNo(blocker.status().desired_enabled)} />
            <Detail label="Applied" value={yesNo(blocker.status().applied_enabled)} />
            <Detail label="Config" value={shortRevision(blocker.status().config_revision)} mono />
            <Detail
              label="Generation"
              value={shortRevision(blocker.status().retained_generation)}
              mono
            />
            <Show when={blocker.status().failure}>
              {(failure) => <Detail label="Failure" value={diagnosticLabel(failure())} />}
            </Show>
            <Show when={blocker.status().retryable}>
              <Detail label="Retry" value={`${blocker.status().retries_remaining} remaining`} />
            </Show>
          </dl>

          <Show when={blocker.status().applied_coverage}>
            {(coverage) => (
              <div class="mt-2 border-t border-white/8 pt-2">
                <div class="mb-1 text-faint">Exact applied coverage</div>
                <dl class="grid grid-cols-[auto_1fr] gap-x-3 gap-y-1">
                  <Detail
                    label="Accepted"
                    value={`${coverage().accepted_rules} / ${coverage().source_rules}`}
                    mono
                  />
                  <Detail
                    label="Blocking entries"
                    value={`${coverage().blocking_rule_entries}`}
                    mono
                  />
                  <Detail label="Rejected" value={`${coverage().rejected_rules}`} mono />
                  <Detail label="Omitted" value={`${coverage().platform_omitted_rules}`} mono />
                  <Detail
                    label="Approximated"
                    value={`${coverage().platform_approximated_rules}`}
                    mono
                  />
                </dl>
              </div>
            )}
          </Show>
          <div class="mt-2 border-t border-white/8 pt-2">
            <div class="mb-1 text-faint">Filter sources</div>
            <dl class="grid grid-cols-[auto_1fr] gap-x-3 gap-y-1">
              <Detail label="State" value={diagnosticLabel(blocker.status().source_phase)} />
              <Detail
                label="Authority"
                value={sourceAuthority(blocker.status().source_package_provenance)}
              />
              <Show when={blocker.status().source_failure}>
                {(failure) => <Detail label="Failure" value={diagnosticLabel(failure())} />}
              </Show>
              <Show when={blocker.status().source_repair_retry_pending}>
                <Detail label="Candidate repair" value="Explicit retry required" />
              </Show>
              <Show when={blocker.status().source_material_repair_pending}>
                <Detail label="Source material" value="Authenticated repair in progress" />
              </Show>
              <Show when={blocker.status().source_material_repair_retry_pending}>
                <Detail label="Source material" value="Explicit refresh required" />
              </Show>
              <Detail
                label="Package"
                value={shortRevision(blocker.status().source_package_revision)}
                mono
              />
              <Detail
                label="Installed"
                value={shortRevision(blocker.status().source_installed_revision)}
                mono
              />
              <Show when={blocker.status().source_identities}>
                {(identities) => (
                  <>
                    <Detail
                      label="Candidate"
                      value={shortRevision(identities().candidate_revision)}
                      mono
                    />
                    <Detail
                      label="Current hash"
                      value={shortDigest(identities().package_manifest_sha256)}
                      mono
                    />
                    <Detail
                      label="Candidate hash"
                      value={shortDigest(identities().candidate_manifest_sha256)}
                      mono
                    />
                    <Detail
                      label="Installed hash"
                      value={shortDigest(identities().installed_manifest_sha256)}
                      mono
                    />
                  </>
                )}
              </Show>
              <Detail label="Freshness" value={freshness(blocker.status().source_package_stale)} />
              <Detail
                label="Lists"
                value={
                  blocker.status().source_count === null ? "—" : `${blocker.status().source_count}`
                }
                mono
              />
              <Detail label="Bytes" value={formatBytes(blocker.status().source_bytes)} mono />
              <Detail
                label="Expires"
                value={formatTimestamp(blocker.status().source_package_expires_unix)}
              />
              <Detail
                label="Last refresh"
                value={formatTimestamp(blocker.status().source_last_refresh_attempt_unix)}
              />
            </dl>
          </div>
          <div class="mt-2 flex gap-1.5 border-t border-white/8 pt-2">
            <button
              type="button"
              disabled={toggleDisabled()}
              onClick={() => void setEnabled(!blocker.status().desired_enabled)}
              class="h-7 flex-1 rounded-md border border-white/8 bg-white/4 px-2 text-[10.5px] text-text hover:bg-white/8 disabled:cursor-not-allowed disabled:opacity-40"
            >
              {blocker.status().desired_enabled ? "Disable" : "Enable"}
            </button>
            <Show when={blocker.status().retryable && blocker.status().desired_generation !== null}>
              <button
                type="button"
                disabled={mutationDisabled()}
                onClick={() => void retry()}
                class="h-7 rounded-md border border-white/8 bg-white/4 px-2.5 text-[10.5px] text-text hover:bg-white/8 disabled:cursor-not-allowed disabled:opacity-40"
              >
                Retry
              </button>
            </Show>
            <Show when={blocker.status().source_package_provenance === "tuf_repository"}>
              <button
                type="button"
                disabled={sourceRefreshDisabled()}
                onClick={() => void refreshSources()}
                class="h-7 rounded-md border border-white/8 bg-white/4 px-2.5 text-[10.5px] text-text hover:bg-white/8 disabled:cursor-not-allowed disabled:opacity-40"
              >
                {blocker.status().source_phase === "refreshing" ? "Refreshing…" : "Refresh lists"}
              </button>
            </Show>
          </div>
          <Show when={actionResult()}>
            {(message) => (
              <div aria-live="polite" class="mt-2 text-[10px] leading-4 text-muted">
                {message()}
              </div>
            )}
          </Show>
          <div class="mt-2 text-[10px] leading-4 text-faint">
            This diagnostics view collects no browsing requests or page URLs.
          </div>
        </section>
      </Show>
    </div>
  );
}

function Detail(props: { label: string; value: string; mono?: boolean }) {
  return (
    <>
      <dt class="text-faint">{props.label}</dt>
      <dd class="min-w-0 truncate text-right text-text" classList={{ "font-mono": props.mono }}>
        {props.value}
      </dd>
    </>
  );
}
