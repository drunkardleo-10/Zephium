export type BundleLimit = { js: number; css: number };
/** Sizes cover the transitive static graph, including shared chunks and CSS. */
export function checkBundleBudget(
  name: string,
  actual: { staticJsBytes: number; staticCssBytes: number },
  limits: Record<string, BundleLimit>,
): string | null {
  const limit = Object.hasOwn(limits, name) ? limits[name] : undefined;
  if (!limit)
    return `Unbudgeted entry ${name}; measured ${actual.staticJsBytes} JS bytes and ${actual.staticCssBytes} CSS bytes; review its startup cost`;
  for (const [kind, size, maximum] of [
    ["JS", actual.staticJsBytes, limit.js],
    ["CSS", actual.staticCssBytes, limit.css],
  ] as const) {
    if (
      !Number.isSafeInteger(maximum) ||
      maximum < 0 ||
      !Number.isSafeInteger(size) ||
      size < 0 ||
      size > maximum
    )
      return `${name} ${kind} budget exceeded: ${size} bytes (limit ${maximum})`;
  }
  return null;
}
