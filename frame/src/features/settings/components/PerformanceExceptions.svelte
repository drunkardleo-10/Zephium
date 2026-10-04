<script lang="ts">
  import * as m from "$shared/i18n/messages";
  import { preferences } from "$domain/preferences";
  import { normalizeSite } from "$domain/time";
  import Button from "$shared/ui/Button";
  import Field from "$shared/ui/Field";

  let draft = $state("");
  let invalid = $state(false);
  let sites = $derived(preferences.value("performance.exceptions").split("\n").filter(Boolean));

  async function add() {
    const site = normalizeSite(draft);
    if (site === null) {
      invalid = true;
      return;
    }
    invalid = false;
    if (!sites.includes(site)) {
      await preferences.set("performance.exceptions", [...sites, site].join("\n"));
      if (preferences.saveFailed()) return;
    }
    draft = "";
  }
</script>

<div class="exceptions" data-setting="performance.exceptions">
  <form
    onsubmit={(event) => {
      event.preventDefault();
      void add();
    }}
  >
    <Field
      label={m.field_url()}
      bind:value={draft}
      autocomplete="off"
      inputmode="url"
      maxlength={2048}
      disabled={preferences.saving()}
      error={invalid ? m.field_invalid_url() : undefined}
    />
    <Button type="submit" disabled={!draft.trim() || preferences.saving() || sites.length >= 256}>
      {m.action_add()}
    </Button>
  </form>
  {#if sites.length > 0}
    <ul aria-label={m.pref_performance_exceptions()}>
      {#each sites as site (site)}
        <li>
          <span>{site}</span><Button
            variant="ghost"
            disabled={preferences.saving()}
            aria-label={`${m.action_remove()} ${site}`}
            onclick={() =>
              void preferences.set(
                "performance.exceptions",
                sites.filter((kept) => kept !== site).join("\n"),
              )}
          >
            {m.action_remove()}
          </Button>
        </li>
      {/each}
    </ul>
  {/if}
</div>

<style>
  .exceptions {
    display: grid;
    gap: 12px;
    padding: 14px 16px;
  }

  form {
    display: flex;
    align-items: end;
    gap: 8px;
  }

  ul {
    margin: 0;
    padding: 0;
    list-style: none;
  }

  li {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    min-block-size: 36px;
  }

  span {
    overflow: hidden;
    font-size: var(--text-body);
    text-overflow: ellipsis;
  }
</style>
