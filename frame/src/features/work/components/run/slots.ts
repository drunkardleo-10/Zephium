import type { Component } from "svelte";
import type { Detail } from "../../lib/board/types";
import type { PartView } from "../../lib/canvas-model";

/**
 * What a helper's own view of its work receives in a part's row: the part,
 * the canvas's detail, and where its steps are (the facts come from the
 * `canvasWork` context).
 */
export type PartContentProps = {
  id: string;
  part: PartView;
  detail: Detail;
  objective: string;
  steps: readonly string[];
};

type Loader<P extends Record<string, unknown>> = () => Promise<{ default: Component<P> }>;
// Found at build time; a view that doesn't exist yet leaves its slot to the fallback.
const contents = import.meta.glob<{ default: Component<PartContentProps> }>([
  "../parts/ComputerPart.svelte",
  "../parts/ConnectionPart.svelte",
]);
const asks = import.meta.glob<{ default: Component<Record<string, unknown>> }>(
  "../asks/AskCard.svelte",
);

const CONTENT: Partial<Record<PartView["helper"], string>> = {
  computer: "../parts/ComputerPart.svelte",
  connection: "../parts/ConnectionPart.svelte",
};

export function partContent(helper: PartView["helper"]): Loader<PartContentProps> | null {
  const path = CONTENT[helper];
  return (path && contents[path]) || null;
}

/** The ask card, once its stream has built it; it takes the ask's own props. */
export function askCard(): Loader<Record<string, unknown>> | null {
  return asks["../asks/AskCard.svelte"] ?? null;
}
