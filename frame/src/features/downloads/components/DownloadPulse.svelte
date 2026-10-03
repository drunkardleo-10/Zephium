<!--
  Where the sidebar is too narrow for the download card, a download that
  starts or finishes is announced by its glyph alone. Renders nothing.
-->
<script lang="ts">
  import { untrack } from "svelte";
  import { DownloadSession, finished } from "$domain/downloads";
  import * as notices from "$session/notice.svelte";
  import * as m from "$shared/i18n/messages";

  let { profile }: { profile: string } = $props();
  let session = $state.raw(untrack(() => new DownloadSession(profile)));
  /** Downloads already announced, by id and whether they had finished. */
  let seen: Map<string, boolean> | null = null;

  $effect(() => {
    const next = new DownloadSession(profile);
    session = next;
    seen = null;
    void next.start(false);
    return () => next.stop();
  });

  $effect(() => {
    const entries = session.entries;
    untrack(() => {
      const known = seen;
      seen = new Map(entries.map((entry) => [entry.id, finished(entry)]));
      // The first snapshot is what was already there; only what follows is news.
      if (known === null) return;
      for (const entry of entries) {
        const before = known.get(entry.id);
        if (before === undefined) notices.show(m.notice_download_started(), "download");
        else if (!before && entry.state === "completed")
          notices.show(m.notice_download_finished(), "download");
      }
    });
  });
</script>
