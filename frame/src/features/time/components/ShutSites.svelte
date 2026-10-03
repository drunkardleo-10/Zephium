<!--
  The sites a focus round keeps shut. Suggestions come from the reader's own
  time, never from a list shipped with the browser.
-->
<script lang="ts">
  import { Add01Icon, Cancel01Icon, Globe02Icon } from "@hugeicons/core-free-icons";
  import * as m from "$shared/i18n/messages";
  import { favicons } from "$domain/favicons";
  import { preferences } from "$domain/preferences";
  import { normalizeSite } from "$domain/time";
  import FavIcon from "$shared/ui/FavIcon";
  import Icon from "$shared/ui/Icon";
  import IconButton from "$shared/ui/IconButton";

  let {
    suggestions = [],
  }: {
    /** Sites the reader spends time on, most first. */
    suggestions?: string[];
  } = $props();

  const MAX_SITES = 256;

  let draft = $state("");
  let invalid = $state(false);
  let sites = $derived(
    preferences
      .value("focus.blocked")
      .split("\n")
      .filter((site) => site.length > 0),
  );
  let offered = $derived(
    suggestions.filter((site) => !sites.some((shut) => covers(shut, site))).slice(0, 4),
  );

  function covers(shut: string, site: string) {
    return site === shut || site.endsWith(`.${shut}`);
  }

  function save(next: string[]) {
    void preferences.set("focus.blocked", next.slice(0, MAX_SITES).join("\n"));
  }

  function add(input: string) {
    const site = normalizeSite(input);
    if (site === null) {
      invalid = true;
      return;
    }
    invalid = false;
    draft = "";
    if (!sites.includes(site)) save([...sites, site]);
  }
</script>

<div class="shut">
  <form
    class="add"
    onsubmit={(event) => {
      event.preventDefault();
      add(draft);
    }}
  >
    <input
      type="text"
      inputmode="url"
      autocomplete="off"
      spellcheck="false"
      maxlength={2048}
      aria-label={m.focus_shut_placeholder()}
      aria-invalid={invalid}
      placeholder={m.focus_shut_placeholder()}
      value={draft}
      oninput={(event) => {
        draft = event.currentTarget.value;
        invalid = false;
      }}
    />
    <button type="submit" class="submit" disabled={!draft.trim() || preferences.saving()}
      ><Icon icon={Add01Icon} size={14} />{m.focus_shut_add()}</button
    >
  </form>
  {#if invalid}<p class="invalid" role="alert">{m.focus_shut_invalid()}</p>{/if}

  {#if offered.length > 0}
    <div class="suggested">
      <span class="caption">{m.focus_suggested()}</span>
      <div class="chips">
        {#each offered as site (site)}
          {@const mark = favicons.mark(null, `https://${site}/`)}
          <button
            type="button"
            class="chip"
            disabled={preferences.saving()}
            onclick={() => add(site)}
            ><FavIcon
              image={mark?.image ?? null}
              tone={mark?.tone}
              size={14}
              lit
              fallback={Globe02Icon}
            /><span>{site}</span><Icon icon={Add01Icon} size={12} /></button
          >
        {/each}
      </div>
    </div>
  {/if}

  {#if sites.length > 0}
    <ul aria-label={m.focus_shut_title()}>
      {#each sites as site (site)}
        {@const mark = favicons.mark(null, `https://${site}/`)}
        <li>
          <FavIcon
            image={mark?.image ?? null}
            tone={mark?.tone}
            size={16}
            lit
            fallback={Globe02Icon}
          />
          <span class="name">{site}</span>
          <IconButton
            icon={Cancel01Icon}
            label={m.focus_shut_remove({ site })}
            size={13}
            buttonSize={24}
            disabled={preferences.saving()}
            onclick={() => save(sites.filter((kept) => kept !== site))}
          />
        </li>
      {/each}
    </ul>
  {:else}
    <p class="none">{m.focus_none_shut()}</p>
  {/if}
</div>

<style>
  .shut {
    display: grid;
    gap: 12px;
  }

  .add {
    display: flex;
    gap: 6px;
  }

  input {
    flex: 1;
    min-width: 0;
    block-size: var(--field-height);
    padding-inline: 10px;
    border: 0;
    border-radius: var(--radius-field);
    background: var(--color-field);
    box-shadow: var(--shadow-field);
    color: var(--color-text);
    font: inherit;
    font-size: var(--text-body);
  }

  input::placeholder {
    color: var(--color-faint);
  }

  input:hover {
    background: var(--color-field-hover);
  }

  input:focus-visible {
    outline: none;
    box-shadow: var(--shadow-field-focus);
  }

  input[aria-invalid="true"] {
    box-shadow: inset 0 0 0 1px var(--color-danger);
  }

  .submit {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    padding-inline: 10px 12px;
    border: 0;
    border-radius: var(--radius-field);
    background: var(--color-control);
    color: var(--color-on-control);
    font: inherit;
    font-size: var(--text-label);
    font-weight: 500;
    cursor: pointer;
  }

  .submit:disabled {
    opacity: 0.5;
    cursor: default;
  }

  .submit:focus-visible {
    outline: 2px solid var(--color-ring);
    outline-offset: 1px;
  }

  .submit:hover:not(:disabled) {
    background: var(--color-control-hover);
  }

  .invalid {
    margin: -6px 0 0;
    color: var(--color-danger);
    font-size: var(--text-label);
  }

  .suggested {
    display: grid;
    gap: 6px;
  }

  .caption {
    color: var(--color-faint);
    font-size: var(--text-caption);
  }

  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }

  .chip {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 4px 8px 4px 6px;
    border: 0;
    border-radius: var(--radius-capsule);
    background: var(--color-fill);
    color: var(--color-label-secondary);
    font: inherit;
    font-size: var(--text-label);
    cursor: pointer;
    transition: background-color var(--motion-fast) var(--ease-out);
  }

  .chip:focus-visible {
    outline: 2px solid var(--color-ring);
    outline-offset: 1px;
  }

  .chip:hover:not(:disabled) {
    background: var(--color-fill-hover);
    color: var(--color-text);
  }

  ul {
    display: grid;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  li {
    display: flex;
    align-items: center;
    gap: 10px;
    min-height: 36px;
    padding-inline: 8px 4px;
    border-radius: var(--radius-row);
  }

  li + li {
    box-shadow: inset 0 1px 0 var(--color-border);
  }

  li:hover {
    background: var(--row-hover);
  }

  .name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    font-size: var(--text-body);
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .none {
    margin: 0;
    color: var(--color-muted);
    font-size: var(--text-label);
  }
</style>
