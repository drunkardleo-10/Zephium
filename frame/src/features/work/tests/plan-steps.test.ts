import { expect, test } from "vitest";
import { defaultSize } from "../lib/canvas-model";
import { stepIcon } from "../lib/plan-steps";
import {
  environmentClusters,
  environmentItems,
  environmentLinks,
  environmentSteps,
  resultHeads,
} from "../lib/project-environment";
import { environmentStages } from "../lib/project-environment-thread";
import { planScene } from "./environment-fixtures";

test("a step's glyph comes from a closed keyword table, travel words first", () => {
  expect(
    [
      "Confirm the batch and interview dates, then choose a travel window.",
      "Check your ESTA eligibility before booking.",
      "Enter dates and a budget on Airbnb’s monthly-stay page.",
      "Compare fares from WAW to SFO.",
      "Set aside $300 for the first week.",
      "Submit the application form.",
      "Tell the team.",
    ].map(stepIcon),
  ).toEqual(["dates", "entry", "stay", "flight", "money", "document", "check"]);
});

test("a plan's steps stand beside their result, one card each, and never twice", () => {
  const { scene, objectives } = planScene();
  const stages = environmentStages(scene, objectives);
  const { items } = environmentSteps(scene, objectives, stages);
  expect(items.map((item) => [item.id, item.step?.index, item.step?.icon])).toEqual([
    ["step:plan-card:0", 1, "dates"],
    ["step:plan-card:1", 2, "entry"],
    ["step:plan-card:2", 3, "stay"],
    ["step:plan-card:3", 4, "flight"],
  ]);
  // A citation marker stays with the lift.
  expect(items[1]!.title).toBe("Check your ESTA eligibility and official entry requirements.");
  const result = environmentItems(scene, [], [], objectives).find(
    (item) => item.id === "plan-card",
  );
  expect(result?.type).toBe("result");
});

test("a pure result reads request → Made in slot 1, its steps boxed beside it, and no tie reaches the request", () => {
  const { scene, objectives } = planScene();
  const stages = environmentStages(scene, objectives);
  const layout = stages[0]!.layout;
  expect(layout.groups.map((group) => group.kind)).toEqual(["made"]);
  const result = layout.positions["plan-card"]!;
  expect(result).toEqual({ x: 348 + 24, y: 44 });
  const width = stages[0]!.contents.results!.members[0]!.size.width;
  const steps = layout.groups[0]!.steps![0]!;
  expect(steps.box).toMatchObject({ x: result.x + width + 48, y: 44 });
  expect(layout.positions["step:plan-card:0"]).toEqual({
    x: steps.box.x + 16,
    y: steps.box.y + 16 + 20,
  });
  expect(layout.positions["step:plan-card:1"]!.x).toBe(steps.box.x + 16 + 220 + 16);
  const { clusters, links } = environmentClusters(stages);
  expect(links.map((link) => [link.source, link.target])).toEqual([
    ["objective-card", "group:objective-card:made"],
    ["plan-card", "group:objective-card:steps:plan-card"],
  ]);
  expect(clusters.map((cluster) => [cluster.id, cluster.label, cluster.within ?? []])).toEqual([
    ["group:objective-card:steps:plan-card", "4 steps", []],
    ["group:objective-card:made", "Result", ["group:objective-card:steps:plan-card"]],
  ]);
  expect(environmentLinks(scene)).toEqual([]);
});

test("a checklist draws no cover: its steps stand in its place, their caption carrying its title", () => {
  const { scene, objectives } = planScene();
  const run = objectives.get("objective")!.executions[0]!;
  run.artifacts[0]!.title = "Implementation roadmap";
  run.artifacts[0]!.general_knowledge = true;
  run.artifacts[0]!.data = {
    kind: "checklist",
    items: Array.from({ length: 11 }, (_, index) => ({
      text: `Step ${index + 1}`,
      completed: false,
    })),
  };
  const stages = environmentStages(scene, objectives);
  const layout = stages[0]!.layout;
  expect(layout.positions["plan-card"]).toBeUndefined();
  const box = layout.groups[0]!.steps![0]!.box;
  expect(box.x).toBe(348 + 24);
  // Eleven steps four across.
  const lefts = new Set(
    Array.from({ length: 11 }, (_, index) => layout.positions[`step:plan-card:${index}`]!.x),
  );
  expect(lefts.size).toBe(4);
  const { clusters, links } = environmentClusters(stages, resultHeads(scene, objectives, stages));
  expect(links.map((link) => [link.source, link.target])).toEqual([
    ["objective-card", "group:objective-card:made"],
  ]);
  expect(clusters[0]).toMatchObject({
    id: "group:objective-card:steps:plan-card",
    label: "11 steps",
    title: "Implementation roadmap",
    opens: "plan-card",
  });
  expect(clusters[0]).not.toHaveProperty("knowledge");
  expect(clusters[1]).toMatchObject({
    id: "group:objective-card:made",
    members: [],
    within: ["group:objective-card:steps:plan-card"],
  });
});

test("cards are as tall as what they say, up to their cap", () => {
  const base = { id: "card", kind: "", detail: "", status: "" };
  const document = (paragraphs: string[]) =>
    defaultSize({
      ...base,
      type: "result",
      title: "Plan",
      artifact: {
        key: "a",
        title: "Plan",
        reviewLabel: "",
        evidence: [],
        content: { kind: "document", paragraphs },
      },
    });
  const short = document(["One line."]);
  const long = document(["word ".repeat(200)]);
  expect(short.width).toBe(420);
  expect(long.height).toBeGreaterThan(short.height);
  expect(
    document(["word ".repeat(2000), ...Array.from({ length: 40 }, (_, i) => `## Part ${i}`)])
      .height,
  ).toBe(560);
  const claims = (count: number) =>
    defaultSize({
      ...base,
      type: "findings",
      title: "Findings",
      findings: {
        items: Array.from({ length: count }, () => ({
          claim: "A claim that runs long enough to need a second line on the card.",
          confidence: "supported" as const,
          evidence: 1,
        })),
        total: count,
      },
    });
  expect(claims(2).height).toBeLessThan(claims(8).height);
  expect(claims(12).height).toBeLessThanOrEqual(420);
  const subject = (facts: number) =>
    defaultSize({
      ...base,
      type: "subject",
      title: "Charming Cole Valley Victorian",
      facts: Array.from({ length: facts }, (_, index) => ({
        label: `Fact ${index}`,
        value: "4.89",
      })),
    });
  expect(subject(0)).toEqual({ width: 220, height: expect.any(Number) });
  expect(subject(4).height).toBeGreaterThan(subject(1).height);
  expect(subject(4).height).toBeLessThanOrEqual(300);
  const request = (text: string) => defaultSize({ ...base, type: "request", title: text });
  expect(request("Short").height).toBeLessThan(request("word ".repeat(40)).height);
  expect(request("word ".repeat(400)).height).toBe(request("word ".repeat(80)).height);
  const sources = (rows: number) =>
    defaultSize({
      ...base,
      type: "sources",
      title: "Sources",
      sources: Array.from({ length: rows }, (_, index) => ({
        key: String(index),
        url: `https://a.example/${index}`,
        where: "a.example",
        title: "A page",
      })),
    });
  expect(sources(2).height).toBeLessThan(sources(6).height);
  expect(sources(20).height).toBe(sources(6).height);
});
