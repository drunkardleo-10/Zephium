<script lang="ts">
  import ConnectionsPage from "../components/sections/ConnectionsPage.svelte";
</script>

<!-- The Settings shell's own page frame around the section, as Settings.svelte draws it. -->
<section class="settings-shell" style="height: 100vh">
  <div class="settings-content scrolls">
    <div class="settings-page">
      <div class="settings-view">
        <header class="settings-page-heading">
          <span class="settings-eyebrow">Settings</span>
          <h1>Connections</h1>
          <p>The tools and services Work can use as you.</p>
        </header>
        <ConnectionsPage />
      </div>
    </div>
  </div>
</section>
