<script lang="ts">
  import * as m from "$shared/i18n/messages";
  import * as preview from "../../lib/preview.svelte";
  import {
    languages,
    readLanguages,
    normalizeLanguages,
    moveLanguage,
  } from "../../lib/language-model";
  import PreviewNotice from "../PreviewNotice.svelte";
  import PreviewDialog from "../PreviewDialog.svelte";
  import PreviewSelect from "../PreviewSelect.svelte";
  import PreviewToggle from "../PreviewToggle.svelte";
  import SettingsGroup from "$shared/ui/SettingsGroup";
  import SettingsRow from "$shared/ui/SettingsRow";
  import Select from "$shared/ui/Select";
  import Checkbox from "$shared/ui/Checkbox";
  import Button from "$shared/ui/Button";
  import IconButton from "$shared/ui/IconButton";
  import SearchField from "$shared/ui/SearchField";
  import { ArrowUp02Icon, ArrowDown02Icon, Cancel01Icon } from "@hugeicons/core-free-icons";
  let interfaceCode = $derived(
    languages.find((language) => language.code === preview.get("languages.interface", "en-US"))
      ?.code ?? "en-US",
  );
  let codes = $derived(readLanguages(preview.get("languages.preferred-codes", '["en-US"]')));
  let query = $state("");
  let adding = $state(false);
  let selection = $state<string[]>([]);
  let filtered = $derived(
    languages.filter((language) =>
      (language.name + " " + language.code).toLowerCase().includes(query.trim().toLowerCase()),
    ),
  );
  function save(next: string[]) {
    preview.set("languages.preferred-codes", JSON.stringify(normalizeLanguages(next)));
  }
  function showPicker() {
    selection = [];
    query = "";
    adding = true;
  }
</script>

<PreviewNotice />
<SettingsGroup title={m.language_interface_title()}>
  <SettingsRow
    settingId="languages.interface"
    title={m.language_interface_title()}
    description={m.language_interface_help()}
    ><Select
      label={m.language_interface_title()}
      labelHidden
      value={interfaceCode}
      options={languages.map((language) => ({ value: language.code, label: language.name }))}
      onchange={(value) => preview.set("languages.interface", value)}
    /></SettingsRow
  >
</SettingsGroup>
{#if interfaceCode !== "en-US"}<p class="settings-inline-note" role="status">
    {m.language_preview_notice()}
  </p>{/if}
<section class="settings-collection preferred-languages" data-setting="languages.preferred">
  <header>
    <div>
      <h2>{m.language_preferred_title()}</h2>
      <p>{m.language_preferred_help()}</p>
    </div>
    <Button onclick={showPicker} disabled={codes.length >= 20}>{m.action_add()}</Button>
  </header>
  <ol class="language-order">
    {#each codes as code, index (code)}{@const language = languages.find(
        (language) => language.code === code,
      )!}
      <li>
        <span class="language-index" aria-hidden="true">{index + 1}</span>
        <div class="language-label">
          <strong>{language.name}</strong><span>{index === 0 ? m.language_first() : code}</span>
        </div>
        <div class="language-actions">
          <IconButton
            icon={ArrowUp02Icon}
            label={m.language_move_up({ language: language.name })}
            buttonSize={26}
            disabled={index === 0}
            onclick={() => save(moveLanguage(codes, code, -1))}
          /><IconButton
            icon={ArrowDown02Icon}
            label={m.language_move_down({ language: language.name })}
            buttonSize={26}
            disabled={index === codes.length - 1}
            onclick={() => save(moveLanguage(codes, code, 1))}
          /><IconButton
            icon={Cancel01Icon}
            label={m.language_remove({ language: language.name })}
            buttonSize={26}
            disabled={codes.length === 1}
            onclick={() => save(codes.filter((value) => value !== code))}
          />
        </div>
      </li>{/each}
  </ol>
  <p class="settings-inline-note">{m.language_order_hint()}</p>
</section>
<SettingsGroup title={m.language_format_title()}
  ><PreviewSelect id="languages.format" /><PreviewToggle id="languages.spelling" /><PreviewToggle
    id="languages.translate"
  /></SettingsGroup
>
<PreviewDialog
  bind:open={adding}
  title={m.language_add()}
  valid={selection.length > 0 && selection.length + codes.length <= 20}
  onapply={() => save([...codes, ...selection])}
  applyLabel={m.language_add()}
>
  <SearchField label={m.language_search()} placeholder={m.language_search()} bind:value={query} />
  <div class="language-picker">
    {#each filtered as language (language.code)}<div class="language-picker-row">
        <Checkbox
          label={language.name}
          checked={codes.includes(language.code) || selection.includes(language.code)}
          disabled={codes.includes(language.code)}
          onchange={(checked) => {
            selection = checked
              ? [...selection, language.code]
              : selection.filter((code) => code !== language.code);
          }}
        />{#if codes.includes(language.code)}<small>{m.language_added()}</small>{/if}
      </div>{/each}{#if filtered.length === 0}<p>{m.language_no_matches()}</p>{/if}
  </div>
</PreviewDialog>
