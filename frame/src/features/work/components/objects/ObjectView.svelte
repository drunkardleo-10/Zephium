<script module lang="ts">
  import type { Component } from "svelte";
  import type { ObjectKind } from "../../lib/board/types";
  type Renderer = Component<Record<string, unknown>>;
  /** Each kind's renderer is its own chunk, fetched the first time the canvas shows one. */
  const LOADERS: Partial<Record<ObjectKind, () => Promise<{ default: unknown }>>> = {
    plot: () => import("./Plot.svelte"),
    sheet: () => import("./Sheet.svelte"),
    picks: () => import("./Picks.svelte"),
    plan: () => import("./Plan.svelte"),
    list: () => import("./List.svelte"),
    diagram: () => import("./Diagram.svelte"),
    code: () => import("./Code.svelte"),
    diff: () => import("./Diff.svelte"),
    document: () => import("./Document.svelte"),
    draft: () => import("./Draft.svelte"),
  };
  const loaded = new Map<ObjectKind, Renderer>();
</script>

<script lang="ts">
  import type { Detail, ObjectActions, ObjectView } from "../../lib/board/types";
  import Reply from "./Reply.svelte";
  /** One canvas object at the detail its on-screen size allows. */
  let {
    object,
    detail = "full",
    actions = {},
  }: { object: ObjectView; detail?: Detail; actions?: ObjectActions } = $props();
  let Drawn = $state.raw<Renderer | null>(null);
  $effect(() => {
    const kind = object.kind;
    const known = loaded.get(kind);
    if (known) {
      Drawn = known;
      return;
    }
    Drawn = null;
    const load = LOADERS[kind];
    if (!load) return;
    let current = true;
    void load().then((module) => {
      const renderer = module.default as Renderer;
      loaded.set(kind, renderer);
      if (current) Drawn = renderer;
    });
    return () => {
      current = false;
    };
  });
</script>

{#if object.kind === "reply"}<Reply {object} {detail} />
{:else if Drawn}<Drawn {object} {detail} {actions} />{/if}
