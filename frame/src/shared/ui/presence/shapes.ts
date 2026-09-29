/** The helpers' silhouettes, as masks over one lit body: one family, a shape per kind. */

export type CharacterKind = "lead" | "browser" | "research" | "computer" | "connection";

const n = (value: number) => Number(value.toFixed(2));

function squircle(power: number, rx: number, ry: number): string {
  const points: string[] = [];
  for (let i = 0; i < 72; i++) {
    const t = (i / 72) * Math.PI * 2;
    const c = Math.cos(t);
    const s = Math.sin(t);
    const x = 50 + rx * Math.sign(c) * Math.abs(c) ** (2 / power);
    const y = 50 + ry * Math.sign(s) * Math.abs(s) ** (2 / power);
    points.push(`${n(x)} ${n(y)}`);
  }
  return `<path d='M${points.join("L")}Z'/>`;
}

function roundedPolygon(sides: number, radius: number, squash: number, soften: number): string {
  const corners = Array.from({ length: sides }, (_, i) => {
    const t = (i / sides) * Math.PI * 2;
    return [50 + radius * Math.cos(t), 50 + radius * squash * Math.sin(t)] as const;
  });
  const toward = (a: readonly [number, number], b: readonly [number, number]) =>
    `${n(a[0] + (b[0] - a[0]) * soften)} ${n(a[1] + (b[1] - a[1]) * soften)}`;
  let path = "";
  corners.forEach((corner, i) => {
    const previous = corners[(i + sides - 1) % sides]!;
    const next = corners[(i + 1) % sides]!;
    path += `${i ? "L" : "M"}${toward(corner, previous)}Q${n(corner[0])} ${n(corner[1])} ${toward(corner, next)}`;
  });
  return `<path d='${path}Z'/>`;
}

const clover = [
  [50, 29],
  [71, 50],
  [50, 71],
  [29, 50],
]
  .map(([x, y]) => `<circle cx='${x}' cy='${y}' r='25'/>`)
  .concat("<circle cx='50' cy='50' r='30'/>")
  .join("");

const SHAPES: Record<Exclude<CharacterKind, "lead">, string> = {
  browser: squircle(4.4, 45, 45),
  research: roundedPolygon(6, 49, 0.9, 0.3),
  computer: "<rect x='3' y='15' width='94' height='70' rx='35'/>",
  connection: clover,
};

const masks = new Map<CharacterKind, string>();

/** A kind's silhouette as a mask image; the lead is a sphere and needs none. */
export function characterMask(kind: CharacterKind): string | undefined {
  if (kind === "lead") return undefined;
  let mask = masks.get(kind);
  if (!mask) {
    const svg = `<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 100 100' fill='white'>${SHAPES[kind]}</svg>`;
    mask = `url("data:image/svg+xml,${encodeURIComponent(svg)}")`;
    masks.set(kind, mask);
  }
  return mask;
}
