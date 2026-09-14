<script lang="ts">
  import type { WorkSession, WorkPlanProposal } from "$domain/work";
  import Button from "$shared/ui/Button";
  import * as m from "$shared/i18n/messages";
  let { session }: { session: WorkSession } = $props();
  const draft = $derived(session.planDraft);
  const blocked = $derived(!!session.pending || !["ready", "rejected"].includes(session.delivery));
  function edit(change: (proposal: WorkPlanProposal) => void) {
    if (!draft || blocked) return;
    const proposal = structuredClone(draft.proposal);
    change(proposal);
    session.editPlan(proposal);
  }
</script>

{#if draft}
  <section class="plan-editor" aria-label={m.work_edit_plan()}>
    <h2>{m.work_edit_plan()}</h2>
    {#each draft.proposal.nodes as node, index (node.key)}
      <fieldset disabled={blocked}>
        <legend>{m.work_plan_step({ number: index + 1 })}</legend>
        <label
          >{m.work_objective()}<textarea
            rows="3"
            maxlength={8192}
            value={node.objective}
            oninput={(event) =>
              edit((plan) => {
                const target = plan.nodes.find((item) => item.key === node.key);
                if (target) target.objective = event.currentTarget.value;
              })}></textarea></label
        >
        <div class="dependencies">
          <span>{m.work_dependencies()}</span>
          {#each draft.proposal.nodes.filter((other) => other.key !== node.key) as other (other.key)}
            <label
              ><input
                type="checkbox"
                checked={node.dependencies.includes(other.key)}
                onchange={(event) =>
                  edit((plan) => {
                    const target = plan.nodes.find((item) => item.key === node.key);
                    if (target)
                      target.dependencies = event.currentTarget.checked
                        ? [...node.dependencies, other.key]
                        : node.dependencies.filter((key) => key !== other.key);
                  })}
              />{other.objective.slice(0, 120)}</label
            >
          {/each}
        </div>
        {#each node.outputs as output, outputIndex (outputIndex)}
          <div class="output">
            <label
              >{m.work_output_name()}<input
                maxlength={128}
                value={output.name}
                oninput={(event) =>
                  edit((plan) => {
                    const target = plan.nodes.find((item) => item.key === node.key)?.outputs[
                      outputIndex
                    ];
                    if (target) target.name = event.currentTarget.value;
                  })}
              /></label
            >
            <label
              >{m.work_output_description()}<textarea
                rows="2"
                maxlength={2048}
                value={output.description}
                oninput={(event) =>
                  edit((plan) => {
                    const target = plan.nodes.find((item) => item.key === node.key)?.outputs[
                      outputIndex
                    ];
                    if (target) target.description = event.currentTarget.value;
                  })}></textarea></label
            >
            <span>{m.work_review_required()}</span>
          </div>
        {/each}
        <Button
          size="compact"
          disabled={draft.proposal.nodes.length < 2}
          onclick={() =>
            edit((plan) => {
              plan.nodes.splice(index, 1);
              for (const remaining of plan.nodes)
                remaining.dependencies = remaining.dependencies.filter((key) => key !== node.key);
            })}>{m.work_remove_step()}</Button
        >
      </fieldset>
    {/each}
    <div class="actions">
      <Button
        disabled={blocked || draft.proposal.nodes.length >= 64}
        onclick={() =>
          edit((plan) => {
            const key = Array.from({ length: 64 }, (_, key) => key).find(
              (key) => !plan.nodes.some((node) => node.key === key),
            );
            if (key !== undefined)
              plan.nodes.push({
                key,
                objective: "",
                dependencies: [],
                outputs: [
                  { name: "Result", description: "", review: "source_mapped_needs_review" },
                ],
              });
          })}>{m.work_add_step()}</Button
      >
      <Button
        disabled={blocked}
        onclick={() => {
          void session.savePlan();
        }}>{m.work_save_plan()}</Button
      >
    </div>
  </section>
{/if}

<style>
  .plan-editor {
    display: grid;
    gap: 16px;
    padding: 24px;
  }

  h2,
  legend {
    font-size: var(--text-body);
    font-weight: 600;
  }

  fieldset {
    display: grid;
    gap: 16px;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-lg);
    padding: 16px;
    min-inline-size: 0;
  }

  label {
    display: grid;
    gap: 8px;
    font-size: var(--text-caption);
  }

  textarea,
  input:not([type="checkbox"]) {
    inline-size: 100%;
    box-sizing: border-box;
    padding: 12px;
    font: inherit;
    background: var(--color-field);
    color: var(--color-text);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-field);
  }

  textarea {
    resize: vertical;
  }

  .dependencies,
  .output {
    display: grid;
    gap: 8px;
  }

  .dependencies label {
    display: flex;
    align-items: center;
  }

  .actions {
    display: flex;
    gap: 12px;
  }

  input:focus-visible,
  textarea:focus-visible {
    outline: 2px solid var(--color-ring);
    outline-offset: 2px;
  }
</style>
