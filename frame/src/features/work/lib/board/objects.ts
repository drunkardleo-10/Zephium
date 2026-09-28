import type { WorkArtifactV1, WorkExecutionFact } from "$shared/ipc/bindings";
import type { CellView, DocumentNodeView, EvidenceReference } from "$shared/ui/data/Artifact";
import { styleSpec, type ChartSpec } from "$shared/ui/data/Chart";
import { workChartSpec } from "$shared/ui/data/Artifact/work-chart";
import { mediaUrl } from "$domain/resources";
import { artifactView } from "../project-work";
import { plain } from "./text";
import { siteKey, siteName } from "../run/site";
import type {
  Block,
  Board,
  Emphasis,
  Entity,
  EntityFacet,
  ObjectView,
  PickView,
  PicksView,
  PlanStepView,
  ReplyView,
  SheetColumn,
  SheetView,
} from "./types";

/**
 * One object of a run as the canvas places it: its view, the legacy block that
 * draws it until its object renderer exists, the part it belongs to, and the
 * earlier object it stands in for.
 */
export type RunObject = {
  id: string;
  view: ObjectView;
  /** The legacy block it was, for the group it stands in. */
  block?: Block;
  emphasis: Emphasis;
  /** The lead part that made it: it sits at the end of that part's row. */
  part?: string;
  /** Its own name, kept for its opened view where the canvas leaves it unsaid. */
  name?: string;
};

const NEW_KINDS = new Set([
  "reply",
  "picks",
  "plan",
  "list",
  "sheet",
  "plot",
  "diff",
  "draft",
  "media",
]);
/** Whether an artifact is one of the lead's objects rather than a legacy result. */
export const leadObject = (artifact: WorkArtifactV1) => NEW_KINDS.has(artifact.data.kind);

const REPLY = { text: 480, point: 110, points: 5, figures: 4, why: 120 } as const;

/** Text cut at the last sentence, or word, that fits; never mid-word. */
function within(text: string, max: number): string {
  const clean = text.replace(/\s+/gu, " ").trim();
  if (clean.length <= max) return clean;
  const cut = clean.slice(0, max);
  const sentence = Math.max(cut.lastIndexOf(". "), cut.lastIndexOf("! "), cut.lastIndexOf("? "));
  if (sentence > max * 0.5) return cut.slice(0, sentence + 1);
  const word = cut.lastIndexOf(" ");
  return `${cut.slice(0, word > 0 ? word : max).replace(/[,;:]$/u, "")}…`;
}

/** Two phrasings of one title: the same words, whatever the case and punctuation. */
const same = (a: string, b: string) =>
  !!a &&
  a.toLowerCase().replace(/[^\p{L}\p{N}]+/gu, "") ===
    b.toLowerCase().replace(/[^\p{L}\p{N}]+/gu, "");

const numberOf = (text: string | null | undefined) => {
  if (text === null || text === undefined || text === "") return undefined;
  const value = Number(text);
  return Number.isFinite(value) ? value : undefined;
};

const hostOf = (url: string | undefined) => {
  if (!url) return undefined;
  try {
    return new URL(url).hostname.replace(/^www\./u, "");
  } catch {
    return undefined;
  }
};

const FACETS: Record<EntityFacet, PicksView["facet"]> = {
  place: "place",
  stay: "stay",
  flight: "flight",
  product: "product",
  person: "person",
  company: "company",
  event: "event",
  repo: "repo",
  pull_request: "repo",
  message: "other",
  job: "job",
};

const TITLE_BREAK = /\s+[-|•·–—]\s+/u;
const lower = (text: string) => text.toLocaleLowerCase();
/** "San Francisco, California": a comma list of names, each one capitalised. */
const PLACE =
  /^(?:\p{Lu}[\p{L}\p{M}.'’]*(?:\s+\p{Lu}[\p{L}\p{M}.'’]*)*)(?:,\s*\p{Lu}[\p{L}\p{M}.'’]*(?:\s+\p{Lu}[\p{L}\p{M}.'’]*)*)*$/u;

/**
 * A page title given as a pick's name, without the tail the page adds: the
 * site's own name ("- Airbnb San Francisco - California") and a place already
 * said elsewhere ("- Flats for Rent in San Francisco, California"). The place
 * it drops can stand as the pick's subtitle; the page keeps its whole title.
 */
export function pickName(
  name: string,
  url: string | undefined,
  context: string,
): { name: string; place?: string } {
  const parts = name.split(TITLE_BREAK);
  if (parts.length < 2) return { name };
  const host = hostOf(url);
  const key = host ? siteKey(host) : "";
  const brand = host ? lower(siteName(host)) : "";
  const names = (part: string) => {
    const words = lower(part);
    return (
      (!!key && words.replace(/[^\p{L}\p{N}]/gu, "").includes(key)) ||
      (!!brand && words.includes(brand))
    );
  };
  const places: string[] = [];
  let kept = parts.length;
  const cut = parts.findIndex((part, index) => index > 0 && names(part));
  if (cut > 0) {
    for (const part of parts.slice(cut)) {
      const rest = brand
        ? part.replace(new RegExp(brand.replace(/[.*+?^${}()|[\]\\]/gu, "\\$&"), "iu"), "").trim()
        : part;
      if (rest && PLACE.test(rest)) places.push(rest);
    }
    kept = cut;
  }
  const said = lower(context);
  while (kept > 1) {
    const place = /(?:^|\s)in\s+(.+)$/u.exec(parts[kept - 1] ?? "")?.[1] ?? parts[kept - 1] ?? "";
    const first = place.split(",")[0]?.trim() ?? "";
    const others = lower(parts.slice(0, kept - 1).join(" "));
    if (
      !PLACE.test(place) ||
      !first ||
      !(said.includes(lower(first)) || others.includes(lower(first)))
    )
      break;
    places.unshift(place);
    kept -= 1;
  }
  if (kept === parts.length) return { name };
  const seen = new Set<string>();
  const place = places
    .flatMap((text) => text.split(",").map((part) => part.trim()))
    .filter((part) => part && !seen.has(lower(part)) && seen.add(lower(part)))
    .join(", ");
  return { name: parts.slice(0, kept).join(" - "), ...(place ? { place } : {}) };
}

function pickOf(entity: Entity, context: string): PickView {
  const src = entity.image ? mediaUrl(entity.image.profile, entity.image.digest) : null;
  const labelled = entity.facts.filter((fact) => fact.label.trim());
  const said = entity.facts.find((fact) => !fact.label.trim());
  const logo = hostOf(entity.homepage);
  const named = pickName(entity.name, entity.homepage, context);
  return {
    ...(entity.element ? { element: entity.element } : {}),
    name: named.name,
    ...(entity.descriptor
      ? { subtitle: entity.descriptor }
      : named.place
        ? { subtitle: named.place }
        : {}),
    ...(src ? { picture: { src } } : logo ? { logo } : {}),
    ...(entity.homepage ? { url: entity.homepage } : {}),
    ...(entity.price ? { price: { display: entity.price } } : {}),
    facts: labelled.slice(0, 4).map((fact) => ({
      label: fact.label,
      value: fact.value,
      kind: "text" as const,
      ...(fact.sources ? { sources: fact.sources } : {}),
    })),
    ...(said ? { why: within(said.value, REPLY.why) } : {}),
    tags: [],
    recommended: false,
    ...(entity.chosen ? { chosen: true } : {}),
    ...(entity.time ? { when: entity.time } : {}),
    ...(entity.sources?.length ? { sources: entity.sources } : {}),
  };
}

const COLUMN_KIND: Record<string, SheetColumn["kind"]> = {
  text: "text",
  long: "text",
  number: "number",
  money: "money",
  date: "date",
  link: "link",
};

/** What the rest of a set says, where a name's trailing place may already stand. */
const othersText = (
  items: readonly { name: string; descriptor?: string | null }[],
  index: number,
  title?: string,
) =>
  [
    title ?? "",
    ...items.filter((_, at) => at !== index).map((item) => `${item.name} ${item.descriptor ?? ""}`),
  ].join(" ");

/** A legacy block as the object it is now: an answer's prose is never one (it is the reply). */
function legacyView(block: Block, board: Board): ObjectView | null {
  const base = {
    id: block.id,
    ...(block.title ? { title: block.title } : {}),
    ...(block.state === "pending" ? { state: "pending" as const } : {}),
    sources: board.sources,
  };
  switch (block.kind) {
    case "gallery":
      return {
        ...base,
        kind: "picks",
        facet: FACETS[block.facet],
        items: block.entities.map((entity, index) =>
          pickOf(entity, othersText(block.entities, index, block.title)),
        ),
      };
    case "entity":
      return {
        ...base,
        kind: "picks",
        facet: FACETS[block.entity.facet],
        items: [pickOf(block.entity, block.title ?? "")],
      };
    case "comparison": {
      const content = block.content;
      if (content.kind === "matrix")
        return {
          ...base,
          kind: "sheet",
          columns: [
            { label: "", kind: "entity" },
            ...content.criteria.map((criterion) => ({
              label: criterion.name,
              kind:
                criterion.kind === "rating"
                  ? ("rating" as const)
                  : criterion.kind === "presence"
                    ? ("yes_no" as const)
                    : ("text" as const),
              ...(criterion.unit ? { unit: criterion.unit } : {}),
            })),
          ],
          rows: content.subjects.map((subject, row) => {
            const logo = hostOf(subject.homepage);
            return {
              cells: [
                pickName(
                  subject.name,
                  subject.homepage,
                  othersText(content.subjects, row, block.title),
                ).name,
                ...content.criteria.map((criterion, column) =>
                  cellText(content.cells[row]?.[column]?.value, criterion.scaleMax),
                ),
              ],
              ...(logo ? { entity: { logo } } : {}),
            };
          }),
        } satisfies SheetView;
      return {
        ...base,
        kind: "sheet",
        columns: [
          { label: "", kind: "text" },
          ...content.criteria.map((label) => ({ label, kind: "text" as const })),
        ],
        rows: content.alternatives.map((alternative) => ({
          cells: [alternative.name, ...alternative.values],
        })),
      } satisfies SheetView;
    }
    case "table":
      return {
        ...base,
        kind: "sheet",
        columns: block.columns.map((column) => ({
          label: column.label,
          kind: COLUMN_KIND[column.type] ?? "text",
        })),
        rows: block.rows.map((cells) => ({ cells: [...cells] })),
      };
    case "chart":
      return {
        ...base,
        kind: "plot",
        spec: {
          ...workChartSpec({
            xLabel: block.chart.xLabel,
            yLabel: block.chart.yLabel,
            series: block.chart.series,
          }),
          ...(block.headline ? { headline: block.headline } : {}),
        },
      };
    case "timeline":
      return {
        ...base,
        kind: "plan",
        checkable: false,
        steps: block.stops.map((stop): PlanStepView => ({
          ...(stop.when ? { when: stop.when } : {}),
          title: stop.title,
          ...(stop.detail ? { detail: stop.detail } : {}),
          kind: "event",
        })),
      };
    case "checklist":
      return {
        ...base,
        kind: "plan",
        checkable: true,
        steps: block.items.map((item) => ({
          title: item.text,
          kind: "task" as const,
          ...(item.completed ? { done: true } : {}),
        })),
      };
    case "diagram":
      return { ...base, kind: "diagram", diagram: block.diagram };
    case "code":
      return {
        ...base,
        kind: "code",
        language: block.language,
        text: block.text,
        notes: block.notes,
      };
    case "document":
      return { ...base, kind: "document", content: block.content };
    default:
      return null;
  }
}

type Cell = CellView["value"] | undefined;
/** A legacy matrix cell as a sheet's typed cell reads it. */
function cellText(value: Cell, scale?: number): string {
  if (!value) return "";
  switch (value.kind) {
    case "text":
      return value.text;
    case "measurement":
      return value.value;
    case "money":
      return `${value.amount} ${value.currency}`.trim();
    case "rating":
      return `${value.value}/${scale ?? 5}`;
    case "presence":
      return value.present ? "yes" : "no";
    case "unknown":
      return "";
  }
}

/**
 * A legacy run's reply: its title as the headline, the answer's lead as the
 * text, its first short list as points, stats as figures; the rest of the
 * answer reads in the opened view. Never a Markdown wall on the canvas.
 */
function legacyReply(id: string, board: Board, live: boolean, pending: string): ReplyView | null {
  const prose = board.blocks.filter((block) => block.kind === "prose" && block.state !== "pending");
  const body: DocumentNodeView[] = [
    ...(board.more?.blocks ?? []),
    ...prose.flatMap((block) => (block.kind === "prose" ? block.blocks : [])),
  ];
  const paragraphs = body.filter((node) => node.type === "paragraph").map(plain);
  const list = body.find((node) => node.type === "bulletList" || node.type === "orderedList");
  const items = (list?.content ?? []).map((item) => plain(item).trim()).filter(Boolean);
  const points =
    items.length && items.every((item) => item.length <= REPLY.point)
      ? items.slice(0, REPLY.points)
      : [];
  const figures = board.blocks.flatMap((block) =>
    block.kind === "stat"
      ? [{ label: block.label, value: `${block.value}${block.unit ? ` ${block.unit}` : ""}` }]
      : [],
  );
  const callouts = board.blocks.flatMap((block) =>
    block.kind === "callout" ? [within(block.text, REPLY.point)] : [],
  );
  const lead = board.lead || paragraphs[0] || "";
  const headline = board.title || (board.lead ? "" : within(lead, 80));
  const said = (part: string) => same(part, headline);
  const text = within(
    [board.title ? lead : "", ...paragraphs.slice(board.lead ? 0 : 1, 2)]
      .filter((part, index, all) => part && !said(part) && all.indexOf(part) === index)
      .join(" "),
    REPLY.text,
  );
  if (!headline && !text) {
    if (!live) return null;
    return {
      id,
      kind: "reply",
      state: "pending",
      headline: pending,
      text: "",
      figures: [],
      points: [],
      sources: board.sources,
    };
  }
  return {
    id,
    kind: "reply",
    headline: headline || within(text, 80),
    text: headline ? text : "",
    figures: figures.slice(0, REPLY.figures),
    points: [...points, ...callouts].slice(0, REPLY.points),
    ...(body.length ? { more: body } : {}),
    sources: board.sources,
  };
}

/** A lead object from its wire data. */
function leadView(
  id: string,
  artifact: WorkArtifactV1,
  execution: WorkExecutionFact,
): ObjectView | null {
  const evidence = artifactView(artifact, execution).evidence;
  const sources: Record<string, EvidenceReference> = Object.fromEntries(
    evidence.map((reference) => [reference.key, reference]),
  );
  const keyOf = (index: number | null | undefined) =>
    index === null || index === undefined ? undefined : evidence[index]?.key;
  const cited = (index: number | null | undefined) => {
    const key = keyOf(index);
    return key ? { sources: [key] } : {};
  };
  const base = { id, ...(artifact.title ? { title: artifact.title } : {}), sources };
  const data = artifact.data;
  switch (data.kind) {
    case "reply":
      return {
        ...base,
        kind: "reply",
        headline: data.headline,
        text: data.text,
        figures: (data.figures ?? []).map((figure) => ({
          label: figure.label,
          value: figure.value,
          ...(figure.note ? { note: figure.note } : {}),
        })),
        points: data.points ?? [],
      };
    case "picks":
      return {
        ...base,
        kind: "picks",
        facet: data.facet,
        items: data.items.map((item): PickView => {
          const amount = numberOf(item.price?.amount);
          const rating = numberOf(item.rating?.value);
          return {
            name: item.name,
            ...(item.subtitle ? { subtitle: item.subtitle } : {}),
            ...(item.logo_host ? { logo: item.logo_host } : {}),
            ...(item.url ? { url: item.url } : {}),
            ...(item.price
              ? {
                  price: {
                    display: item.price.display,
                    ...(amount !== undefined ? { amount } : {}),
                    ...(item.price.currency ? { currency: item.price.currency } : {}),
                  },
                }
              : {}),
            facts: (item.facts ?? []).map((fact) => ({ ...fact })),
            ...(rating !== undefined && item.rating
              ? {
                  rating: {
                    value: rating,
                    max: item.rating.max === 10 ? 10 : 5,
                    ...(item.rating.count ? { count: item.rating.count } : {}),
                  },
                }
              : {}),
            ...(item.why ? { why: item.why } : {}),
            tags: item.tags ?? [],
            recommended: !!item.recommended,
            ...(item.route
              ? {
                  route: {
                    from: item.route.from,
                    to: item.route.to,
                    ...(item.route.depart ? { depart: item.route.depart } : {}),
                    ...(item.route.arrive ? { arrive: item.route.arrive } : {}),
                    ...(item.route.duration ? { duration: item.route.duration } : {}),
                    stops: item.route.stops,
                    ...(item.route.carrier ? { carrier: item.route.carrier } : {}),
                    ...(item.route.carrier_host ? { carrierHost: item.route.carrier_host } : {}),
                  },
                }
              : {}),
            ...(item.when ? { when: item.when } : {}),
            ...(item.duration ? { duration: item.duration } : {}),
            ...(keyOf(item.source) ? { sources: [keyOf(item.source)!] } : {}),
          };
        }),
      };
    case "plan":
      return {
        ...base,
        kind: "plan",
        checkable: !!data.checkable,
        ...(data.total ? { total: { label: data.total.label, value: data.total.value } } : {}),
        steps: data.steps.map((step) => ({
          ...(step.when ? { when: step.when } : {}),
          title: step.title,
          ...(step.detail ? { detail: step.detail } : {}),
          kind: step.kind,
          ...(step.cost ? { cost: step.cost } : {}),
          ...(step.place ? { place: step.place } : {}),
          ...pickLink(step.pick, execution),
        })),
      };
    case "list":
      return {
        ...base,
        kind: "list",
        style: data.style,
        items: data.items.map((item) => ({
          title: item.title,
          ...(item.detail ? { detail: item.detail } : {}),
          ...(item.due ? { due: item.due } : {}),
          ...(item.priority ? { priority: item.priority } : {}),
          ...(item.from
            ? {
                from: Object.fromEntries(
                  Object.entries(item.from).filter(
                    ([, value]) => value !== null && value !== undefined,
                  ),
                ),
              }
            : {}),
        })),
      };
    case "sheet":
      return {
        ...base,
        kind: "sheet",
        columns: data.columns.map((column) => ({
          label: column.label,
          kind: column.kind,
          ...(column.unit ? { unit: column.unit } : {}),
          ...(column.currency ? { currency: column.currency } : {}),
          ...(column.best ? { best: column.best } : {}),
        })),
        rows: data.rows.map((row) => ({
          cells: row.cells,
          ...(row.entity?.logo_host ? { entity: { logo: row.entity.logo_host } } : {}),
          ...cited(row.source),
        })),
        ...(data.note ? { note: data.note } : {}),
      };
    case "plot": {
      const spec: ChartSpec = {
        ...styleSpec(data.style),
        series: data.series.map((series) => ({
          name: series.name,
          points: series.points.map((point) => {
            const y = numberOf(point.y) ?? null;
            const y2 = numberOf(point.y2);
            return { x: point.x, y, ...(y2 !== undefined ? { y2 } : {}) };
          }),
        })),
        x: { ...(data.x.label ? { label: data.x.label } : {}), kind: data.x.kind },
        y: {
          ...(data.y.label ? { label: data.y.label } : {}),
          ...(data.y.unit ? { unit: data.y.unit } : {}),
          format: data.y.format,
          ...(data.y.currency ? { currency: data.y.currency } : {}),
        },
        basis: data.basis,
        ...(data.knowledge ? { knowledge: true } : {}),
        ...(data.headline
          ? { headline: { label: data.headline.label, value: data.headline.value } }
          : {}),
      };
      return { ...base, kind: "plot", spec };
    }
    case "diff":
      return {
        ...base,
        kind: "diff",
        path: data.path,
        language: data.language,
        summary: data.summary,
        hunks: data.hunks.map((hunk) => ({
          oldStart: hunk.old_start,
          newStart: hunk.new_start,
          lines: hunk.lines.map((line) => ({ op: line.op, text: line.text })),
        })),
      };
    case "draft":
      return {
        ...base,
        kind: "draft",
        destination: data.destination,
        ...(data.to ? { to: data.to } : {}),
        ...(data.subject ? { subject: data.subject } : {}),
        body: data.body,
        ...(data.target_url ? { targetUrl: data.target_url } : {}),
        send: "draft",
      };
    case "media":
      return {
        ...base,
        kind: "media",
        media: data.medium,
        url: data.url,
        ...(data.provider ? { provider: data.provider } : {}),
        ...(data.poster ? { poster: { src: data.poster } } : {}),
        ...(data.duration ? { duration: data.duration } : {}),
        ...(data.start_secs !== null && data.start_secs !== undefined
          ? { startSecs: data.start_secs }
          : {}),
      };
    default:
      return null;
  }
}

/** A plan step's pick, drawn as a small thumbnail beside it. */
function pickLink(
  pick: { artifact: string; index: number } | null | undefined,
  execution: WorkExecutionFact,
): { pick?: PlanStepView["pick"] } {
  if (!pick) return {};
  const artifact = execution.artifacts.find((entry) => entry.id === pick.artifact);
  const item = artifact?.data.kind === "picks" ? artifact.data.items[pick.index] : undefined;
  return item
    ? { pick: { name: item.name, ...(item.logo_host ? { logo: item.logo_host } : {}) } }
    : {};
}

export type ObjectsInput = {
  board: Board;
  /** The head's id: the reply stands there. */
  head: string;
  live: boolean;
  pending: string;
  /** Lead objects placed on the canvas, by element, newest version resolved. */
  lead: readonly {
    element: string;
    artifact: WorkArtifactV1;
    execution: WorkExecutionFact;
    updated?: string;
  }[];
};

const HERO = new Set(["diagram", "picks", "sheet", "plan"]);

/**
 * A run's objects: the reply that heads its result, then everything else it
 * made, legacy blocks turned into the objects they are now.
 */
export function runObjects(input: ObjectsInput): { reply?: RunObject; objects: RunObject[] } {
  const objects: RunObject[] = [];
  let reply: RunObject | undefined;
  for (const entry of input.lead) {
    const view = leadView(entry.element, entry.artifact, entry.execution);
    if (!view) continue;
    const shown = entry.updated ? { ...view, updated: entry.updated } : view;
    // The reply stands as its own element, so opening it opens what the agent wrote.
    if (shown.kind === "reply" && !reply) {
      reply = { id: entry.element, view: shown, emphasis: "hero" };
      continue;
    }
    objects.push({
      id: entry.element,
      view: shown,
      emphasis: HERO.has(shown.kind) ? "hero" : "primary",
      ...(entry.artifact.part ? { part: entry.artifact.part } : {}),
    });
  }
  if (!reply) {
    const answer = input.board.blocks.find(
      (block) => block.kind === "prose" && block.state === "ready" && !block.id.endsWith(":facts"),
    );
    const id = answer?.id ?? input.head;
    const view = legacyReply(id, input.board, input.live, input.pending);
    if (view) reply = { id, view, emphasis: "hero" };
  }
  for (const block of input.board.blocks) {
    if (block.kind === "prose" || block.kind === "stat" || block.kind === "callout") continue;
    const view = legacyView(block, input.board);
    if (!view) continue;
    objects.push({ id: block.id, view, block, emphasis: block.emphasis });
  }
  // The reply says the title once: an object named the same goes without its name.
  const headline = reply?.view.kind === "reply" ? reply.view.headline : "";
  const named = objects.map((object) => {
    if (!object.view.title || !same(object.view.title, headline)) return object;
    const { title, ...view } = object.view;
    return { ...object, view: view as ObjectView, name: title };
  });
  return { ...(reply ? { reply } : {}), objects: named };
}
