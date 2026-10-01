import type { WorkEnvironmentReference, WorkExecutionFact } from "$shared/ipc/bindings";
import type {
  ArtifactView,
  DocumentNodeView,
  EvidenceReference,
  FindingView,
} from "$shared/ui/data/Artifact";
import { formatValue } from "$shared/ui/data/Chart";
import { workChartSpec } from "$shared/ui/data/Artifact/work-chart";
import { artifactView } from "../project-work";
import { subjectFacts, subjectKey, subjectMatrixRows, subjectsOf } from "../subjects";
import { entityOf } from "./entities";
import { leadObject, onCanvas } from "./objects";
import { columnTypes, labelKey, timelineOf } from "./tabular";
import { overlap, plain, splitLead, terms } from "./text";
import type {
  Block,
  Board,
  ChartBlock,
  ComparisonBlock,
  Entity,
  EntityFacet,
  Fact,
  Picture,
  ProseBlock,
  TableBlock,
} from "./types";

type Element = { id: string; reference: WorkEnvironmentReference };
export type BoardInput = {
  id: string;
  executions: readonly WorkExecutionFact[];
  /** The lane's elements: what its runs placed on the canvas, in the order they were placed. */
  elements: readonly Element[];
  /** An element's admitted picture. */
  pictures?: ReadonlyMap<string, Picture>;
  /** Elements the person chose. */
  chosen?: ReadonlySet<string>;
  /** While a run is live and no answer stands yet: what the prose is waiting on. */
  pending?: string;
};

type Placed = { element: string; execution: WorkExecutionFact; view: ArtifactView };

/**
 * A request's board, derived from what its runs published: the answer is the
 * prose and its first sentence the lead; subjects are entities, a kind of them
 * a gallery; findings fold onto the entity they name or into the prose; a
 * diagram, gallery, comparison or timeline leads when there is one.
 */
export function boardOf(input: BoardInput): Board {
  const sources = new Map<string, EvidenceReference>();
  const cite = (references: readonly EvidenceReference[]) => {
    for (const reference of references)
      if (!sources.has(reference.key)) sources.set(reference.key, reference);
    return references.map((reference) => reference.key);
  };
  const placed: Placed[] = [];
  const entities: Entity[] = [];
  const known = new Set<string>();
  for (const element of input.elements) {
    const reference = element.reference;
    if (reference.kind !== "artifact" && reference.kind !== "subject") continue;
    const execution = input.executions.find((entry) => entry.id === reference.execution);
    const artifact = execution?.artifacts.find((entry) => entry.id === reference.artifact);
    if (!execution || !artifact || !onCanvas(artifact, execution)) continue;
    if (reference.kind === "artifact") {
      // The lead's objects are drawn as themselves, not through a legacy block.
      if (leadObject(artifact)) continue;
      placed.push({ element: element.id, execution, view: artifactView(artifact, execution) });
      continue;
    }
    const subject = subjectsOf(artifact)[reference.index];
    if (!subject) continue;
    const key = subjectKey(subject);
    if (known.has(key)) continue;
    known.add(key);
    const rows = subjectMatrixRows(execution, subject);
    const cited = rows.flatMap(({ artifact: matrix, index }) => {
      const view = artifactView(matrix, execution);
      return view.content.kind === "matrix"
        ? (view.content.cells[index] ?? []).flatMap((cell) => cell.evidence)
        : [];
    });
    const image = input.pictures?.get(element.id);
    entities.push(
      entityOf(key, subject, subjectFacts(execution, subject), {
        element: element.id,
        ...(image ? { image } : {}),
        sources: cite(uniqueBy(cited)).slice(0, 3),
        chosen: !!input.chosen?.has(element.id),
      }),
    );
  }

  const blocks: Block[] = [];
  let title = "";
  let lead = "";
  let prose: ProseBlock | null = null;
  const loose: { claim: string; keys: string[] }[] = [];
  for (const { element, view } of placed) {
    const content = view.content;
    const base = {
      id: element,
      title: view.title,
      emphasis: "primary" as const,
      state: "ready" as const,
    };
    switch (content.kind) {
      case "answer": {
        if (prose) break;
        const split = leadOf(content.blocks);
        title = view.title;
        lead = split.lead;
        prose = {
          ...base,
          kind: "prose",
          blocks: split.blocks,
          cites: {},
          ...(view.knowledge ? {} : { sources: cite(view.evidence) }),
        };
        blocks.push(prose);
        break;
      }
      case "document":
        blocks.push({ ...base, kind: "document", emphasis: "supporting", content });
        break;
      case "table": {
        const stops = timelineOf(content.columns, content.rows);
        blocks.push(
          stops
            ? { ...base, kind: "timeline", stops }
            : {
                ...base,
                kind: "table",
                columns: columnTypes(content.columns, content.rows),
                rows: content.rows,
              },
        );
        break;
      }
      case "comparison":
      case "matrix":
        blocks.push({ ...base, kind: "comparison", content, sources: cite(view.evidence) });
        break;
      case "chart": {
        const headline = headlineOf(content);
        blocks.push({ ...base, kind: "chart", chart: content, ...(headline ? { headline } : {}) });
        break;
      }
      case "checklist":
        blocks.push({ ...base, kind: "checklist", emphasis: "supporting", items: content.items });
        break;
      case "code":
        blocks.push({
          ...base,
          kind: "code",
          language: content.language,
          text: content.text,
          notes: content.notes,
        });
        break;
      case "diagram":
        if (content.nodes.length) blocks.push({ ...base, kind: "diagram", diagram: content });
        break;
      case "browser":
        blocks.push({
          ...base,
          kind: "callout",
          emphasis: "supporting",
          tone: "quote",
          text: content.summary,
        });
        break;
      case "findings":
        foldFindings(content.subjects, content.items, entities, loose, cite);
        break;
      default:
        break;
    }
  }

  // Facts no entity names close the paragraph they speak to, or read as the prose.
  const said = (prose?.sources ?? []).map((key) => ({
    claim: `${sources.get(key)?.label ?? ""} ${sources.get(key)?.origin ?? ""}`,
    keys: [key],
  }));
  if (prose && (loose.length || said.length)) {
    const at = blocks.indexOf(prose);
    prose = attach(prose, [...loose, ...said]);
    blocks[at] = prose;
  } else if (loose.length) {
    prose = attach(synthesized(`${input.id}:facts`, loose), loose);
    blocks.unshift(prose);
  }

  blocks.push(...entityBlocks(entities));
  fold(blocks);
  if (!prose && input.pending)
    blocks.unshift({
      id: `${input.id}:pending`,
      kind: "prose",
      emphasis: "primary",
      state: "pending",
      pending: input.pending,
      blocks: [],
      cites: {},
    });
  if (!title) title = blocks.find((block) => block.title)?.title ?? "";
  group(blocks);
  // A short answer reads on from its lead, above everything it introduces.
  const more = prose && short(prose) ? prose : undefined;
  if (more) blocks.splice(blocks.indexOf(more), 1);
  emphasize(blocks);
  return {
    id: input.id,
    title,
    lead,
    ...(more ? { more } : {}),
    blocks,
    sources: Object.fromEntries(sources),
  };
}

/** An answer of a few plain paragraphs, nothing a block would hold. */
const SHORT = 420;
function short(prose: ProseBlock): boolean {
  if (prose.state !== "ready" || !prose.blocks.length) return false;
  if (!prose.blocks.every((block) => block.type === "paragraph")) return false;
  return prose.blocks.reduce((sum, block) => sum + plain(block).length, 0) <= SHORT;
}

function uniqueBy(references: readonly EvidenceReference[]): EvidenceReference[] {
  const seen = new Set<string>();
  return references.filter((reference) => !seen.has(reference.key) && !!seen.add(reference.key));
}

/** The answer's first sentence becomes the board's lead; its paragraph keeps the rest. */
function leadOf(blocks: readonly DocumentNodeView[]): {
  lead: string;
  blocks: DocumentNodeView[];
} {
  const at = blocks.findIndex((block) => block.type === "paragraph");
  if (at < 0 || blocks.slice(0, at).some((block) => block.type !== "heading"))
    return { lead: "", blocks: [...blocks] };
  const first = blocks[at]!;
  const { lead, rest } = splitLead(plain(first));
  if (!lead) return { lead: "", blocks: [...blocks] };
  // A paragraph with marks keeps them only when the lead was all of it.
  const kept: DocumentNodeView | null = rest
    ? { type: "paragraph", content: [{ type: "text", text: rest }] }
    : null;
  const next = [...blocks.slice(0, at), ...(kept ? [kept] : []), ...blocks.slice(at + 1)];
  // A heading that only introduced the lead has nothing left under it.
  return {
    lead,
    blocks: next[0]?.type === "heading" && next.length === 1 ? [] : next,
  };
}

/** Each finding onto the entity it names, as a fact with its sources; the rest are loose. */
function foldFindings(
  subjects: readonly { name: string }[],
  items: readonly FindingView[],
  entities: Entity[],
  loose: { claim: string; keys: string[] }[],
  cite: (references: readonly EvidenceReference[]) => string[],
) {
  for (const item of items) {
    const keys = cite(item.evidence);
    const named = item.subject === undefined ? undefined : subjects[item.subject];
    const entity = named
      ? entities.find((candidate) => candidate.key === subjectKey(named))
      : undefined;
    if (entity) {
      const fact: Fact = {
        label: "",
        value: item.claim,
        ...(keys.length ? { sources: keys } : {}),
      };
      if (!entity.facts.some((known) => known.value === item.claim)) entity.facts.push(fact);
      continue;
    }
    loose.push({ claim: item.claim, keys });
  }
}

/**
 * Sources close the paragraph or list item whose words they share most, and
 * the last one when none shares a word. A block is addressed by its index, a
 * list item by its list's index and its own: "3.1".
 */
function attach(prose: ProseBlock, facts: readonly { claim: string; keys: string[] }[]) {
  const spots: { at: string; words: Set<string> }[] = [];
  prose.blocks.forEach((block, index) => {
    if (block.type === "bulletList" || block.type === "orderedList")
      (block.content ?? []).forEach((item, row) =>
        spots.push({ at: `${index}.${row}`, words: terms(plain(item)) }),
      );
    else if (block.type === "paragraph" || block.type === "blockquote")
      spots.push({ at: String(index), words: terms(plain(block)) });
  });
  const last = spots.at(-1);
  if (!last) return prose;
  const cites: Record<string, string[]> = {};
  for (const fact of facts) {
    const words = terms(fact.claim);
    let best = last.at;
    let score = 0;
    for (const spot of spots) {
      const shared = overlap(words, spot.words);
      if (shared > score) [best, score] = [spot.at, shared];
    }
    const list = (cites[best] ??= []);
    for (const key of fact.keys) if (!list.includes(key)) list.push(key);
  }
  return { ...prose, cites };
}

/** Facts with no answer to live in read as a short list, each closed by its sources. */
function synthesized(id: string, facts: readonly { claim: string; keys: string[] }[]): ProseBlock {
  const blocks: DocumentNodeView[] = [
    {
      type: "bulletList",
      content: facts.map((fact) => ({
        type: "listItem",
        content: [{ type: "paragraph", content: [{ type: "text", text: fact.claim }] }],
      })),
    },
  ];
  return {
    id,
    kind: "prose",
    emphasis: "primary",
    state: "ready",
    blocks,
    cites: {},
  };
}

/** Entities of one kind stand as a gallery; a kind with one member stands alone. */
function entityBlocks(entities: readonly Entity[]): Block[] {
  const byFacet = new Map<EntityFacet, Entity[]>();
  for (const entity of entities) {
    const list = byFacet.get(entity.facet) ?? [];
    list.push(entity);
    byFacet.set(entity.facet, list);
  }
  return [...byFacet.values()].map((list): Block => {
    const first = list[0]!;
    const id = first.element ?? `entity:${first.key}`;
    return list.length === 1
      ? { id, kind: "entity", emphasis: "primary", state: "ready", entity: first }
      : {
          id,
          kind: "gallery",
          emphasis: "primary",
          state: "ready",
          facet: first.facet,
          entities: list,
        };
  });
}

/**
 * A comparison whose subjects are all a gallery's, or a lone entity's, is that
 * gallery: it takes the comparison's title and opens onto its table.
 */
function fold(blocks: Block[]) {
  for (const comparison of blocks.filter(
    (block): block is ComparisonBlock => block.kind === "comparison",
  )) {
    const content = comparison.content;
    if (content.kind !== "matrix" || !content.subjects.length) continue;
    const keys = content.subjects.map((subject) => subjectKey(subject));
    const holder = blocks.find((block) => {
      const held =
        block.kind === "gallery"
          ? block.entities.map((entity) => entity.key)
          : block.kind === "entity"
            ? [block.entity.key]
            : [];
      return held.length > 0 && keys.every((key) => held.includes(key));
    });
    if (!holder) continue;
    if (holder.kind === "gallery") {
      holder.title = comparison.title;
      holder.compare = content;
      if (comparison.sources) holder.sources = comparison.sources;
    }
    blocks.splice(blocks.indexOf(comparison), 1);
  }
}

/** A chart says its total when it carries one, or its span when its values are ranges. */
function headlineOf(chart: Extract<ArtifactView["content"], { kind: "chart" }>) {
  const spec = workChartSpec({ xLabel: chart.xLabel, yLabel: chart.yLabel, series: chart.series });
  const points = spec.series[0]?.points ?? [];
  const total = points.find((point) =>
    /^(total|overall|sum|all|in total)\b/iu.test(String(point.x)),
  );
  if (total?.y !== null && total?.y !== undefined)
    return { label: String(total.x), value: total.display ?? formatValue(total.y, spec.y) };
  if (spec.kind !== "range") return undefined;
  const lows = points.flatMap((point) => (point.y === null ? [] : [point.y]));
  const highs = points.flatMap((point) => (point.y === null ? [] : [point.y2 ?? point.y]));
  if (lows.length < 2) return undefined;
  const low = formatValue(Math.min(...lows), spec.y);
  const high = formatValue(Math.max(...highs), spec.y);
  return { label: chart.yLabel || spec.series[0]!.name, value: `${low}–${high}` };
}

/**
 * A chart and the table of its own rows are one block, the table its exact
 * values; a chart whose table only shares words with it sits beside it.
 */
function group(blocks: Block[]) {
  const tables = blocks.filter((block): block is TableBlock => block.kind === "table");
  const taken = new Set<string>();
  for (const chart of blocks.filter((block): block is ChartBlock => block.kind === "chart")) {
    const labels = new Set(
      chart.chart.series.flatMap((series) => series.points.map((point) => labelKey(point.label))),
    );
    const firsts = (table: TableBlock) => new Set(table.rows.map((row) => labelKey(row[0] ?? "")));
    const shared = (table: TableBlock) => [...labels].filter((label) => firsts(table).has(label));
    const free = tables.filter((table) => !taken.has(table.id));
    const match =
      free.find((table) => labels.size > 0 && shared(table).length * 2 >= labels.size) ??
      free.find((table) => overlap(terms(table.title ?? ""), terms(chart.title ?? "")) > 0);
    if (!match) continue;
    taken.add(match.id);
    // The same rows twice are one block: the chart, its table as its exact values.
    if (labels.size > 0 && shared(match).length * 2 >= labels.size) {
      chart.values = { columns: match.columns, rows: match.rows };
      chart.title ??= match.title;
      blocks.splice(blocks.indexOf(match), 1);
      continue;
    }
    match.group = chart.group = `pair:${match.id}`;
    // The chart follows its table.
    blocks.splice(blocks.indexOf(chart), 1);
    blocks.splice(blocks.indexOf(match) + 1, 0, chart);
  }
}

const HERO = ["diagram", "gallery", "comparison", "timeline"] as const;
/** One block leads: a diagram, a gallery, a comparison or a timeline; otherwise the prose. */
function emphasize(blocks: Block[]) {
  for (const kind of HERO) {
    const hero = blocks.find((block) => block.kind === kind);
    if (hero) {
      hero.emphasis = "hero";
      return;
    }
  }
  const prose = blocks.find((block) => block.kind === "prose");
  if (prose) prose.emphasis = "hero";
}
