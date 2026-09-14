<script lang="ts">
  import type { WorkSession } from "$domain/work";
  import Button from "$shared/ui/Button";
  import { artifactFields } from "../lib/artifact-editor";
  import * as m from "$shared/i18n/messages";
  let {
    session,
    execution,
    artifact,
  }: { session: WorkSession; execution: string; artifact: string } = $props();
  const draft = $derived(session.artifactDraft(artifact));
  const fact = $derived(session.projection?.executions.find((item) => item.id === execution));
  const editable = $derived(fact && ["needs_review", "completed"].includes(fact.status));
  const blocked = $derived(!!session.pending || !["ready", "rejected"].includes(session.delivery));
  const fields = $derived(draft ? artifactFields(draft.data) : []);
  let page = $state(0);
  const first = $derived(Math.min(page, Math.max(0, Math.ceil(fields.length / 24) - 1)) * 24);
  const decision = $derived(
    fact?.user_artifacts?.find((item) => item.artifact === artifact)?.decision,
  );
  function review(decision: "accepted" | "rejected") {
    if (!blocked && !draft)
      void session.execute({ kind: "review_artifact", execution, artifact, decision });
  }
</script>

{#if editable}
  <section class="artifact-actions" aria-label={m.work_review_result()}>
    {#if draft}
      <p>{m.work_edit_preserves_original()}</p>
      <form
        onsubmit={(event) => {
          event.preventDefault();
          if (!blocked) void session.saveArtifact(artifact);
        }}
      >
        <fieldset disabled={blocked}>
          <legend>{m.work_edit_result()}</legend>
          {#each fields.slice(first, first + 24) as field, index (first + index)}
            <label>
              {#if typeof field.value === "boolean"}<input
                  type="checkbox"
                  checked={field.value}
                  onchange={(event) =>
                    session.editArtifact(
                      execution,
                      artifact,
                      field.change(event.currentTarget.checked),
                    )}
                />{field.label}
              {:else}{field.label}<textarea
                  rows="2"
                  maxlength={32768}
                  value={field.value}
                  oninput={(event) =>
                    session.editArtifact(
                      execution,
                      artifact,
                      field.change(event.currentTarget.value),
                    )}></textarea>{/if}
            </label>
          {/each}
        </fieldset>
        {#if fields.length > 24}<nav aria-label={m.work_edit_result()}>
            <Button
              size="compact"
              disabled={first === 0}
              onclick={() => (page = Math.max(0, page - 1))}>{m.work_previous()}</Button
            >
            <span
              >{m.work_table_range({
                first: first + 1,
                last: Math.min(first + 24, fields.length),
                total: fields.length,
              })}</span
            >
            <Button
              size="compact"
              disabled={first + 24 >= fields.length}
              onclick={() => (page += 1)}>{m.work_next()}</Button
            >
          </nav>{/if}
        <div class="controls">
          <Button type="submit" disabled={blocked}>{m.work_save_result()}</Button><Button
            disabled={!!session.pending}
            onclick={() => session.discardArtifact(artifact)}>{m.work_discard_drafts()}</Button
          >
        </div>
      </form>
    {:else}
      <p>{m.work_review_explanation()}</p>
      <div class="controls">
        <Button disabled={blocked} onclick={() => session.editArtifact(execution, artifact)}
          >{m.work_edit_result()}</Button
        >
        <Button disabled={blocked || decision === "accepted"} onclick={() => review("accepted")}
          >{m.work_accept_result()}</Button
        >
        <Button disabled={blocked || decision === "rejected"} onclick={() => review("rejected")}
          >{m.work_reject_result()}</Button
        >
      </div>
    {/if}
  </section>
{/if}

<style>
  .artifact-actions {
    margin-block-start: 24px;
    border-block-start: 1px solid var(--color-border);
    padding-block-start: 16px;
  }

  p,
  legend {
    font-size: var(--text-caption);
    color: var(--color-muted);
  }

  fieldset {
    display: grid;
    gap: 16px;
    margin: 0;
    padding: 0;
    border: 0;
  }

  label {
    display: block;
    font-size: var(--text-caption);
  }

  textarea {
    display: block;
    inline-size: 100%;
    box-sizing: border-box;
    margin-block-start: 8px;
    padding: 8px;
    background: var(--color-fill);
    color: var(--color-text);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-control);
    font: inherit;
    resize: vertical;
  }

  .controls,
  nav {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
    margin-block-start: 16px;
  }
</style>
