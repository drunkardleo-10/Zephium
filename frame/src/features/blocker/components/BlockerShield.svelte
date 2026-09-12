<script lang="ts">
  import { Shield01Icon } from "@hugeicons/core-free-icons";
  import { blocker } from "$domain/blocker";
  import { shieldPresentation } from "$domain/blocker";
  import Icon from "$shared/ui/Icon";

  let shield = $derived(shieldPresentation(blocker.status()));
</script>

<!--
  Chrome shows protection standing only. Coverage counts, supply-chain
  identities and runtime health are diagnostics: they belong to Settings,
  Privacy, not to the browsing surface.
-->
{#if shield.visible}
  <span
    class="flex h-5 w-5 shrink-0 items-center justify-center"
    class:text-faint={shield.tone === "quiet"}
    class:opacity-40={shield.blocked}
    class:text-warning={shield.tone === "warning"}
    title={shield.label}
    role="img"
    aria-label={shield.label}
  >
    <Icon icon={Shield01Icon} size={14} />
  </span>
{/if}
