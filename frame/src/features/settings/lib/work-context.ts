const DAY = 86_400_000;

/** "today", "yesterday", "3 days ago", then a date: how long ago, the way people say it. */
export function sinceLabel(ms: string | null | undefined, now = Date.now()): string {
  const at = Number(ms);
  if (!ms || !Number.isFinite(at)) return "";
  const days = Math.floor(now / DAY) - Math.floor(at / DAY);
  if (days < 7)
    return new Intl.RelativeTimeFormat(undefined, { numeric: "auto" }).format(-days, "day");
  return new Intl.DateTimeFormat(undefined, {
    month: "short",
    day: "numeric",
    ...(new Date(at).getFullYear() === new Date(now).getFullYear() ? {} : { year: "numeric" }),
  }).format(at);
}

export type SkillFields = {
  name: string;
  description: string;
  tools: string;
  role: "" | "lead" | "page" | "light";
  body: string;
};

/** A `SKILL.md` split into its fields, as Rust reads it; fields it doesn't know are dropped. */
export function parseSkill(text: string): SkillFields {
  const fields: SkillFields = { name: "", description: "", tools: "", role: "", body: text.trim() };
  const match = /^\uFEFF?---\r?\n([\s\S]*?)\r?\n---\r?\n?([\s\S]*)$/u.exec(text);
  if (!match) return fields;
  for (const line of match[1]!.split(/\r?\n/u)) {
    const at = line.indexOf(":");
    if (at < 0) continue;
    const key = line.slice(0, at).trim();
    const value = line
      .slice(at + 1)
      .trim()
      .replace(/^"(.*)"$/u, "$1");
    if (key === "name") fields.name = value;
    else if (key === "description") fields.description = value;
    else if (key === "tools") fields.tools = value.replace(/^\[|\]$/gu, "").trim();
    else if (key === "role" && ["lead", "page", "light"].includes(value))
      fields.role = value as SkillFields["role"];
  }
  fields.body = match[2]!.trim();
  return fields;
}

/** The fields back as `SKILL.md` text. */
export function renderSkill(fields: SkillFields): string {
  const tools = fields.tools
    .split(",")
    .map((tool) => tool.trim())
    .filter(Boolean);
  return [
    "---",
    `name: ${fields.name.trim()}`,
    `description: ${fields.description.trim().replace(/\s+/gu, " ")}`,
    ...(tools.length ? [`tools: [${tools.join(", ")}]`] : []),
    ...(fields.role ? [`role: ${fields.role}`] : []),
    "---",
    "",
    fields.body.trim(),
    "",
  ].join("\n");
}

/** A skill's name as a person reads it: `trip-planning` is "Trip planning". */
export function skillTitle(name: string): string {
  const words = name.replace(/-+/gu, " ").trim();
  return words ? words.charAt(0).toUpperCase() + words.slice(1) : name;
}

/** What a name typed as a title becomes: lowercase words joined by hyphens. */
export function skillName(title: string): string {
  return title
    .normalize("NFKD")
    .replace(/[\u0300-\u036f]/gu, "")
    .toLowerCase()
    .replace(/\u0142/gu, "l")
    .replace(/\u00f8/gu, "o")
    .replace(/\u00df/gu, "ss")
    .replace(/[^a-z0-9]+/gu, "-")
    .replace(/^-+|-+$/gu, "")
    .slice(0, 40)
    .replace(/-+$/u, "");
}
