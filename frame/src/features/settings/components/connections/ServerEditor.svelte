<script lang="ts">
  import { tick } from "svelte";
  import type { WorkServerDraftV1, WorkServerRowV1 } from "$shared/ipc/bindings";
  import Button from "$shared/ui/Button";
  import Field from "$shared/ui/Field";
  import Icon from "$shared/ui/Icon";
  import SegmentedControl from "$shared/ui/SegmentedControl";
  import Add01Icon from "@hugeicons/core-free-icons/Add01Icon";
  import Cancel01Icon from "@hugeicons/core-free-icons/Cancel01Icon";
  import SquareLock02Icon from "@hugeicons/core-free-icons/SquareLock02Icon";
  import { serviceKey, serviceMark } from "$domain/connections";
  import * as m from "$shared/i18n/messages";
  import {
    PRESETS,
    draftOf,
    emptyForm,
    formFault,
    formOf,
    secretByName,
    type FormFault,
    type ServerForm,
  } from "../../lib/connections";

  let {
    editing,
    taken,
    saving,
    onsave,
    oncancel,
  }: {
    /** The server being edited, or `null` to add one. */
    editing: WorkServerRowV1 | null;
    /** Ids other servers already use. */
    taken: readonly string[];
    saving: boolean;
    onsave: (draft: WorkServerDraftV1) => void;
    oncancel: () => void;
  } = $props();

  // The form starts from the row once; later list refreshes don't overwrite typing.
  // svelte-ignore state_referenced_locally
  let form = $state<ServerForm>(editing ? formOf(editing) : emptyForm());
  let tried = $state(false);
  let root = $state<HTMLElement>();
  const fault = $derived(formFault(form, editing));
  const shown = $derived<FormFault | null>(tried ? fault : null);

  const FAULTS: Record<FormFault, () => string> = {
    name: m.connections_fault_name,
    command: m.connections_fault_command,
    url: m.connections_fault_url,
    env: m.connections_fault_env,
    token: m.connections_fault_token,
  };

  $effect(() => {
    void tick().then(() => root?.querySelector("input")?.focus());
  });
  // Escape anywhere in the form cancels it; listening on the element keeps
  // the form itself non-interactive for assistive technology.
  $effect(() => {
    const form = root;
    if (!form) return;
    form.addEventListener("keydown", escape);
    return () => form.removeEventListener("keydown", escape);
  });

  function preset(index: number) {
    const chosen = PRESETS[index];
    if (!chosen) return;
    form = { ...form, name: chosen.name, kind: "http", url: chosen.url, auth: chosen.auth };
  }

  function addVariable() {
    form.env = [...form.env, { name: "", value: "", secret: true, held: false }];
    void tick().then(() => {
      const names = root?.querySelectorAll<HTMLInputElement>(".variable input");
      names?.[names.length - 2]?.focus();
    });
  }

  function save() {
    tried = true;
    if (fault) return;
    onsave(draftOf(form, editing, taken));
  }

  function escape(event: KeyboardEvent) {
    if (event.key !== "Escape") return;
    event.preventDefault();
    oncancel();
  }
</script>

<form
  class="editor"
  bind:this={root}
  onsubmit={(event) => {
    event.preventDefault();
    save();
  }}
>
  {#if !editing}
    <div class="presets" role="group" aria-label={m.connections_presets()}>
      {#each PRESETS as item, index (item.name)}<button
          type="button"
          class="preset"
          aria-pressed={form.kind === "http" && form.url === item.url}
          onclick={() => preset(index)}
          ><Icon
            icon={serviceMark(serviceKey(item.name))}
            size={13}
            strokeWidth={1.6}
          />{item.name}</button
        >{/each}
    </div>
  {/if}
  <Field
    label={m.connections_name()}
    maxlength={40}
    autocomplete="off"
    spellcheck="false"
    bind:value={form.name}
    error={shown === "name" ? FAULTS.name() : undefined}
  />
  <div class="kind">
    <span class="label">{m.connections_kind()}</span>
    <SegmentedControl
      label={m.connections_kind()}
      size="compact"
      options={[
        { value: "http", label: m.connections_kind_http() },
        { value: "stdio", label: m.connections_kind_stdio() },
      ]}
      bind:value={form.kind}
    />
  </div>
  {#if form.kind === "http"}
    <Field
      label={m.connections_url()}
      type="url"
      placeholder="https://mcp.example.com/mcp"
      autocomplete="off"
      spellcheck="false"
      bind:value={form.url}
      error={shown === "url" ? FAULTS.url() : undefined}
    />
    <div class="auth">
      <span class="label">{m.connections_auth()}</span>
      <SegmentedControl
        label={m.connections_auth()}
        size="compact"
        options={[
          { value: "oauth", label: m.connections_auth_oauth() },
          { value: "bearer", label: m.connections_auth_bearer() },
          { value: "none", label: m.connections_auth_none() },
        ]}
        bind:value={form.auth}
      />
    </div>
    {#if form.auth === "bearer"}
      <Field
        label={m.connections_token()}
        type="password"
        autocomplete="off"
        spellcheck="false"
        placeholder={editing?.secrets.includes("bearer") ? m.connections_token_held() : ""}
        bind:value={form.token}
        error={shown === "token" ? FAULTS.token() : undefined}
      />
    {/if}
  {:else}
    <Field
      class="command"
      label={m.connections_command()}
      placeholder="npx -y @modelcontextprotocol/server-filesystem ~/Projects"
      autocomplete="off"
      spellcheck="false"
      bind:value={form.command}
      hint={shown === "command" ? undefined : m.connections_command_hint()}
      error={shown === "command" ? FAULTS.command() : undefined}
    />
    <div class="env">
      <span class="label">{m.connections_env()}</span>
      {#each form.env as variable, index (index)}
        <div class="variable">
          <Field
            label={m.connections_env_name()}
            labelHidden
            placeholder={m.connections_env_name()}
            autocomplete="off"
            spellcheck="false"
            value={variable.name}
            oninput={(event) => {
              const name = event.currentTarget.value.toUpperCase();
              const secret = variable.secret || secretByName(name);
              form.env[index] = { ...variable, name, secret };
            }}
          />
          <Field
            label={m.connections_env_value()}
            labelHidden
            type={variable.secret ? "password" : "text"}
            autocomplete="off"
            spellcheck="false"
            placeholder={variable.held ? m.connections_token_held() : m.connections_env_value()}
            value={variable.value}
            oninput={(event) =>
              (form.env[index] = { ...variable, value: event.currentTarget.value })}
          />
          <button
            type="button"
            class="square"
            aria-pressed={variable.secret}
            aria-label={m.connections_env_secret()}
            title={m.connections_env_secret()}
            onclick={() => (form.env[index] = { ...variable, secret: !variable.secret })}
            ><Icon icon={SquareLock02Icon} size={14} /></button
          >
          <button
            type="button"
            class="square"
            aria-label={m.connections_env_remove({
              name: variable.name || m.connections_env_name(),
            })}
            onclick={() => (form.env = form.env.filter((_, at) => at !== index))}
            ><Icon icon={Cancel01Icon} size={14} /></button
          >
        </div>
      {/each}
      {#if shown === "env"}<p class="fault" role="alert">{FAULTS.env()}</p>{/if}
      <button type="button" class="add-variable" onclick={addVariable}
        ><Icon icon={Add01Icon} size={13} />{m.connections_env_add()}</button
      >
    </div>
  {/if}
  <div class="buttons">
    <Button size="compact" variant="ghost" onclick={oncancel}>{m.connections_cancel()}</Button>
    <Button size="compact" variant="primary" type="submit" pending={saving}
      >{m.connections_save()}</Button
    >
  </div>
</form>

<style>
  .editor {
    display: grid;
    gap: 12px;
    margin: 14px 0 4px 44px;
    animation: editor-in var(--motion-base) var(--ease-out);
  }

  @keyframes editor-in {
    from {
      opacity: 0;
      translate: 0 -4px;
    }
  }

  .presets {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }

  .preset {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 4px 11px 4px 9px;
    border: 0;
    border-radius: var(--radius-capsule);
    background: var(--color-fill);
    color: var(--color-text);
    font: inherit;
    font-size: var(--text-label);
    line-height: 18px;
    transition: background-color var(--motion-instant) var(--ease-smooth);
  }

  .preset:hover {
    background: var(--color-fill-hover);
  }

  .preset[aria-pressed="true"] {
    background: var(--color-lit);
    color: var(--color-on-lit);
  }

  .kind,
  .auth {
    display: flex;
    align-items: center;
    gap: 12px;
  }

  .label {
    color: var(--color-muted);
    font-size: var(--text-label);
    font-weight: 500;
  }

  .kind .label,
  .auth .label {
    min-inline-size: 52px;
  }

  .editor :global(.command input) {
    font-family: var(--font-mono);
    font-size: var(--text-caption);
  }

  .env {
    display: grid;
    gap: 8px;
  }

  .variable {
    display: grid;
    grid-template-columns: minmax(0, 0.9fr) minmax(0, 1.4fr) 28px 28px;
    align-items: center;
    gap: 6px;
  }

  .variable :global(input) {
    font-family: var(--font-mono);
    font-size: var(--text-caption);
  }

  .square {
    display: grid;
    inline-size: 28px;
    block-size: 28px;
    padding: 0;
    border: 0;
    border-radius: var(--radius-control-compact);
    background: transparent;
    color: var(--color-faint);
    place-items: center;
    transition:
      background-color var(--motion-instant) var(--ease-smooth),
      color var(--motion-instant) var(--ease-smooth);
  }

  .square:hover {
    background: var(--color-fill-hover);
    color: var(--color-text);
  }

  .square[aria-pressed="true"] {
    color: var(--color-text);
  }

  .add-variable {
    display: inline-flex;
    align-items: center;
    justify-self: start;
    gap: 6px;
    padding: 3px 8px 3px 6px;
    border: 0;
    border-radius: var(--radius-control-compact);
    background: transparent;
    color: var(--color-muted);
    font: inherit;
    font-size: var(--text-label);
  }

  .add-variable:hover {
    background: var(--color-fill-hover);
    color: var(--color-text);
  }

  .fault {
    margin: 0;
    color: var(--color-danger);
    font-size: var(--text-label);
  }

  .preset:focus-visible,
  .square:focus-visible,
  .add-variable:focus-visible {
    outline: 2px solid var(--color-ring);
    outline-offset: 1px;
  }

  .buttons {
    display: flex;
    justify-content: flex-end;
    gap: 6px;
  }

  @media (prefers-reduced-motion: reduce) {
    .editor {
      animation: none;
    }
  }
</style>
