<script lang="ts">
  import Button from "$shared/ui/Button";
  import Icon from "$shared/ui/Icon";
  import * as m from "$shared/i18n/messages";
  import AskReceipt from "./AskReceipt.svelte";
  import AskShell from "./AskShell.svelte";
  import type { FolderAsk } from "./asks";
  import { Folder01Icon } from "./icons";

  /** "Read Lunios?": a folder the request named, read for this work once allowed. */
  let {
    ask,
    placement = "canvas",
    busy = false,
    onanswer,
  }: {
    ask: FolderAsk;
    placement?: "canvas" | "island";
    busy?: boolean;
    /** Rust's own option words go back as they came. */
    onanswer: (answer: string) => void;
  } = $props();

  /** The folder where a person keeps it: `~/Dev/Lunios`. */
  const where = $derived(ask.path.replace(/^\/Users\/[^/]+/u, "~"));
</script>

{#if ask.state === "open"}
  <AskShell
    {placement}
    {busy}
    label={m.work_ask_folder_title({ name: ask.name })}
    {where}
    title={m.work_ask_folder_title({ name: ask.name })}
    note={m.work_ask_folder_note()}
  >
    {#snippet mark()}<Icon icon={Folder01Icon} size={16} />{/snippet}
    {#snippet actions()}
      <Button variant="ghost" disabled={busy} onclick={() => onanswer(ask.decline)}
        >{m.work_ask_not_now()}</Button
      >
      <Button variant="primary" pending={busy} onclick={() => onanswer(ask.allow)}
        >{m.work_ask_folder_allow()}</Button
      >
    {/snippet}
  </AskShell>
{:else if ask.answer}
  <AskReceipt
    tone={ask.answer === ask.allow ? "done" : "declined"}
    status={ask.answer === ask.allow ? m.work_ask_allowed() : m.work_ask_not_now()}
    text={ask.name}
  />
{/if}
