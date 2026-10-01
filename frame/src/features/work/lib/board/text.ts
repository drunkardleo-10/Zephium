import type { DocumentNodeView } from "$shared/ui/data/Artifact";

/** A node's words, as a reader would say them. */
export function plain(node: DocumentNodeView): string {
  if (node.type === "text") return node.text ?? "";
  if (node.type === "hardBreak") return " ";
  return (node.content ?? []).map(plain).join(node.type === "listItem" ? " " : "");
}

/** The first sentence of a run of prose, and whatever follows it. */
export function splitLead(text: string): { lead: string; rest: string } {
  const clean = text.replace(/\s+/gu, " ").trim();
  const match = /^(.+?[.!?])(?:\s+|$)(.*)$/u.exec(clean);
  if (!match) return { lead: clean, rest: "" };
  return { lead: match[1]!.trim(), rest: match[2]!.trim() };
}

const STOP = new Set(
  (
    "about after also and any are because been before being between both but can could does " +
    "each for from have here into its just like more most much must not only other over same " +
    "should some such than that the their them then there these they this those through under " +
    "very was were what when where which while who will with would your you our out per via"
  ).split(" "),
);

/** The words that carry meaning, lower-cased: four letters or more, or any number. */
export function terms(text: string): Set<string> {
  const words = text
    .normalize("NFKC")
    .toLowerCase()
    .match(/[\p{L}\p{N}]+/gu);
  return new Set(
    (words ?? []).filter((word) => (word.length >= 4 || /\d/u.test(word)) && !STOP.has(word)),
  );
}

/** How many meaningful words two texts share. */
export function overlap(a: ReadonlySet<string>, b: ReadonlySet<string>): number {
  let count = 0;
  for (const word of a) if (b.has(word)) count += 1;
  return count;
}
