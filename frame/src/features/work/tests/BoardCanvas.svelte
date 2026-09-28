<script lang="ts">
  import { SvelteMap } from "svelte/reactivity";
  import WorkCanvas from "../components/WorkCanvas.svelte";
  import { clearOfBands, environmentStages, measureKey } from "../lib/project-environment-board";
  import {
    environmentAgents,
    environmentBoards,
    environmentBranches,
    environmentItems,
    environmentSources,
    environmentView,
  } from "../lib/project-environment";
  import { environmentRequests } from "../lib/project-environment-thread";
  import type { BoardActions } from "../lib/canvas-context";
  import type { BoardScene } from "./board-fixtures";
  let {
    scene,
    viewport = { x: 40, y: 40, zoom: 1 },
    asked = [],
  }: {
    scene: BoardScene;
    viewport?: { x: number; y: number; zoom: number };
    asked?: string[];
  } = $props();
  const measured = new SvelteMap<string, number>();
  let open = $state<string | null>(null);
  const recorded = () => scene.pages;
  const stages = $derived(
    environmentStages(scene.snapshot, scene.objectives, {
      recorded,
      pictures: scene.pictures,
      measured,
      open,
    }),
  );
  const requests = $derived(environmentRequests(stages));
  const own = $derived(environmentView(scene.snapshot));
  const agents = $derived(
    environmentAgents(scene.snapshot, scene.objectives, () => undefined, stages),
  );
  const items = $derived([
    ...environmentItems(scene.snapshot, [], [], scene.objectives),
    ...requests.items,
    ...environmentBoards(stages, open),
    ...environmentSources(scene.objectives, stages),
    ...environmentBranches(scene.objectives, stages, recorded),
    ...agents.items,
  ]);
  const pictures = $derived(
    new Map(
      stages.flatMap((stage) =>
        stage.board.blocks.flatMap((block) =>
          (block.kind === "gallery"
            ? block.entities
            : block.kind === "entity"
              ? [block.entity]
              : []
          ).flatMap((entity) => (entity.image ? [[entity.key, entity.image] as const] : [])),
        ),
      ),
    ),
  );
  const board: BoardActions = {
    measure(id, width, opened, height) {
      const key = measureKey(id, width, opened);
      if (measured.get(key) !== height) measured.set(key, height);
    },
    toggle: (id) => (open = open === id ? null : id),
    ask: (name) => asked.push(name),
    choose: () => {},
    evidence: () => {},
    entity: () => {},
    command: () => {},
    page: () => {},
    note: () => undefined,
  };
</script>

<WorkCanvas
  {items}
  links={requests.links}
  {pictures}
  authoritative={new Set()}
  initialView={{
    positions: {
      ...clearOfBands(own.positions, own.sizes ?? {}, stages),
      ...requests.positions,
      ...agents.positions,
    },
    sizes: own.sizes,
    viewport,
  }}
  {board}
  oninspect={() => {}}
  onopen={(id) => board.toggle(id)}
/>
