import type { WorkExecutionFact, WorkPageV1 } from "$shared/ipc/bindings";
import { pageFrameUrl } from "$domain/resources";
import type { PageGroup } from "../project-environment-stage";
import type { PickFrom, PicksView } from "../board/types";
import { pageName } from "./sources";

/** An address as one page: no fragment, no trailing slash, the query kept. */
function samePage(url: string): string {
  try {
    const parsed = new URL(url);
    parsed.hash = "";
    return `${parsed.host.replace(/^www\./u, "")}${parsed.pathname.replace(/\/$/u, "")}${parsed.search}`;
  } catch {
    return url;
  }
}

/**
 * The read step an extraction came from: a read lists what it made, and what
 * it made cites the extraction it was drawn from.
 */
function readOf(runs: readonly WorkExecutionFact[], extraction: string): string | null {
  for (const run of runs) {
    const made = new Set(
      run.artifacts.flatMap((artifact) =>
        artifact.evidence?.some((link) => link.extraction_id === extraction) ? [artifact.id] : [],
      ),
    );
    const read = (run.steps ?? []).find(
      (step) =>
        step.kind.kind === "read" && !!step.artifacts?.some((artifact) => made.has(artifact)),
    );
    if (read) return read.id;
  }
  return null;
}

/**
 * Where each thing a part found was taken from, among the part's own pages:
 * its own page when the part read it, else the page its cited extraction was
 * read on, else the page its citation names. Only told when it tells something: two things or more, from two
 * pages or more. Returns the set with each item's page, and the pages that
 * gave something, which the part no longer stacks.
 */
export function provenance(
  picks: PicksView,
  pages: readonly PageGroup[],
  runs: readonly WorkExecutionFact[],
  recorded: readonly WorkPageV1[],
): { view: PicksView; used: Set<string> } | null {
  if (picks.items.length < 2 || !pages.length) return null;
  const byStep = new Map(pages.flatMap((group) => group.steps.map((step) => [step.id, group])));
  const byUrl = new Map(pages.map((group) => [samePage(group.url), group]));
  const found = picks.items.map((item) => {
    const own = item.url ? byUrl.get(samePage(item.url)) : undefined;
    if (own) return own;
    for (const key of item.sources ?? []) {
      const extraction = key.slice(0, key.lastIndexOf(":"));
      const step = extraction ? readOf(runs, extraction) : null;
      const group = step ? byStep.get(step) : undefined;
      if (group) return group;
      const url = picks.sources?.[key]?.url;
      const cited = url ? byUrl.get(samePage(url)) : undefined;
      if (cited) return cited;
    }
    return undefined;
  });
  const used = new Set(found.flatMap((group) => (group ? [group.id] : [])));
  if (used.size < 2) return null;
  const fromOf = (group: PageGroup): PickFrom => {
    const page =
      group.page ??
      recorded.find((entry) => group.steps.some((step) => step.id === entry.step) && entry.frame);
    return {
      page: group.id,
      url: group.url,
      title: pageName(runs, group.url, group.steps.at(-1)?.id),
      frame: page?.frame ? pageFrameUrl(page.attempt, page.step, page.frame.generation) : null,
    };
  };
  return {
    view: {
      ...picks,
      items: picks.items.map((item, index) => {
        const group = found[index];
        return group ? { ...item, from: fromOf(group) } : item;
      }),
    },
    used,
  };
}
