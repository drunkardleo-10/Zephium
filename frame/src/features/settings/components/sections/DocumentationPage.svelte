<script lang="ts">
  import * as m from "$shared/i18n/messages";
  import { Dialog } from "bits-ui";
  import Button from "$shared/ui/Button";
  import Icon from "$shared/ui/Icon";
  import {
    ArrowRight01Icon,
    Globe02Icon,
    StarIcon,
    KeyboardIcon,
    Shield01Icon,
  } from "@hugeicons/core-free-icons";
  const guides = [
    {
      id: "browsing",
      title: m.docs_browsing,
      help: m.docs_browsing_help,
      body: m.docs_browsing_body,
      icon: Globe02Icon,
    },
    {
      id: "essentials",
      title: m.docs_essentials,
      help: m.docs_essentials_help,
      body: m.docs_essentials_body,
      icon: StarIcon,
    },
    {
      id: "keyboard",
      title: m.docs_keyboard,
      help: m.docs_keyboard_help,
      body: m.docs_keyboard_body,
      icon: KeyboardIcon,
    },
    {
      id: "privacy",
      title: m.docs_privacy,
      help: m.docs_privacy_help,
      body: m.docs_privacy_body,
      icon: Shield01Icon,
    },
  ];
  let current = $state(guides[0]!);
  let open = $state(false);
</script>

<div class="settings-guide-grid">
  {#each guides as guide (guide.id)}<button
      type="button"
      onclick={() => {
        current = guide;
        open = true;
      }}
      ><span class="guide-icon"><Icon icon={guide.icon} size={21} /></span><strong
        >{guide.title()}</strong
      >
      <p>{guide.help()}</p>
      <span class="guide-action">{m.docs_read()}<Icon icon={ArrowRight01Icon} size={14} /></span
      ></button
    >{/each}
</div>
<Dialog.Root bind:open
  ><Dialog.Portal
    ><Dialog.Overlay class="settings-dialog-overlay" /><Dialog.Content class="settings-dialog"
      ><Dialog.Title class="settings-dialog-title">{current.title()}</Dialog.Title
      ><Dialog.Description class="settings-guide-body">{current.body()}</Dialog.Description><Button
        onclick={() => (open = false)}>{m.panel_close()}</Button
      ></Dialog.Content
    ></Dialog.Portal
  ></Dialog.Root
>
