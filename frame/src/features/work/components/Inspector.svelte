<script lang="ts">
  import type { WorkEnvironmentElement, WorkArea } from "$shared/ipc/bindings";
  import Icon from "$shared/ui/Icon";
  import Button from "$shared/ui/Button";
  import { Cancel01Icon } from "../lib/icons";
  import type { CanvasItem } from "../lib/canvas-model";
  import * as m from "$shared/i18n/messages";
  let {
    element,
    item,
    areas,
    busy = false,
    openLabel,
    openDisabled = false,
    onopen,
    onarea,
    onremove,
    oncontinue,
    onclose,
  }: {
    element: WorkEnvironmentElement;
    item: CanvasItem;
    areas: readonly WorkArea[];
    busy?: boolean;
    openLabel: string;
    openDisabled?: boolean;
    onopen: () => void;
    onarea: (area: string | null) => void;
    onremove: () => void;
    oncontinue?: () => void;
    onclose: () => void;
  } = $props();
  const id = $props.id();
  const provenance = $derived.by((): [string, string][] => {
    const reference = element.reference;
    switch (reference.kind) {
      case "browser":
        return [[m.work_env_tabs(), reference.tab]];
      case "resource":
        return [[m.work_env_notes(), reference.resource]];
      case "objective":
        return [[m.work_env_objective(), reference.objective]];
      case "artifact":
      case "subject":
      case "finding":
        return [
          [m.work_env_objective(), reference.objective],
          [m.work_env_results(), reference.artifact],
        ];
    }
  });
</script>

<section class="inspector" aria-label={m.work_env_inspector()}>
  <header>
    <span class="kind">{item.kind}</span>
    <button type="button" class="close" aria-label={m.work_env_close()} onclick={onclose}>
      <Icon icon={Cancel01Icon} size={14} />
    </button>
  </header>
  <h2>{item.title}</h2>
  {#if item.status}<p class="status">{item.status}</p>{/if}
  <div class="actions">
    <Button size="compact" variant="primary" disabled={busy || openDisabled} onclick={onopen}
      >{openLabel}</Button
    >
    {#if oncontinue}<Button size="compact" disabled={busy} onclick={oncontinue}
        >{m.work_env_continue_work()}</Button
      >{/if}
  </div>
  <dl>
    <dt><label for={`${id}-area`}>{m.work_env_area()}</label></dt>
    <dd>
      <select
        id={`${id}-area`}
        disabled={busy}
        value={element.area ?? ""}
        onchange={(event) => onarea(event.currentTarget.value || null)}
      >
        <option value="">{m.work_env_no_area()}</option>
        {#each areas as area (area.id)}<option value={area.id}>{area.title}</option>{/each}
      </select>
    </dd>
    <dt>{m.work_env_provenance()}</dt>
    {#each provenance as [label, value] (label + value)}<dd class="mono">
        <span>{label}</span><code>{value}</code>
      </dd>{/each}
  </dl>
  <footer>
    <Button size="compact" variant="ghost" disabled={busy} onclick={onremove}
      >{m.work_env_remove_reference()}</Button
    >
  </footer>
</section>

<style>
  .inspector {
    display: flex;
    flex-direction: column;
    gap: 12px;
    min-block-size: 0;
  }

  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }

  .kind,
  .status,
  dt {
    color: var(--color-faint);
    font-size: var(--text-caption);
  }

  .close {
    display: grid;
    place-items: center;
    inline-size: 26px;
    block-size: 26px;
    border: 0;
    border-radius: 50%;
    background: var(--color-fill);
    color: var(--color-muted);
    cursor: default;
  }

  .close:hover {
    background: var(--color-fill-hover);
    color: var(--color-text);
  }

  h2 {
    margin: 0;
    font-size: 15px;
    font-weight: 600;
    line-height: 20px;
    letter-spacing: -0.01em;
    overflow-wrap: anywhere;
  }

  .status {
    margin: 0;
  }

  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }

  dl {
    display: flex;
    flex-direction: column;
    gap: 6px;
    margin: 0;
  }

  dd {
    margin: 0;
  }

  dt {
    margin-block-start: 6px;
  }

  select {
    box-sizing: border-box;
    inline-size: 100%;
    block-size: 30px;
    padding: 0 8px;
    border: 0;
    border-radius: var(--radius-control-compact);
    background: var(--color-field);
    color: var(--color-text);
    font: inherit;
  }

  .mono {
    display: flex;
    justify-content: space-between;
    gap: 8px;
    font-size: var(--text-caption);
    color: var(--color-muted);
  }

  code {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-family: var(--font-mono);
    font-size: 10.5px;
  }

  footer {
    margin-block-start: auto;
    padding-block-start: 8px;
    border-block-start: 1px solid var(--color-border);
  }
</style>
