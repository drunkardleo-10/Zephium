import { getContext, setContext } from "svelte";

/**
 * shadcn-svelte's chart config, vendored: each series or part by its key,
 * with the label people read and the colour it is drawn in. The container
 * turns the colours into `--color-<key>`, and every mark, the tooltip and
 * the legend read them from there.
 */
export type ChartConfig = Record<string, { label: string; color: string }>;

type ChartContextValue = { readonly config: ChartConfig };

const KEY = Symbol("chart");

export function setChartContext(value: ChartContextValue) {
  setContext(KEY, value);
}

export function useChart(): ChartContextValue {
  return getContext<ChartContextValue>(KEY);
}

/** The colour a key is drawn in, as the container publishes it. */
export const colorOf = (key: string) => `var(--color-${key})`;
