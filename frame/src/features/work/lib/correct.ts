import type { WorkArtifactDataV1, WorkCellValue, WorkCriterion } from "$shared/ipc/bindings";

const DECIMAL = /^[+-]?\d+(\.\d+)?$/u;
const YES = /^(yes|y|true|included|✓)$/iu;
const NO = /^(no|n|false|not included|none|✗)$/iu;

/** A person's correction keeps the cell's kind when the text still fits it, and
 * otherwise says what they wrote: plain text is admissible under any criterion. */
function corrected(
  previous: WorkCellValue,
  criterion: WorkCriterion,
  grounded: boolean,
  raw: string,
): WorkCellValue {
  const text = raw.trim();
  if (!text) return { kind: "unknown" };
  if (criterion.kind.kind === "presence") {
    if (YES.test(text)) return { kind: "presence", present: true };
    if (NO.test(text)) return { kind: "presence", present: false };
    return { kind: "text", text };
  }
  const bare = text.replace(/[^\d.+-]/gu, "");
  if (previous.kind === "money" && grounded && DECIMAL.test(bare))
    return {
      kind: "money",
      amount: bare,
      currency: previous.currency,
      ...(previous.observed_at ? { observed_at: previous.observed_at } : {}),
    };
  if (criterion.kind.kind === "measurement" && grounded && DECIMAL.test(bare))
    return { kind: "measurement", value: bare };
  if (criterion.kind.kind === "rating" && grounded) {
    const value = Number(bare);
    if (Number.isInteger(value) && value >= 0 && value <= criterion.kind.scale_max)
      return { kind: "rating", value };
  }
  return { kind: "text", text };
}

/** The same comparison with one cell corrected; anything out of range is refused. */
export function correctedMatrix(
  data: WorkArtifactDataV1,
  subject: number,
  criterion: number,
  text: string,
): WorkArtifactDataV1 | null {
  if (data.kind !== "comparison_matrix") return null;
  const row = data.cells[subject];
  const cell = row?.[criterion];
  const column = data.criteria[criterion];
  if (!row || !cell || !column) return null;
  const grounded = !!cell.evidence?.length || !!cell.general_knowledge;
  const value = corrected(cell.value, column, grounded, text);
  if (JSON.stringify(value) === JSON.stringify(cell.value)) return null;
  return {
    ...data,
    cells: data.cells.map((entries, index) =>
      index === subject
        ? entries.map((entry, position) => (position === criterion ? { ...entry, value } : entry))
        : entries,
    ),
  };
}

/** The same diagram with one part renamed; an unknown part or an unchanged name is refused. */
export function renamedPart(
  data: WorkArtifactDataV1,
  node: string,
  name: string,
): WorkArtifactDataV1 | null {
  const text = name.trim();
  if (data.kind !== "diagram" || !text || text.length > 256) return null;
  const part = data.nodes.find((entry) => entry.id === node);
  if (!part || part.name === text) return null;
  return {
    ...data,
    nodes: data.nodes.map((entry) => (entry === part ? { ...entry, name: text } : entry)),
  };
}
