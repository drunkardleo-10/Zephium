<script lang="ts">
  import * as m from "$shared/i18n/messages";
  import { tabs } from "$domain/tabs";
  import { favicons } from "$domain/favicons";
  import { WorkSitesSession, siteOf } from "$domain/work-context";
  import { commands, type WorkSiteAccessV1 } from "$shared/ipc/bindings";
  import Button from "$shared/ui/Button";
  import FavIcon from "$shared/ui/FavIcon";
  import IconButton from "$shared/ui/IconButton";
  import SegmentedControl from "$shared/ui/SegmentedControl";
  import SettingsGroup from "$shared/ui/SettingsGroup";
  import Icon from "$shared/ui/Icon";
  import Add01Icon from "@hugeicons/core-free-icons/Add01Icon";
  import Cancel01Icon from "@hugeicons/core-free-icons/Cancel01Icon";
  import Globe02Icon from "@hugeicons/core-free-icons/Globe02Icon";

  const profile = $derived(tabs.profile());
  let session = $state.raw<WorkSitesSession | null>(null);
  $effect(() => {
    const id = profile && profile.kind !== "incognito" ? profile.id : null;
    if (!id) return;
    const owner = new WorkSitesSession(id);
    session = owner;
    void owner.start().then(() => {
      const origins = owner.sites.map((row) => `https://${row.site}`);
      if (origins.length) void commands.faviconProbe(id, origins).catch(() => {});
    });
    return () => owner.dispose();
  });

  const choices = (sensitive: boolean) => [
    { value: "always", label: m.settings_sites_always(), disabled: sensitive },
    { value: "ask", label: m.settings_sites_ask() },
    { value: "never", label: m.settings_sites_never() },
  ];

  let adding = $state("");
  const added = $derived(siteOf(adding));
  const known = $derived(!!added && !!session?.sites.some((row) => row.site === added));
  let failed = $state(false);
  async function add() {
    if (!session || !added || known) return;
    failed = !(await session.set(added, "always"));
    if (!failed) adding = "";
  }
</script>

{#if !profile || profile.kind === "incognito"}
  <p class="sites-note">{m.work_regular_profile()}</p>
{:else}
  <SettingsGroup title={m.settings_sites_title()} description={m.settings_sites_help()}>
    {#if session?.loaded && session.sites.length === 0}
      <p class="empty">{m.settings_sites_empty()}</p>
    {/if}
    {#each session?.sites ?? [] as row (row.site)}
      {@const mark = favicons.forPage(`https://${row.site}`)}
      <div class="site">
        <span class="mark"
          ><FavIcon
            image={mark?.image ?? null}
            tone={mark?.tone}
            size={20}
            fallback={Globe02Icon}
          /></span
        >
        <span class="words">
          <strong>{row.name}</strong>
          <span
            >{#if row.name !== row.site}{row.site}{/if}{#if row.name !== row.site && row.sensitive}&nbsp;·&nbsp;{/if}{#if row.sensitive}<span
                class="sensitive">{m.settings_sites_sensitive()}</span
              >{/if}</span
          >
        </span>
        <SegmentedControl
          label={m.settings_sites_access({ name: row.name })}
          size="compact"
          options={choices(row.sensitive)}
          value={row.access}
          disabled={!!session?.busy}
          onchange={(value) => void session?.set(row.site, value as WorkSiteAccessV1)}
        />
        <IconButton
          icon={Cancel01Icon}
          size={14}
          buttonSize={26}
          label={m.settings_sites_forget({ name: row.name })}
          disabled={!!session?.busy}
          onclick={() => void session?.set(row.site, null)}
        />
      </div>
    {/each}
    <form
      class="add"
      onsubmit={(event) => {
        event.preventDefault();
        void add();
      }}
    >
      <span class="mark add-mark" aria-hidden="true"><Icon icon={Add01Icon} size={15} /></span>
      <label class="sr-only" for="work-site-add">{m.settings_sites_add_label()}</label>
      <input
        id="work-site-add"
        placeholder={m.settings_sites_add_placeholder()}
        autocomplete="off"
        spellcheck="false"
        maxlength="253"
        bind:value={adding}
        oninput={() => (failed = false)}
      />
      <Button type="submit" size="compact" disabled={!added || known || !!session?.busy}
        >{m.settings_sites_add()}</Button
      >
    </form>
  </SettingsGroup>
  {#if session?.unavailable || failed}<p class="sites-note" role="alert">
      {m.settings_sites_unavailable()}
    </p>{:else if adding.trim() && !added}<p class="sites-note">
      {m.settings_sites_invalid()}
    </p>{:else if known}<p class="sites-note">
      {m.settings_sites_known({ site: added ?? "" })}
    </p>{/if}
{/if}

<style>
  .site {
    position: relative;
    display: flex;
    align-items: center;
    gap: 12px;
    box-sizing: border-box;
    min-block-size: var(--row-page);
    padding: 10px 12px 10px 18px;
  }

  .site + .site::before,
  .add::before {
    content: "";
    position: absolute;
    inset-inline: 50px 0;
    inset-block-start: 0;
    block-size: 1px;
    background: var(--color-border);
  }

  .mark {
    display: grid;
    flex: none;
    place-items: center;
    inline-size: 20px;
    block-size: 20px;
  }

  .words {
    display: flex;
    flex: 1;
    flex-direction: column;
    gap: 1px;
    min-inline-size: 0;
  }

  strong {
    overflow: hidden;
    color: var(--color-text);
    font-size: var(--text-page-title);
    font-weight: 500;
    line-height: 19px;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .words > span {
    overflow: hidden;
    color: var(--color-muted);
    font-size: var(--text-label);
    line-height: 16px;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .sensitive {
    color: var(--color-warning);
  }

  .add {
    position: relative;
    display: flex;
    align-items: center;
    gap: 12px;
    margin: 0;
    padding: 10px 12px 10px 18px;
  }

  .add-mark {
    color: var(--color-muted);
  }

  .add input {
    flex: 1;
    min-inline-size: 0;
    block-size: 28px;
    padding: 0;
    border: 0;
    background: transparent;
    color: var(--color-text);
    font: inherit;
    font-size: var(--text-page-title);
    outline: none;
  }

  .add input::placeholder {
    color: var(--color-faint);
  }

  .empty {
    margin: 0;
    padding: 16px 18px 12px;
    color: var(--color-muted);
    font-size: var(--text-body);
    line-height: 1.5;
  }

  .sites-note {
    margin: -26px 16px 28px;
    color: var(--color-muted);
    font-size: var(--text-label);
    line-height: 1.5;
  }
</style>
