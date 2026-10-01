<script lang="ts">
  import type { ArtifactContent } from "$shared/ui/data/Artifact";
  import LazyView from "$shared/ui/LazyView";
  import { CODE_CARD_LINES, loadCodeBlock } from "$shared/ui/data/Code";
  import * as m from "$shared/i18n/messages";
  let { content, label }: { content: Extract<ArtifactContent, { kind: "code" }>; label: string } =
    $props();
</script>

<!-- The first lines with their notes; the lift has the whole text. -->
<div class="code">
  <LazyView
    loader={loadCodeBlock}
    loadingLabel={m.surface_loading()}
    failureLabel={m.work_artifact_unavailable()}
    retryLabel={m.surface_retry()}
    >{#snippet children(CodeBlock)}<CodeBlock
        language={content.language}
        text={content.text}
        notes={content.notes}
        {label}
        limit={CODE_CARD_LINES}
      />{/snippet}</LazyView
  >
</div>

<style>
  .code {
    margin-inline: -4px;
  }
</style>
