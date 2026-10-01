<script lang="ts">
  import { SvelteMap } from "svelte/reactivity";
  import { provideLeadLook } from "$shared/ui/presence";
  import WorkCanvas from "../components/WorkCanvas.svelte";
  import {
    clearOfBands,
    environmentStages,
    fetchedPictures,
    measurer,
  } from "../lib/project-environment-board";
  import {
    environmentAgents,
    environmentBoards,
    environmentInputs,
    environmentItems,
    environmentParts,
    environmentSources,
    environmentView,
  } from "../lib/project-environment";
  import { environmentRequests, requestDress, runFolders } from "../lib/project-environment-thread";
  import type { BoardActions } from "../lib/canvas-context";
  import { defaultSize } from "../lib/canvas-model";
  import { exchangeOf } from "../components/asks/asks";
  import type { PartAsk } from "../lib/run/parts";
  import type { BoardScene } from "./board-fixtures";
  import type { NoteSummary } from "$shared/ipc/bindings";
  let {
    scene,
    viewport,
    asked = [],
    asks,
    notes = [],
  }: {
    scene: BoardScene;
    /** The person's notes the scene's resources may be. */
    notes?: NoteSummary[];
    /** Questions waiting on the person, by part. */
    asks?: (objective: string) => readonly PartAsk[];
    /** Without one, the canvas opens as a work with no saved camera: on its newest run. */
    viewport?: { x: number; y: number; zoom: number };
    asked?: string[];
  } = $props();
  provideLeadLook(() => scene.snapshot.id);
  const measured = new SvelteMap<string, number>();
  let open = $state<string | null>(null);
  const recorded = () => scene.pages;
  const stages = $derived(
    environmentStages(scene.snapshot, scene.objectives, {
      recorded,
      pictures: scene.pictures,
      ...(scene.media ? { fetched: fetchedPictures(scene.snapshot, scene.media) } : {}),
      measured,
      open,
      exchange: exchangeOf,
      ...(asks ? { asks } : {}),
    }),
  );
  const requests = $derived(environmentRequests(stages));
  const own = $derived(environmentView(scene.snapshot));
  const agents = $derived(
    environmentAgents(scene.snapshot, scene.objectives, () => undefined, stages),
  );
  const folders = $derived(runFolders(stages));
  const dress = $derived(requestDress(stages));
  const items = $derived([
    ...[
      ...environmentItems(scene.snapshot, [], notes, scene.objectives, scene.media).filter(
        (item) => !folders.has(item.id),
      ),
      ...requests.items,
    ].map((item) => (dress.get(item.id) ? { ...item, ...dress.get(item.id) } : item)),
    ...environmentBoards(stages),
    ...environmentParts(scene.objectives, stages, recorded),
    ...environmentInputs(stages),
    ...environmentSources(stages),
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
    measure: measurer(measured),
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
      ...clearOfBands(
        own.positions,
        {
          ...Object.fromEntries(
            environmentItems(scene.snapshot, [], notes, scene.objectives, scene.media).map(
              (item) => [item.id, defaultSize(item)],
            ),
          ),
          ...own.sizes,
        },
        stages,
        environmentItems(scene.snapshot, [], notes, scene.objectives, scene.media).flatMap(
          (item) => (item.type === "objective" || folders.has(item.id) ? [] : [item.id]),
        ),
      ),
      ...requests.positions,
      ...agents.positions,
    },
    sizes: own.sizes,
    ...(viewport ? { viewport } : {}),
  }}
  home={stages.at(-1)?.lane.box}
  fitTopInset={56}
  {board}
  work={(objective) => scene.objectives.get(objective)}
  oninspect={() => {}}
  onopen={(id) => board.toggle(id)}
/>
