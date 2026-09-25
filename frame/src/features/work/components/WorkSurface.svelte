<script lang="ts">
  import { SvelteMap } from "svelte/reactivity";
  import { untrack, type Snippet } from "svelte";
  import { observe } from "$shared/lib/observe";
  import Button from "$shared/ui/Button";
  import Artifact, { type EvidenceReference } from "$shared/ui/data/Artifact";
  import LazyView from "$shared/ui/LazyView";
  import {
    surfaceRenderable,
    type WorkSurfaceView,
    type WorkSurfaceIntent,
    type WorkRequestView,
  } from "../lib/work-surface";
  import type { CanvasView } from "../lib/canvas-model";
  import * as m from "$shared/i18n/messages";
  let {
    view,
    active = true,
    request,
    initialView,
    onintent,
    onrefresh,
    onviewchange,
    onevidence,
    resources,
  }: {
    resources?: Snippet;
    view: WorkSurfaceView;
    active?: boolean;
    request: WorkRequestView;
    initialView?: CanvasView;
    onintent: (intent: WorkSurfaceIntent) => void | Promise<void>;
    onrefresh?: () => void | Promise<void>;
    onviewchange?: (view: CanvasView) => void;
    onevidence?: (reference: EvidenceReference) => void;
  } = $props();
  // The host keys this component by Work/profile identity. Drafts stay local and
  // must be handed to its draft owner before navigation once persistence is wired.
  let objective = $state(untrack(() => view.objective));
  const answers = new SvelteMap<string, string>();
  let selected = $state<string | null>(null);
  let inspectorFocus = $state<HTMLButtonElement>();
  let returnFocus: HTMLElement | null = null;
  let focusedSelection: string | null = null;
  $effect(() => {
    const selection = selected;
    if (selection && inspectorFocus && selection !== focusedSelection) {
      focusedSelection = selection;
      inspectorFocus.focus();
    }
    if (!selection) focusedSelection = null;
  });
  function inspect(id: string) {
    returnFocus = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    selected = id;
  }
  function closeInspector() {
    selected = null;
    if (returnFocus?.isConnected) returnFocus.focus();
  }
  let action = $state.raw<{ key: string; basis: WorkSurfaceView } | null>(null);
  let submitting = $state(false);
  let observationLifetime = new AbortController();
  $effect(() => {
    if (active && observationLifetime.signal.aborted) observationLifetime = new AbortController();
    const lifetime = observationLifetime;
    if (!active) lifetime.abort();
    return () => lifetime.abort();
  });
  let deliveryFailure = $state.raw<WorkRequestView | null>(null);
  const instanceId = $props.id();
  let spatial = $state(true);
  let arrangement = $state<CanvasView | undefined>(untrack(() => initialView));
  const loadCanvas = () => import("./WorkCanvas.svelte");
  let valid = $derived(surfaceRenderable(view));
  let blocked = $derived(
    submitting || deliveryFailure === request || !["ready", "rejected"].includes(request.state),
  );
  let selectedItem = $derived(view.items.find((item) => item.id === selected));
  let artifact = $derived(view.artifacts.find((item) => item.key === selectedItem?.artifactKey));
  let selectedAction = $derived(
    action?.basis === view ? view.actions.find((item) => item.key === action?.key) : undefined,
  );
  async function dispatch(callback: () => void | Promise<void>) {
    const basis = request;
    submitting = true;
    try {
      const result = await observe(Promise.resolve(callback()), 10_000, observationLifetime.signal);
      if (result.state !== "received") deliveryFailure = basis;
    } catch {
      deliveryFailure = basis;
    } finally {
      submitting = false;
    }
  }
  function submit(intent: WorkSurfaceIntent) {
    if (!blocked) {
      action = null;
      void dispatch(() => onintent(intent));
    }
  }
  function refresh() {
    if (!submitting && onrefresh) void dispatch(onrefresh);
  }
  function saveView(next: CanvasView) {
    arrangement = next;
    onviewchange?.(next);
  }
</script>

{#if active}
  {#if !valid}<p role="alert">{m.work_artifact_unavailable()}</p>
  {:else}
    <section class="work-surface" aria-label={view.title}>
      <header class="work-header">
        <div>
          <p class="eyebrow">{m.work_workspace()}</p>
          <h1>{view.title}</h1>
        </div>
        <span class="phase">{view.phase}</span>
      </header>
      {#if view.notice}<aside class="notice" role="status">
          <strong>{view.notice.title}</strong>
          <p>{view.notice.detail}</p>
        </aside>{/if}
      <div class="authoring">
        <form
          onsubmit={(event) => {
            event.preventDefault();
            if (objective.trim()) submit({ kind: "objective", text: objective.trim() });
          }}
        >
          <label for={`${instanceId}-objective`}>{m.work_objective()}</label>
          <div class="objective-row">
            <textarea
              id={`${instanceId}-objective`}
              bind:value={objective}
              maxlength={16384}
              rows="2"
              disabled={blocked}></textarea><Button
              type="submit"
              disabled={blocked || !objective.trim() || objective === view.objective}
              >{m.work_submit_objective()}</Button
            >
          </div>
        </form>
        {#each view.questions as question (question.key)}<form
            class="question"
            onsubmit={(event) => {
              event.preventDefault();
              const text = answers.get(question.key)?.trim();
              if (text) submit({ kind: "answer", question: question.key, text });
            }}
          >
            <label for={`${instanceId}-question-${question.key}`}>{question.prompt}</label>
            {#if question.options.length}<div class="options">
                {#each question.options as option, i (i)}<Button
                    size="compact"
                    disabled={blocked}
                    aria-pressed={answers.get(question.key) === option}
                    onclick={() => answers.set(question.key, option)}>{option}</Button
                  >{/each}
              </div>{/if}
            <div class="objective-row">
              <input
                id={`${instanceId}-question-${question.key}`}
                maxlength={4096}
                bind:value={
                  () => answers.get(question.key) ?? "", (value) => answers.set(question.key, value)
                }
                disabled={blocked}
              /><Button type="submit" disabled={blocked || !answers.get(question.key)?.trim()}
                >{m.work_submit_answer()}</Button
              >
            </div>
          </form>{/each}
      </div>
      <div class="request" role="status" aria-live="polite">
        <span>{deliveryFailure === request ? m.work_delivery_unknown() : request.message}</span
        >{#if (["conflict", "unknown"].includes(request.state) || deliveryFailure === request) && onrefresh}<Button
            size="compact"
            disabled={submitting}
            onclick={refresh}>{m.work_reconcile()}</Button
          >{/if}
      </div>
      <div class="workspace-toolbar">
        <h2>{m.work_plan_resources()}</h2>
        <div class="options">
          <Button size="compact" aria-pressed={!spatial} onclick={() => (spatial = false)}
            >{m.work_list_view()}</Button
          ><Button size="compact" aria-pressed={spatial} onclick={() => (spatial = true)}
            >{m.work_canvas_view()}</Button
          >
        </div>
      </div>
      <div class="work-resource-layout" class:has-resources={!!resources}>
        {#if resources}<aside class="work-resources">{@render resources()}</aside>{/if}
        <div class="workspace" class:inspecting={!!selectedItem}>
          <div class="workspace-main">
            {#if spatial}<LazyView
                loader={loadCanvas}
                loadingLabel={m.surface_loading()}
                failureLabel={m.surface_render_failed()}
                retryLabel={m.surface_retry()}
                >{#snippet children(Canvas)}<Canvas
                    items={view.items}
                    links={view.links}
                    initialView={arrangement}
                    oninspect={inspect}
                    onviewchange={saveView}
                  />{/snippet}</LazyView
              >
            {:else}<ul class="work-list">
                {#each view.items as item (item.id)}<li>
                    <button
                      type="button"
                      aria-pressed={selected === item.id}
                      onclick={() => inspect(item.id)}
                      ><span class="eyebrow">{item.kind}</span><strong>{item.title}</strong><span
                        >{item.detail}</span
                      ><small>{item.status}</small></button
                    >
                  </li>{:else}<li>{m.work_canvas_empty()}</li>{/each}
              </ul>{/if}
          </div>
          {#if selectedItem}<aside class="inspector" aria-label={m.work_inspector()}>
              <Button bind:ref={inspectorFocus} size="compact" onclick={closeInspector}
                >{m.work_close_inspector()}</Button
              >{#if artifact}<Artifact {artifact} {onevidence} />{:else}<h2>
                  {selectedItem.title}
                </h2>
                <p>{selectedItem.detail}</p>
                <p>{selectedItem.status}</p>{/if}
            </aside>{/if}
        </div>
      </div>
      {#if view.actions.length}<footer class="actions" aria-label={m.work_actions()}>
          {#each view.actions as item (item.key)}<div>
              <Button
                disabled={blocked || !!item.disabledReason}
                onclick={() => (action = { key: item.key, basis: view })}>{item.label}</Button
              >{#if item.disabledReason}<small>{item.disabledReason}</small>{/if}
            </div>{/each}
        </footer>{/if}
      {#if selectedAction}<section class="confirmation" aria-label={selectedAction.label}>
          <h2>{selectedAction.label}</h2>
          <p>{selectedAction.scope}</p>
          <p>{selectedAction.consequence}</p>
          <div class="options">
            <Button
              disabled={blocked || !!selectedAction.disabledReason}
              onclick={() => submit({ kind: "action", key: selectedAction.key })}
              >{m.work_confirm_intent()}</Button
            ><Button onclick={() => (action = null)}>{m.work_keep_reviewing()}</Button>
          </div>
        </section>{/if}
    </section>
  {/if}
{/if}

<style>
  .work-surface {
    min-height: 0;
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 20px;
    padding: 28px;
    box-sizing: border-box;
    overflow: auto;
    background: var(--color-canvas);
    color: var(--color-text);
  }

  .work-header,
  .workspace-toolbar,
  .objective-row,
  .options,
  .request {
    display: flex;
    align-items: center;
    gap: 12px;
  }

  .work-header,
  .workspace-toolbar {
    justify-content: space-between;
  }

  h1 {
    font-size: var(--text-title);
    margin: 4px 0 0;
    font-weight: 600;
    letter-spacing: -0.02em;
  }

  h2 {
    font-size: var(--text-body);
    margin: 0;
    font-weight: 600;
  }

  .eyebrow,
  .phase,
  small {
    font-size: var(--text-caption);
    color: var(--color-muted);
  }

  .eyebrow {
    margin: 0;
  }

  .phase {
    padding: 8px 12px;
    background: var(--color-fill);
    border-radius: var(--radius-capsule);
  }

  .notice,
  .confirmation {
    padding: 16px 20px;
    border: 1px solid var(--color-border-strong);
    border-radius: var(--radius-card);
    background: var(--color-surface);
  }

  p {
    line-height: 1.6;
    overflow-wrap: anywhere;
  }

  .notice p {
    margin-block: 6px 0;
    color: var(--color-muted);
  }

  .authoring {
    display: grid;
    gap: 16px;
  }

  label {
    display: block;
    margin-block-end: 8px;
    font-size: var(--text-caption);
    color: var(--color-muted);
  }

  .objective-row {
    align-items: center;
  }

  textarea,
  input {
    flex: 1;
    min-width: 0;
    resize: vertical;
    padding: 12px;
    font: inherit;
    line-height: 1.5;
    background: var(--color-field);
    color: var(--color-text);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-field);
  }

  .options,
  .actions {
    flex-wrap: wrap;
  }

  .question .options {
    margin-block-end: 8px;
  }

  .request {
    color: var(--color-muted);
    min-height: 24px;
    font-size: var(--text-caption);
  }

  .work-resource-layout {
    display: grid;
    min-height: 420px;
    flex: 1 0 420px;
  }

  .work-resource-layout.has-resources {
    grid-template-columns: 220px minmax(0, 1fr);
    gap: 16px;
  }

  .work-resources {
    border: 1px solid var(--color-border);
    border-radius: var(--radius-card);
    background: var(--color-surface);
    overflow: auto;
    max-height: 700px;
  }

  @media (width <= 900px) {
    .work-resource-layout.has-resources {
      grid-template-columns: minmax(0, 1fr);
    }
  }

  .workspace {
    display: grid;
    grid-template-columns: minmax(0, 1fr);
    min-height: 420px;
    flex: 1 0 420px;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-card);
    overflow: hidden;
  }

  .workspace.inspecting {
    grid-template-columns: minmax(320px, 1fr) minmax(300px, 440px);
  }

  .workspace-main {
    min-width: 0;
    min-height: 420px;
  }

  .inspector {
    border-inline-start: 1px solid var(--color-border);
    padding: 20px;
    background: var(--color-surface);
    overflow: auto;
    max-height: 700px;
  }

  .inspector > :global(button) {
    margin-block-end: 16px;
  }

  .actions {
    display: flex;
    gap: 12px;
  }

  .actions > div {
    display: grid;
    gap: 8px;
  }

  .work-list {
    list-style: none;
    padding: 16px;
    margin: 0;
    display: grid;
    gap: 8px;
  }

  .work-list button {
    display: grid;
    gap: 8px;
    width: 100%;
    padding: 16px;
    text-align: start;
    color: var(--color-text);
    background: var(--color-fill);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-control);
    font: inherit;
    cursor: pointer;
  }

  .work-list button[aria-pressed="true"] {
    border-color: var(--color-accent);
  }

  textarea:focus-visible,
  input:focus-visible,
  .work-list button:focus-visible {
    outline: 2px solid var(--color-ring);
    outline-offset: 2px;
  }

  @media (width <= 900px) {
    .workspace.inspecting {
      grid-template-columns: minmax(0, 1fr);
    }

    .inspector {
      border-inline-start: 0;
      border-block-start: 1px solid var(--color-border);
    }

    .work-header,
    .objective-row {
      align-items: stretch;
      flex-direction: column;
    }
  }
</style>
