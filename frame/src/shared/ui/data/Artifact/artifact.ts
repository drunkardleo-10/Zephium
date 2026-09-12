import type { ChartSeries } from "../Chart";

/** Rendering inputs only. The runtime adapter owns wire validation, identities and permissions. */
export type EvidenceReference = { key: string; label: string };
export type ArtifactContent =
  | { kind: "document"; paragraphs: readonly string[] }
  | { kind: "table"; columns: readonly string[]; rows: readonly (readonly string[])[] }
  | {
      kind: "comparison";
      criteria: readonly string[];
      alternatives: readonly { name: string; values: readonly string[] }[];
    }
  | { kind: "chart"; xLabel: string; yLabel: string; series: readonly ChartSeries[] }
  | { kind: "checklist"; items: readonly { text: string; completed: boolean }[] }
  | { kind: "sources"; summary: string }
  | { kind: "browser"; title: string; location: string; summary: string }
  | { kind: "unavailable"; reason: string };
export type ArtifactView = {
  key: string;
  title: string;
  content: ArtifactContent;
  reviewLabel: string;
  evidence: readonly EvidenceReference[];
};

// Renderer ceilings, not runtime admission rules. Oversized inputs remain unavailable
// rather than silently presenting a truncated result as the complete artifact.
const text = (value: string) => typeof value === "string" && value.length <= 16_384;
const strings = (values: readonly string[], max: number) =>
  values.length <= max && values.every(text);
export function artifactRenderable(view: ArtifactView): boolean {
  if (
    view.key.length > 128 ||
    view.title.length > 512 ||
    !text(view.title) ||
    !text(view.reviewLabel) ||
    view.evidence.length > 128 ||
    new Set(view.evidence.map((item) => item.key)).size !== view.evidence.length ||
    !view.evidence.every((item) => item.key.length <= 128 && item.label.length <= 512)
  )
    return false;
  const data = view.content;
  switch (data.kind) {
    case "document":
      return (
        strings(data.paragraphs, 512) &&
        data.paragraphs.reduce((n, p) => n + p.length, 0) <= 262_144
      );
    case "table":
      return (
        strings(data.columns, 32) &&
        data.rows.length <= 10_000 &&
        data.rows.every((row) => row.length === data.columns.length && strings(row, 32)) &&
        data.rows.reduce((n, row) => n + row.reduce((s, cell) => s + cell.length, 0), 0) <=
          1_048_576
      );
    case "comparison":
      return (
        strings(data.criteria, 32) &&
        data.alternatives.length <= 100 &&
        data.alternatives.reduce(
          (n, row) =>
            n + row.name.length + row.values.reduce((total, cell) => total + cell.length, 0),
          0,
        ) <= 262_144 &&
        data.alternatives.every(
          (row) =>
            text(row.name) && row.values.length === data.criteria.length && strings(row.values, 32),
        )
      );
    case "chart":
      return (
        text(data.xLabel) &&
        text(data.yLabel) &&
        data.series.length <= 16 &&
        data.series.reduce(
          (n, series) =>
            n +
            series.points.reduce(
              (total, point) => total + point.label.length + point.value.length,
              0,
            ),
          0,
        ) <= 65_536 &&
        data.series.every(
          (series) =>
            text(series.name) &&
            series.points.length <= 256 &&
            series.points.every((point) => text(point.label) && text(point.value)),
        )
      );
    case "checklist":
      return (
        data.items.length <= 512 &&
        data.items.reduce((n, item) => n + item.text.length, 0) <= 262_144 &&
        data.items.every((item) => text(item.text) && typeof item.completed === "boolean")
      );
    case "sources":
      return text(data.summary);
    case "browser":
      return text(data.title) && text(data.location) && text(data.summary);
    case "unavailable":
      return text(data.reason);
    default:
      return false;
  }
}

/** Text only: a URL in an artifact is never a navigation or browser-context grant. */
export function displayLocation(value: string): string {
  try {
    const url = new URL(value);
    return ["https:", "http:"].includes(url.protocol) && !url.username && !url.password
      ? url.origin
      : "";
  } catch {
    return "";
  }
}
