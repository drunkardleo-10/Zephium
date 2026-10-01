<script lang="ts">
  import Button from "$shared/ui/Button";
  import Icon from "$shared/ui/Icon";
  import * as m from "$shared/i18n/messages";
  import AskReceipt from "./AskReceipt.svelte";
  import AskShell from "./AskShell.svelte";
  import { folderName, type FolderAsk } from "./asks";
  import { Folder01Icon } from "./icons";

  /**
   * "Read Lunios?": a folder the request named, read for this work once
   * allowed. "Allow Documents?": a folder the run needs as it works, chosen
   * in the system's own folder panel.
   */
  let {
    ask,
    placement = "canvas",
    busy = false,
    onanswer,
    onchoose,
  }: {
    ask: FolderAsk;
    placement?: "canvas" | "island";
    busy?: boolean;
    /** Rust's own option words go back as they came. */
    onanswer: (answer: string) => void;
    /** Opens the folder panel at the folder; its choice goes back as the answer. */
    onchoose?: (() => void) | undefined;
  } = $props();

  /** Where a person keeps it: `~/Dev/Lunios`. */
  const home = (path: string) => path.replace(/^\/Users\/[^/]+/u, "~");
  const where = $derived(home(ask.path));
  const title = $derived(
    ask.choose
      ? m.work_ask_folder_choose_title({ name: ask.name })
      : m.work_ask_folder_title({ name: ask.name }),
  );
  const allowed = $derived(!!ask.answer && ask.answer !== ask.decline);
  /** The folder the person chose, which may not be the one asked for. */
  const chosen = $derived(
    ask.choose && ask.answer?.startsWith("/") ? folderName(ask.answer) : ask.name,
  );
</script>

{#if ask.state === "open"}
  <AskShell
    {placement}
    {busy}
    label={title}
    {where}
    {title}
    note={ask.choose ? (ask.reason ?? m.work_ask_folder_choose_note()) : m.work_ask_folder_note()}
  >
    {#snippet mark()}<Icon icon={Folder01Icon} size={16} />{/snippet}
    {#snippet actions()}
      <Button variant="ghost" disabled={busy} onclick={() => onanswer(ask.decline)}
        >{m.work_ask_not_now()}</Button
      >
      {#if ask.choose && onchoose}
        <Button variant="primary" pending={busy} onclick={onchoose}
          >{m.work_ask_folder_choose()}</Button
        >
      {:else}
        <Button variant="primary" pending={busy} onclick={() => onanswer(ask.allow)}
          >{m.work_ask_folder_allow()}</Button
        >
      {/if}
    {/snippet}
  </AskShell>
{:else if ask.answer}
  <AskReceipt
    tone={allowed ? "done" : "declined"}
    status={allowed ? m.work_ask_allowed() : m.work_ask_not_now()}
    text={chosen}
  />
{/if}
