<script lang="ts">
  import ToolSlot from "../components/ToolSlot.svelte";
  let {
    tool,
    profile,
    host,
    onback,
  }: {
    tool: "notes" | "tasks";
    profile: string;
    host: "sidebar" | "floating";
    onback: () => void;
  } = $props();
</script>

<div class="host" style:width={host === "sidebar" ? "384px" : "760px"}>
  {#if host === "sidebar"}
    <div class="sidebar-columns">
      <div class="sidebar-browser-column sidebar-tool-rail"></div>
      <div class="sidebar-tool-host"><ToolSlot {tool} {profile} {host} onclose={() => {}} /></div>
    </div>
  {:else}
    <ToolSlot {tool} {profile} {host} {onback} onclose={() => {}} />
  {/if}
</div>

<style>
  .host {
    font: 13px system-ui;
    color: var(--color-text);
    background: var(--color-chrome);
    display: flex;
    height: 700px;
    min-width: 0;
  }
</style>
