import type { WorkArtifactDataV1, WorkExecutionFact } from "$shared/ipc/bindings";

type Subject = {
  name: string;
  homepage?: string | null;
  image_candidates?: readonly string[] | null;
};
type Matrix = Extract<WorkArtifactDataV1, { kind: "comparison_matrix" }>;
export type SubjectFact = { label: string; value: string };

/** Subjects merge across artifacts by page identity, then by name. */
export function subjectKey(subject: Subject): string {
  const page = subject.homepage ? pageKey(subject.homepage) : null;
  return page ?? `name:${subject.name.trim().toLowerCase()}`;
}

function pageKey(homepage: string): string | null {
  try {
    const url = new URL(homepage);
    for (const key of [...url.searchParams.keys()])
      if (key.startsWith("utm_") || key === "ref" || key === "tag") url.searchParams.delete(key);
    url.hash = "";
    return `page:${url.host.toLowerCase()}${url.pathname.replace(/\/+$/, "")}${url.search}`;
  } catch {
    return null;
  }
}

function rows(execution: WorkExecutionFact, key: string): { matrix: Matrix; index: number }[] {
  const out: { matrix: Matrix; index: number }[] = [];
  for (const artifact of execution.artifacts) {
    const matrix = artifact.data;
    if (matrix.kind !== "comparison_matrix") continue;
    matrix.subjects.forEach((subject, index) => {
      if (subjectKey(subject) === key && matrix.cells[index]) out.push({ matrix, index });
    });
  }
  return out;
}

/** Everything the run has established about one subject, price first. */
export function subjectFacts(execution: WorkExecutionFact, subject: Subject): SubjectFact[] {
  const money: SubjectFact[] = [];
  const other: SubjectFact[] = [];
  const seen = new Set<string>();
  for (const { matrix, index } of rows(execution, subjectKey(subject))) {
    matrix.criteria.forEach((criterion, column) => {
      const cell = matrix.cells[index]?.[column];
      if (!cell || seen.has(criterion.name)) return;
      const value = cell.value;
      let text: string | null = null;
      switch (value.kind) {
        case "money":
          text = formatMoney(value.amount, value.currency);
          break;
        case "text":
          text = value.text.trim() && !/^https?:\/\//i.test(value.text) ? value.text.trim() : null;
          break;
        case "measurement":
          text =
            criterion.kind.kind === "measurement"
              ? `${value.value} ${criterion.kind.unit}`
              : value.value;
          break;
        case "rating":
          text =
            criterion.kind.kind === "rating"
              ? `${value.value}/${criterion.kind.scale_max}`
              : String(value.value);
          break;
        default:
          break;
      }
      if (!text) return;
      seen.add(criterion.name);
      (value.kind === "money" ? money : other).push({
        label: criterion.name,
        value: text.slice(0, 80),
      });
    });
  }
  return [...money, ...other].slice(0, 3);
}

/** Public image candidates the run has observed for one subject. */
export function subjectImageCandidates(execution: WorkExecutionFact, subject: Subject): string[] {
  const out: string[] = [];
  for (const { matrix, index } of rows(execution, subjectKey(subject)))
    for (const candidate of matrix.subjects[index]?.image_candidates ?? [])
      if (!out.includes(candidate)) out.push(candidate);
  return out;
}

function formatMoney(amount: string, currency: string): string {
  const value = Number(amount);
  if (!Number.isFinite(value)) return `${amount} ${currency}`;
  try {
    return new Intl.NumberFormat(undefined, { style: "currency", currency }).format(value);
  } catch {
    return `${amount} ${currency}`;
  }
}
