<script lang="ts">
  let { onopencitation }: { onopencitation: (url: string) => void } = $props();
</script>

<button onclick={() => onopencitation("https://example.com/source")}>Open source in Browse</button>
