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
    media: () => import("./Media.svelte"),
    page: () => import("./Page.svelte"),
    note: () => import("./Note.svelte"),
    file: () => import("./File.svelte"),
    folder: () => import("./Folder.svelte"),
    project: () => import("./Project.svelte"),
  };
  const loaded: Partial<Record<ObjectKind, Renderer>> = {};
</script>

<script lang="ts">
  import type { ObjectActions, ObjectView } from "../../lib/board/types";
  import Reply from "./Reply.svelte";
  /** One canvas object, drawn the same at every zoom. */
  let {
    object,
    actions = {},
    centre = false,
  }: {
    object: ObjectView;
    actions?: ObjectActions;
    /** The object opened in the centre: its whole reading or editing surface. */
    centre?: boolean;
  } = $props();
  let Drawn = $state.raw<Renderer | null>(null);
  $effect(() => {
    const kind = object.kind;
    const known = loaded[kind];
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
      loaded[kind] = renderer;
      if (current) Drawn = renderer;
    });
    return () => {
      current = false;
    };
  });
</script>

{#if object.kind === "reply"}<Reply {object} {centre} />
{:else if Drawn}<Drawn {object} {actions} {centre} />{/if}
