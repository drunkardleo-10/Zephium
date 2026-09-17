<script lang="ts">
  import type { WorkSession } from "$domain/work";
  import { preferences } from "$domain/preferences";
  import { preparationFailure } from "../lib/preparation-failure";
  import Button from "$shared/ui/Button";
  import ContextManifestList from "./composer/ContextManifestList.svelte";
  import * as m from "$shared/i18n/messages";
  let { session }: { session: WorkSession } = $props();
  const work = $derived(session.projection?.work);
  const operation = $derived(
    work ? session.operations.latest(work.id, ["prepare_plan", "prepare_account"]) : undefined,
  );
  const executionOperation = $derived(
    work ? session.operations.latest(work.id, "start") : undefined,
  );
  const executionFailure = $derived(
    executionOperation?.state.kind === "refused"
      ? executionOperation.state.error
      : executionOperation?.state.kind === "settled" &&
          executionOperation.state.response.reply.kind === "error"
        ? executionOperation.state.response.reply.error
        : null,
  );
  const planning = $derived(operation?.state.kind === "planned" ? operation.state.response : null);
  const reply = $derived(
    planning?.outcome.kind === "settled"
      ? planning.outcome.response.reply
      : operation?.state.kind === "settled"
        ? operation.state.response.reply
        : null,
  );
  const approval = $derived(reply?.kind === "approval_draft" ? reply : null);
  const account = $derived(
    approval?.spec.nodes
      .map((node) => node.capability)
      .find(
        (capability) => capability.kind === "account_read" || capability.kind === "account_update",
      ) ?? null,
  );
  let attested = $state(false);
  $effect(() => {
    void approval;
    attested = false;
  });
  const executed = $derived(
    approval &&
      session.projection?.executions.some(
        (execution) => JSON.stringify(execution.spec) === JSON.stringify(approval.spec),
      ),
  );
  const current = $derived(
    approval &&
      approval.work === work?.id &&
      approval.expected_revision === work.revision &&
      approval.spec.plan_revision === work.plan?.revision,
  );
  const unsettled = $derived(
    session.projection?.executions.find((item) =>
      ["approved", "running", "cancel_requested"].includes(item.status),
    ),
  );
  const blocked = $derived(
    preferences.value("ai.enabled") === "false" ||
      !work ||
      !!session.pending ||
      session.hasDrafts ||
      session.operations.busy(work.id) ||
      !["ready", "rejected"].includes(session.delivery),
  );
  const start = $derived(
    unsettled?.status === "approved" &&
      unsettled.authorization !== "user_directed_public_read" &&
      !session.projection?.interrupted.includes(unsettled.id)
      ? unsettled
      : null,
  );
  const failure = $derived(preparationFailure(operation?.state));
  const dollars = (micro: number) =>
    new Intl.NumberFormat("en", { style: "currency", currency: "USD" }).format(micro / 1_000_000);
</script>

{#if work}
  <section class="execution-review" aria-label={m.work_execution_review()}>
    {#if executionOperation?.state.kind === "pending"}<p role="status">
        {m.work_operation_pending()}
      </p>{/if}
    {#if executionOperation?.state.kind === "unknown"}<p role="status">
        {m.work_operation_unknown()}
      </p>{/if}
    {#if executionFailure}<p role="alert">
        {m.work_request_failed()}
      </p>{/if}
    {#if work.status === "plan_ready" && !unsettled}
      <Button
        disabled={blocked}
        onclick={() => {
          if (work)
            void session.operations.begin({
              kind: "prepare_plan",
              request: { version: 1, work: work.id, expected_revision: work.revision },
            });
        }}>{m.work_prepare_execution()}</Button
      >
      <p class="hint">{m.work_prepare_disclosure()}</p>
    {/if}
    {#if operation?.state.kind === "pending"}<p role="status">
        {m.work_preparing_execution()}
      </p>{/if}
    {#if operation?.state.kind === "unknown"}<p role="status">{m.work_operation_unknown()}</p>{/if}
    {#if failure}<p role="alert">{m.work_request_failed()}</p>{/if}
    {#if approval && !current && !unsettled && !executed}<p role="status">
        {m.work_approval_stale()}
      </p>{/if}
    {#if approval && current && !unsettled}
      <details class="approval" open>
        <summary>{m.work_env_review_scope()}</summary>
        <h2>{m.work_execution_review()}</h2>
        <p>{m.work_exact_approval_explanation()}</p>
        {#if approval.spec.context}<div class="context">
            <h3>{m.work_context_disclosed()}</h3>
            <ContextManifestList disclosure={approval.spec.context} />
          </div>{/if}
        <dl class="limits">
          <div>
            <dt>{m.work_cost_limit()}</dt>
            <dd>{dollars(approval.spec.limits.cost_micro_usd)}</dd>
          </div>
          <div>
            <dt>{m.work_token_limit()}</dt>
            <dd>{approval.spec.limits.model_tokens.toLocaleString()}</dd>
          </div>
          <div>
            <dt>{m.work_operation_limit()}</dt>
            <dd>{approval.spec.limits.operations}</dd>
          </div>
          <div>
            <dt>{m.work_time_limit()}</dt>
            <dd>{m.work_minutes({ minutes: approval.spec.limits.timeout_seconds / 60 })}</dd>
          </div>
          <div>
            <dt>{m.work_worker_limit()}</dt>
            <dd>{approval.spec.limits.max_workers}</dd>
          </div>
        </dl>
        <ol>
          {#each approval.spec.nodes as node (node.node)}
            {@const plan = work.plan?.draft.nodes.find((entry) => entry.id === node.node)}
            <li>
              <strong>{plan?.objective ?? m.work_plan_node()}</strong>
              <p>
                {node.capability.kind === "public_search"
                  ? m.work_capability_provider_search()
                  : node.capability.kind === "synthesize"
                    ? m.work_capability_synthesis()
                    : node.capability.kind === "account_read"
                      ? m.work_capability_account_read()
                      : node.capability.kind === "account_update"
                        ? m.work_capability_account_update()
                        : node.capability.kind === "coordinate" ||
                            node.capability.kind === "coordinate_public_discovery" ||
                            node.capability.kind === "coordinate_public_research"
                          ? m.work_capability_coordination()
                          : m.work_capability_discovery()}
              </p>
              {#if node.capability.kind === "account_read" || node.capability.kind === "account_update"}<dl
                  class="account"
                >
                  <div>
                    <dt>{m.work_account_origin()}</dt>
                    <dd>{node.capability.scope.origin}</dd>
                  </div>
                  <div>
                    <dt>{m.work_account_page()}</dt>
                    <dd>{node.capability.scope.url}</dd>
                  </div>
                  <div>
                    <dt>{m.work_account_effect()}</dt>
                    <dd>
                      {node.capability.kind === "account_read"
                        ? m.work_account_effect_read_label()
                        : m.work_account_effect_update_label()}
                    </dd>
                  </div>
                </dl>
                {#if node.capability.kind === "account_update"}<blockquote>
                    {m.work_account_update_summary({
                      field: node.capability.update.field ?? m.work_account_update_any_field(),
                      from: node.capability.update.from,
                      to: node.capability.update.to,
                    })}
                  </blockquote>{/if}
                <p>{m.work_account_disclosure()}</p>{/if}
              {#if node.capability.kind === "public_search"}<p>
                  {m.work_provider_search_scope({
                    provider: "OpenAI",
                    model: node.capability.scope.model,
                  })}
                </p>
                <p>{m.work_provider_search_disclosure()}</p>
                <blockquote>{node.capability.scope.query}</blockquote>
              {:else if node.capability.kind === "coordinate_public_research"}<p>
                  {m.work_provider_research_scope({
                    provider: "OpenAI",
                    model: node.capability.model,
                  })}
                </p>
                <p>{m.work_provider_browser_child_limit({ hops: node.capability.max_hops })}</p>
              {:else if node.capability.kind === "public_discovery"}<p>
                  {m.work_browser_isolation()}
                </p>
                <p>{m.work_search_disclosure()}</p>
                <blockquote>{node.capability.scope.search_query}</blockquote>
                <p>{m.work_navigation_limit({ hops: node.capability.scope.max_hops })}</p>
              {:else if node.capability.kind === "public_browse" || node.capability.kind === "coordinate"}<p
                >
                  {node.capability.scope.start_url}
                </p>{/if}
              {#if node.parent}<p>
                  {m.work_responsible_to({
                    objective:
                      work.plan?.draft.nodes.find((entry) => entry.id === node.parent)?.objective ??
                      m.work_primary_agent(),
                  })}
                </p>{/if}
              <ul>
                {#each plan?.outputs ?? [] as output (output.name)}<li>
                    {output.name}: {output.description}
                  </li>{/each}
              </ul>
            </li>
          {/each}
        </ol>
        {#if account}<label class="attest"
            ><input
              type="checkbox"
              bind:checked={attested}
              disabled={blocked}
            />{m.work_account_attest()}</label
          >{/if}
        <Button
          disabled={blocked || !current || (!!account && !attested)}
          onclick={() => {
            if (approval && current)
              void session.execute(
                { kind: "approve", spec: approval.spec },
                approval.expected_revision,
              );
          }}>{m.work_approve_plan()}</Button
        >
      </details>
    {/if}
    {#if unsettled?.status === "approved" && unsettled.authorization === "user_directed_public_read" && !session.projection?.interrupted.includes(unsettled.id)}<p
        role="status"
      >
        {m.work_env_public_unstarted()}
      </p>{/if}
    {#if start}<div class="approved">
        <p>{m.work_approved_ready()}</p>
        <Button
          disabled={blocked}
          onclick={() => {
            if (start && work)
              void session.operations.begin({
                kind: "start",
                request: {
                  version: 1,
                  work: work.id,
                  expected_revision: work.revision,
                  execution: start.id,
                },
              });
          }}>{m.work_start_execution()}</Button
        >
      </div>{/if}
  </section>
{/if}

<style>
  .execution-review {
    padding: 0 24px 16px;
    max-block-size: 50%;
    overflow: auto;
    flex-shrink: 0;
  }

  summary {
    cursor: pointer;
    font-weight: 550;
  }

  summary:focus-visible {
    outline: 2px solid var(--color-ring);
    outline-offset: 2px;
  }

  .hint {
    color: var(--color-muted);
    font-size: var(--text-caption);
  }

  .approval,
  .approved {
    border: 1px solid var(--color-border);
    border-radius: var(--radius-control);
    background: var(--color-fill);
    padding: 16px;
    margin-block-start: 16px;
  }

  h2 {
    font-size: var(--text-body);
    font-weight: 600;
    margin-block-start: 0;
  }

  p,
  li,
  dt,
  dd {
    font-size: var(--text-caption);
    overflow-wrap: anywhere;
  }

  .limits {
    display: flex;
    gap: 24px;
    flex-wrap: wrap;
  }

  dt {
    color: var(--color-muted);
  }

  dd {
    margin: 4px 0 0;
    font-weight: 600;
  }

  li {
    margin-block: 8px;
  }

  .account {
    display: grid;
    gap: 6px;
    margin: 8px 0;
  }

  .account dd {
    margin: 2px 0 0;
    font-weight: 500;
  }

  .attest {
    display: flex;
    align-items: flex-start;
    gap: 8px;
    margin-block: 12px;
    font-size: var(--text-caption);
  }

  .attest input {
    margin-block-start: 2px;
  }

  blockquote {
    margin: 8px 0;
    padding: 8px 16px;
    border-inline-start: 2px solid var(--color-border);
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    font-size: var(--text-caption);
  }
</style>
