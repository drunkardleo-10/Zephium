<script lang="ts" module>
  import {
    File02Icon,
    FileBoxIcon,
    FileCodeIcon,
    FileEmpty02Icon,
    FileImageIcon,
    FileMusicIcon,
    FilePlayIcon,
    FileSpreadsheetIcon,
    FileZipIcon,
  } from "@hugeicons/core-free-icons";
  import type { IconSvgElement } from "@hugeicons/svelte";
  import type { DownloadKind } from "$domain/downloads";

  // One outline family, one weight: the kind reads from the mark inside the
  // page, never from colour.
  const GLYPHS: Record<DownloadKind, IconSvgElement> = {
    image: FileImageIcon,
    video: FilePlayIcon,
    audio: FileMusicIcon,
    archive: FileZipIcon,
    document: File02Icon,
    sheet: FileSpreadsheetIcon,
    code: FileCodeIcon,
    app: FileBoxIcon,
    file: FileEmpty02Icon,
  };
</script>

<script lang="ts">
  import { downloadKind } from "$domain/downloads";
  import Icon from "$shared/ui/Icon";

  let {
    filename,
    size = 32,
    alert = false,
  }: {
    filename: string;
    size?: number;
    /** The download stopped on a problem; the one mark of colour it carries. */
    alert?: boolean;
  } = $props();

  let glyph = $derived(GLYPHS[downloadKind(filename)]);
</script>

<span class="glyph" style:--glyph={`${size}px`} aria-hidden="true">
  <Icon icon={glyph} size={Math.round(size * 0.56)} strokeWidth={1.5} />
  {#if alert}<i class="badge"></i>{/if}
</span>

<style>
  .glyph {
    position: relative;
    display: grid;
    flex: none;
    place-items: center;
    width: var(--glyph);
    height: var(--glyph);
    border-radius: calc(var(--glyph) * 0.3);
    background: var(--color-fill);
    box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--color-text) 5%, transparent);
    color: var(--color-muted);
  }

  /* An exclamation drawn in CSS, so a failed row costs no extra icon data. */
  .badge {
    position: absolute;
    inset-block-end: -2px;
    inset-inline-end: -2px;
    width: 13px;
    height: 13px;
    border-radius: 50%;
    background: var(--color-danger);
  }

  .badge::before,
  .badge::after {
    content: "";
    position: absolute;
    inset-inline-start: 50%;
    width: 1.75px;
    border-radius: 1px;
    background: var(--color-on-accent);
    translate: -50% 0;
  }

  .badge::before {
    inset-block-start: 3px;
    height: 4.5px;
  }

  .badge::after {
    inset-block-end: 2.75px;
    height: 1.75px;
  }

  @media (forced-colors: active) {
    .badge {
      background: Mark;
    }
  }
</style>
