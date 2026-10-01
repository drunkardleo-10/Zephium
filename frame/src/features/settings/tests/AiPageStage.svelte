<script lang="ts">
  import AiPage from "../components/sections/AiPage.svelte";
</script>

<!-- The Settings shell's own page frame around the section, as Settings.svelte draws it. -->
<section class="settings-shell" style="height: 100vh">
  <div class="settings-content scrolls">
    <div class="settings-page">
      <div class="settings-view">
        <header class="settings-page-heading">
          <span class="settings-eyebrow">Settings</span>
          <h1>AI</h1>
          <p>The models Work runs on and the keys it uses.</p>
        </header>
        <AiPage />
      </div>
    </div>
  </div>
</section>
