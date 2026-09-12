<script lang="ts">
  let { fail }: { fail: boolean } = $props();
  function renderText() {
    if (fail) throw new Error("private render details");
    return "Restored view";
  }
  let text = $derived.by(renderText);
</script>

<p>{text}</p>
