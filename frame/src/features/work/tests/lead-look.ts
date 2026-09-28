import type {
  WorkArtifactV1,
  WorkEnvironmentSnapshot,
  WorkExecutionFact,
  WorkPartFactV1,
} from "$shared/ipc/bindings";
import type { BoardScene } from "./board-fixtures";

type Data = WorkArtifactV1["data"];

/**
 * The exported trip as the lead runtime would record it: parts for the stay,
 * the flights, the batch and the entry rules, the steps it took in each, and
 * the objects it made. Real pages and frames; the objects are written to the
 * spec's shapes for looking at, not taken from a run.
 */
export function leadTrip(scene: BoardScene, live: boolean): BoardScene {
  const [objective, projection] = [...scene.objectives.entries()][0]!;
  const copy = structuredClone(projection);
  const run: WorkExecutionFact = copy.executions.at(-1)!;
  const partOf = (url: string, query: string): string | undefined => {
    if (/airbnb\./u.test(url) || /airbnb/iu.test(query)) return "stay";
    if (/ycombinator\./u.test(url) || /y combinator/iu.test(query)) return "batch";
    if (/flight|LOT|SFO|BART/iu.test(query)) return "flights";
    if (/ESTA|visa|citizens/iu.test(query)) return "entry";
    return undefined;
  };
  let steps = (run.steps ?? []).filter(
    (step) => step.kind.kind !== "publish" && step.kind.kind !== "finish",
  );
  if (live) {
    const last = steps.findLastIndex((step) => step.kind.kind === "read");
    steps = steps.slice(0, last + 1);
    steps[last] = { ...steps[last]!, status: "running" };
    const flight = steps.findLastIndex(
      (step) => step.kind.kind === "search" && /LOT|SFO/u.test(step.kind.query),
    );
    if (flight >= 0) steps[flight] = { ...steps[flight]!, status: "running" };
  }
  run.steps = steps.map((step) => {
    const url = step.kind.kind === "read" ? step.kind.url : "";
    const query = step.kind.kind === "search" ? step.kind.query : "";
    const part = partOf(url, query);
    return part ? { ...step, part } : step;
  });
  const state = (_part: string, done: boolean): WorkPartFactV1["state"] =>
    live && !done ? "running" : "done";
  run.parts = [
    {
      id: "stay",
      title: "Stay",
      helper: "browser",
      service: { host: "airbnb.com" },
      goal: "A month near the YC office",
      state: state("stay", false),
      summary: "3 homes",
    },
    {
      id: "flights",
      title: "Flights",
      helper: "research",
      service: { host: "lot.com" },
      goal: "Warsaw to San Francisco and back",
      state: state("flights", false),
      summary: "2 flights",
    },
    {
      id: "batch",
      title: "YC batch",
      helper: "browser",
      service: { host: "ycombinator.com" },
      goal: "The batch dates",
      state: "done",
      summary: "Winter 2027",
    },
    {
      id: "entry",
      title: "Entry",
      helper: "research",
      goal: "What a Polish citizen needs",
      state: "done",
      summary: "ESTA",
    },
  ];
  run.status = live ? "running" : "completed";
  const base = run.artifacts[0]!;
  const object = (id: string, title: string, data: Data, part?: string): WorkArtifactV1 => ({
    ...base,
    id,
    title,
    data,
    evidence: [],
    ...(part ? { part } : {}),
  });
  const stays: Data = {
    kind: "picks",
    facet: "stay",
    items: [
      {
        name: "Lux 2br/2ba next to YC",
        subtitle: "Mountain View · whole flat",
        logo_host: "airbnb.com",
        price: { display: "zł 19,342 / month" },
        facts: [
          { label: "Walk to YC", value: "6 min", kind: "text" },
          { label: "Workspace", value: "yes", kind: "yes" },
        ],
        recommended: true,
        why: "Closest to the office, with a desk.",
      },
      {
        name: "Apartment in San Francisco",
        subtitle: "SoMa · one bedroom",
        logo_host: "airbnb.com",
        price: { display: "zł 15,900 / month" },
        facts: [{ label: "Caltrain", value: "10 min", kind: "text" }],
      },
      {
        name: "SF sublet near Caltrain",
        subtitle: "Mission Bay · studio",
        logo_host: "airbnb.com",
        price: { display: "zł 12,480 / month" },
        facts: [{ label: "Caltrain", value: "4 min", kind: "text" }],
      },
    ],
  };
  const flights: Data = {
    kind: "picks",
    facet: "flight",
    items: [
      {
        name: "LOT via Chicago",
        price: { display: "zł 4,120" },
        route: {
          from: "WAW",
          to: "SFO",
          depart: "10:25",
          arrive: "17:05",
          duration: "15 h 40 m",
          stops: 1,
          carrier: "LOT",
          carrier_host: "lot.com",
        },
        recommended: true,
      },
      {
        name: "Lufthansa via Frankfurt",
        price: { display: "zł 4,560" },
        route: {
          from: "WAW",
          to: "SFO",
          depart: "06:50",
          arrive: "13:55",
          duration: "16 h 5 m",
          stops: 1,
          carrier: "Lufthansa",
          carrier_host: "lufthansa.com",
        },
      },
    ],
  };
  const made: WorkArtifactV1[] = live
    ? [
        object(
          "lead-batch",
          "YC Winter 2027",
          {
            kind: "list",
            style: "requirements",
            items: [
              {
                title: "Batch runs 5 January to 25 March 2027",
                from: { host: "ycombinator.com", app: "Y Combinator" },
              },
            ],
          },
          "batch",
        ),
      ]
    : [
        object("lead-reply", "", {
          kind: "reply",
          headline: "Your Winter 2027 batch, Warsaw to San Francisco",
          text: "Fly LOT via Chicago on 3 January, stay by the YC office for the batch, and apply for ESTA at least 72 hours before you fly.",
          figures: [
            { label: "Total", value: "zł 27,580" },
            { label: "Stay", value: "zł 19,342" },
            { label: "Flights", value: "zł 8,240" },
          ],
          points: [],
        }),
        object("lead-stays", "Stays", stays, "stay"),
        object("lead-flights", "Flights", flights, "flights"),
        object(
          "lead-batch",
          "YC Winter 2027",
          {
            kind: "list",
            style: "requirements",
            items: [
              {
                title: "Batch runs 5 January to 25 March 2027",
                from: { host: "ycombinator.com", app: "Y Combinator" },
              },
            ],
          },
          "batch",
        ),
        object("lead-plan", "Your trip", {
          kind: "plan",
          checkable: true,
          total: { label: "Total", value: "zł 27,580" },
          steps: [
            {
              when: "Now",
              title: "Apply for ESTA",
              detail: "At least 72 hours before you fly",
              kind: "task",
              cost: "$21",
            },
            {
              when: "3 Jan",
              title: "Fly WAW → SFO",
              detail: "LOT via Chicago, 15 h 40 m",
              kind: "travel",
              cost: "zł 4,120",
            },
            {
              when: "3 Jan",
              title: "BART from SFO",
              detail: "About 30 min to downtown",
              kind: "travel",
              cost: "$10",
            },
            {
              when: "3 Jan – 28 Mar",
              title: "Stay next to YC",
              detail: "Lux 2br/2ba, Mountain View",
              kind: "stay",
              cost: "zł 19,342",
            },
            { when: "5 Jan", title: "Batch starts", kind: "milestone" },
            { when: "25 Mar", title: "Demo Day", kind: "event" },
            {
              when: "28 Mar",
              title: "Fly SFO → WAW",
              detail: "Lufthansa via Frankfurt",
              kind: "travel",
              cost: "zł 4,120",
            },
          ],
        }),
      ];
  run.artifacts = [
    ...run.artifacts.filter((artifact) => artifact.data.kind === "evidence_collection"),
    ...made,
  ];
  const snapshot: WorkEnvironmentSnapshot = {
    ...scene.snapshot,
    elements: [
      ...scene.snapshot.elements.filter((element) => element.reference.kind === "objective"),
      ...made.map((artifact) => ({
        id: `el-${artifact.id}`,
        area: null,
        reference: {
          kind: "artifact" as const,
          objective,
          execution: run.id,
          artifact: artifact.id,
        },
      })),
    ],
  };
  return { ...scene, snapshot, objectives: new Map([[objective, copy]]) };
}
