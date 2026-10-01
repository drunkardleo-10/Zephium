<script lang="ts">
  import { untrack } from "svelte";
  import { noteSession } from "$domain/notes";
  import * as m from "$shared/i18n/messages";
  import NoteView from "./NoteView.svelte";

  /** One note by identity, for a host that shows a single note on its own. */
  let { profile, host, id }: { profile: string; host: string; id: string } = $props();

  let session = $state.raw(untrack(() => noteSession(profile, host)));
  $effect(() => {
    const owner = profile;
    const key = host;
    const note = id;
    return untrack(() => {
      const current = noteSession(owner, key);
      session = current;
      if (!current) return;
      void current.start().then(() => current.open(note));
      return () => current.stop();
    });
  });
</script>

<div class="note-host scrolls">
  {#if session?.note}{#key session.note.version}<NoteView {session} density="page" />{/key}
  {:else if session?.openError}<p role="alert">{m.note_unavailable()}</p>{/if}
</div>

<style>
  .note-host {
    height: 100%;
    overflow: auto;
  }
</style>
