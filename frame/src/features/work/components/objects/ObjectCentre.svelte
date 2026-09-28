<script lang="ts">
  import type { ObjectActions, ObjectView as View } from "../../lib/board/types";
  import ObjectView from "./ObjectView.svelte";
  /** An object opened in the centre: all of it, at reading size, inside the lift's well. */
  let { object, actions = {} }: { object: View; actions?: ObjectActions } = $props();
</script>

<div class="centre {object.kind}">
  <ObjectView {object} detail="full" {actions} centre />
</div>

<style>
  .centre {
    box-sizing: border-box;
    inline-size: 100%;
    margin-inline: auto;
  }

  .reply,
  .document,
  .list,
  .note,
  .draft,
  .file {
    max-inline-size: 680px;
  }

  /* The lift is the surface: an object's own material steps back inside it. */
  .centre :global(:is(.plot, .sheet, .code, .diff)) {
    box-shadow: none;
  }
</style>
