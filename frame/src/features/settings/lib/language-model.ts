/** English display names; selection is a UI draft, never a locale activation. */
const codes =
  "aa ab ae af ak am an ar as av ay az ba be bg bh bi bm bn bo br bs ca ce ch co cr cs cu cv cy da de dv dz ee el en eo es et eu fa ff fi fj fo fr fy ga gd gl gn gu gv ha he hi ho hr ht hu hy hz ia id ie ig ii ik io is it iu ja jv ka kg ki kj kk kl km kn ko kr ks ku kv kw ky la lb lg li ln lo lt lu lv mg mh mi mk ml mn mr ms mt my na nb nd ne ng nl nn no nr nv ny oc oj om or os pa pi pl ps pt qu rm rn ro ru rw sa sc sd se sg si sk sl sm sn so sq sr ss st su sv sw ta te tg th ti tk tl tn to tr ts tt tw ty ug uk ur uz ve vi vo wa wo xh yi yo za zh zu".split(
    " ",
  );
const names = new Intl.DisplayNames(["en"], { type: "language" });
const variants = [
  "en-US",
  "en-GB",
  "pt-BR",
  "pt-PT",
  "zh-Hans",
  "zh-Hant",
  "zh-CN",
  "zh-TW",
  "es-419",
];
export const languages = [...new Set([...codes, ...variants])]
  .map((code) => ({ code, name: names.of(code) ?? code }))
  .sort((a, b) => a.name.localeCompare(b.name, "en"));
const known = new Set<string>(languages.map((language) => language.code));
export function normalizeLanguages(values: readonly string[]): string[] {
  const valid = [...new Set(values.filter((value) => known.has(value)))].slice(0, 20);
  return valid.length ? valid : ["en-US"];
}
export function readLanguages(value: string): string[] {
  if (value.length > 1000) return ["en-US"];
  try {
    const parsed: unknown = JSON.parse(value);
    return Array.isArray(parsed) && parsed.every((value) => typeof value === "string")
      ? normalizeLanguages(parsed)
      : ["en-US"];
  } catch {
    return ["en-US"];
  }
}
export function moveLanguage(values: readonly string[], code: string, offset: -1 | 1): string[] {
  const next = normalizeLanguages(values);
  const index = next.indexOf(code);
  const target = index + offset;
  if (index < 0 || target < 0 || target >= next.length) return next;
  [next[index], next[target]] = [next[target]!, next[index]!];
  return next;
}
