<script lang="ts">
  import type { Component } from "svelte";
  let { title, description, page }: { title: string; description: string; page: Component } =
    $props();
  const Page = $derived(page);
</script>

<!-- The Settings shell's own page frame around a section, as Settings.svelte draws it. -->
<section class="settings-shell" style="height: 100vh">
  <div class="settings-content scrolls">
    <div class="settings-page">
      <div class="settings-view">
        <header class="settings-page-heading">
          <span class="settings-eyebrow">Settings</span>
          <h1>{title}</h1>
          <p>{description}</p>
        </header>
        <Page />
      </div>
    </div>
  </div>
</section>
