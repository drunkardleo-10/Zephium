<script lang="ts">
  import Lift from "../components/Lift.svelte";
  import LiftHeader from "../components/LiftHeader.svelte";
  let { bounds, onclose }: { bounds: DOMRect; onclose: () => void } = $props();
</script>

<Lift origin={null} {bounds} preferred={{ width: 2000, height: 2000 }} title="Stripe" {onclose}>
  <LiftHeader kind="Subject" title="Stripe" host="stripe.com" onhost={() => {}} />
  <div style:block-size="2400px">Body</div>
</Lift>
