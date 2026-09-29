/**
 * The working indicators: dot spheres drawn once into a strip of frames and
 * shown one frame at a time, so a running indicator costs one composited
 * layer sliding by whole frames and never a paint.
 */

export type OrbKind = "thinking" | "searching" | "reading" | "planning" | "working" | "waiting";

type Point = readonly [number, number, number];
type Dot = { x: number; y: number; r: number; a: number };
type Line = { x1: number; y1: number; x2: number; y2: number; a: number };
type Frame = { dots: Dot[]; lines: Line[] };

type OrbStrip = { frames: number; fps: number; mask: string; loops: number };

const TAU = Math.PI * 2;
const rad = (degrees: number) => (degrees * Math.PI) / 180;

function rotateX([x, y, z]: Point, angle: number): Point {
  const c = Math.cos(angle);
  const s = Math.sin(angle);
  return [x, y * c - z * s, y * s + z * c];
}

function rotateY([x, y, z]: Point, angle: number): Point {
  const c = Math.cos(angle);
  const s = Math.sin(angle);
  return [x * c + z * s, y, -x * s + z * c];
}

function rotateZ([x, y, z]: Point, angle: number): Point {
  const c = Math.cos(angle);
  const s = Math.sin(angle);
  return [x * c - y * s, x * s + y * c, z];
}

/** Front dots are brighter and larger; the far side stays as a faint lattice. */
const depth = (z: number) => (z + 1) / 2;
const alphaAt = (z: number, back = 0.14) => back + (1 - back) * depth(z) ** 1.6;
const sizeAt = (z: number) => 0.62 + 0.38 * depth(z);

function latLong(lat: number, long: number): Point {
  const c = Math.cos(lat);
  return [c * Math.sin(long), -Math.sin(lat), c * Math.cos(long)];
}

function meridians(): Point[] {
  const points: Point[] = [];
  for (let m = 0; m < 12; m++)
    for (let k = 0; k <= 12; k++) points.push(latLong(rad(-78 + (156 * k) / 12), (TAU * m) / 12));
  return points;
}

function rings(): Point[] {
  const points: Point[] = [];
  for (const [lat, count] of [
    [-72, 6],
    [-48, 12],
    [-24, 18],
    [0, 24],
    [24, 18],
    [48, 12],
    [72, 6],
  ] as const)
    for (let k = 0; k < count; k++) points.push(latLong(rad(lat), (TAU * k) / count));
  return points;
}

const PHI = (1 + Math.sqrt(5)) / 2;
const ICOSAHEDRON: Point[] = (
  [
    [-1, PHI, 0],
    [1, PHI, 0],
    [-1, -PHI, 0],
    [1, -PHI, 0],
    [0, -1, PHI],
    [0, 1, PHI],
    [0, -1, -PHI],
    [0, 1, -PHI],
    [PHI, 0, -1],
    [PHI, 0, 1],
    [-PHI, 0, -1],
    [-PHI, 0, 1],
  ] as Point[]
).map(normalise);

function normalise([x, y, z]: Point): Point {
  const length = Math.hypot(x, y, z);
  return [x / length, y / length, z / length];
}

/** The icosahedron turned so one vertex is the pole: it repeats every fifth of a turn. */
function poled(points: readonly Point[]): Point[] {
  const angle = Math.atan2(1, PHI);
  return points.map((point) => rotateZ(point, angle));
}

function edges(points: readonly Point[]): [number, number][] {
  const pairs: [number, number][] = [];
  for (let i = 0; i < points.length; i++)
    for (let j = i + 1; j < points.length; j++) {
      const [a, b] = [points[i]!, points[j]!];
      if (Math.hypot(a[0] - b[0], a[1] - b[1], a[2] - b[2]) < 1.1) pairs.push([i, j]);
    }
  return pairs;
}

/** An evenly dotted sphere: the icosahedron's faces split in three, pushed out to the sphere. */
function geodesic(): Point[] {
  const base = poled(ICOSAHEDRON);
  const seen = new Map<string, Point>();
  const add = (point: Point) => {
    const unit = normalise(point);
    seen.set(unit.map((value) => value.toFixed(4)).join(","), unit);
  };
  const pairs = edges(base);
  const linked = new Set(pairs.map(([i, j]) => `${i}:${j}`));
  const joined = (i: number, j: number) => linked.has(`${Math.min(i, j)}:${Math.max(i, j)}`);
  for (let i = 0; i < base.length; i++)
    for (let j = i + 1; j < base.length; j++)
      for (let k = j + 1; k < base.length; k++) {
        if (!joined(i, j) || !joined(j, k) || !joined(i, k)) continue;
        const [a, b, c] = [base[i]!, base[j]!, base[k]!];
        for (let u = 0; u <= 3; u++)
          for (let v = 0; v <= 3 - u; v++) {
            const w = 3 - u - v;
            add([
              (a[0] * u + b[0] * v + c[0] * w) / 3,
              (a[1] * u + b[1] * v + c[1] * w) / 3,
              (a[2] * u + b[2] * v + c[2] * w) / 3,
            ]);
          }
      }
  return [...seen.values()];
}

type Spec = {
  frames: number;
  fps: number;
  loops: number;
  draw: (t: number) => {
    points: { p: Point; scale?: number; alpha?: number }[];
    links?: [Point, Point][];
  };
};

const MERIDIANS = meridians();
const RINGS = rings();
const POLED = poled(ICOSAHEDRON);
const LINKS = edges(POLED);
const GEODESIC = geodesic();

/** Turn about the vertical axis, then tip the axis toward the viewer and sideways. */
const view = (tilt: number, lean: number) => (p: Point, turn: number) =>
  rotateZ(rotateX(rotateY(p, turn), rad(tilt)), rad(lean));

const SPECS: Record<OrbKind, Spec> = {
  // Meridians dense enough to read as lines: a ball of thought turning on a leaning axis.
  thinking: {
    frames: 12,
    fps: 20,
    loops: Infinity,
    draw: (t) => {
      const at = view(-18, -16);
      return { points: MERIDIANS.map((p) => ({ p: at(p, (t * TAU) / 12), scale: 0.72 })) };
    },
  },
  // A globe of latitude rings turning under the eye: looking everywhere.
  searching: {
    frames: 16,
    fps: 20,
    loops: Infinity,
    draw: (t) => {
      const at = view(-22, 0);
      return { points: RINGS.map((p) => ({ p: at(p, (t * TAU) / 6) })) };
    },
  },
  // Rows of dots inside the disc, lit one after another from left to right like lines being read.
  reading: {
    frames: 40,
    fps: 16,
    loops: Infinity,
    draw: (t) => {
      const rows = [-0.56, -0.28, 0, 0.28, 0.56];
      const head = t * rows.length;
      const points: { p: Point; scale: number; alpha: number }[] = [];
      rows.forEach((y, row) => {
        const half = Math.sqrt(1 - y * y) * 0.92;
        const count = Math.max(3, Math.round((half * 2) / 0.19));
        for (let k = 0; k < count; k++) {
          const x = -half + (2 * half * k) / (count - 1);
          const along = row + (k + 0.5) / count;
          const gap = head - along;
          const lit = gap >= 0 && gap < 0.34 ? 1 - gap / 0.34 : 0;
          const read = gap >= 0 ? 0.5 : 0.2;
          const z = Math.sqrt(Math.max(0, 1 - x * x - y * y));
          points.push({
            p: [x, y, z],
            scale: 0.8 + 0.55 * lit,
            alpha: Math.min(1, read + 0.6 * lit) * (0.55 + 0.45 * z),
          });
        }
      });
      return { points };
    },
  },
  // A constellation that holds its shape as it turns: parts joined into a plan.
  planning: {
    frames: 18,
    fps: 15,
    loops: Infinity,
    draw: (t) => {
      const at = view(-24, -12);
      const points = POLED.map((p) => at(p, (t * TAU) / 5));
      return {
        points: points.map((p) => ({ p, scale: 1.25 })),
        links: LINKS.map(([i, j]) => [points[i]!, points[j]!] as [Point, Point]),
      };
    },
  },
  // An evenly dotted sphere, turning steadily: at work.
  working: {
    frames: 18,
    fps: 15,
    loops: Infinity,
    draw: (t) => {
      const at = view(-20, -14);
      return { points: GEODESIC.map((p) => ({ p: at(p, (t * TAU) / 5), scale: 0.86 })) };
    },
  },
  // Still rings that breathe from the centre out: listening, a few times, then at rest.
  waiting: {
    frames: 24,
    fps: 12,
    loops: 4,
    draw: (t) => {
      const at = view(-22, 0);
      return {
        points: RINGS.map((p) => {
          const q = at(p, rad(15));
          const wave = Math.sin(TAU * (t - Math.hypot(q[0], q[1]) * 0.55));
          return { p: q, scale: 1 + 0.22 * wave, alpha: (0.72 + 0.28 * wave) * alphaAt(q[2], 0.3) };
        }),
      };
    },
  },
};

function frame(spec: Spec, t: number, size: number): Frame {
  const { points, links = [] } = spec.draw(t);
  const radius = size * 0.42;
  const centre = size / 2;
  const dot = Math.max(0.55, size * 0.021);
  const place = (p: Point) => [centre + p[0] * radius, centre + p[1] * radius] as const;
  const dots = points
    .map(({ p, scale = 1, alpha }) => {
      const [x, y] = place(p);
      return { x, y, r: dot * scale * sizeAt(p[2]), a: alpha ?? alphaAt(p[2]), z: p[2] };
    })
    .sort((a, b) => a.z - b.z);
  const lines = links.map(([a, b]) => {
    const [x1, y1] = place(a);
    const [x2, y2] = place(b);
    return { x1, y1, x2, y2, a: 0.1 + 0.42 * depth((a[2] + b[2]) / 2) ** 2 };
  });
  return { dots, lines };
}

const n = (value: number) => Number(value.toFixed(2));

function svg(spec: Spec, size: number): string {
  const stroke = n(Math.max(0.4, size * 0.012));
  const parts: string[] = [];
  for (let i = 0; i < spec.frames; i++) {
    const { dots, lines } = frame(spec, i / spec.frames, size);
    parts.push(`<g transform='translate(${i * size} 0)'>`);
    for (const line of lines)
      parts.push(
        `<line x1='${n(line.x1)}' y1='${n(line.y1)}' x2='${n(line.x2)}' y2='${n(line.y2)}' stroke='white' stroke-width='${stroke}' stroke-opacity='${n(line.a)}'/>`,
      );
    for (const dot of dots)
      parts.push(
        `<circle cx='${n(dot.x)}' cy='${n(dot.y)}' r='${n(dot.r)}' fill-opacity='${n(Math.min(1, dot.a))}'/>`,
      );
    parts.push("</g>");
  }
  const width = size * spec.frames;
  return `<svg xmlns='http://www.w3.org/2000/svg' width='${width}' height='${size}' viewBox='0 0 ${width} ${size}' fill='white'>${parts.join("")}</svg>`;
}

const strips = new Map<string, OrbStrip>();

/** The strip for one indicator at one size, drawn once per document and reused by every copy. */
export function orbStrip(kind: OrbKind, size: number): OrbStrip {
  const key = `${kind}:${size}`;
  let strip = strips.get(key);
  if (!strip) {
    const spec = SPECS[kind];
    strip = {
      frames: spec.frames,
      fps: spec.fps,
      loops: spec.loops,
      mask: `url("data:image/svg+xml,${encodeURIComponent(svg(spec, size))}")`,
    };
    strips.set(key, strip);
  }
  return strip;
}

/** Whole-frame holds: the strip jumps a frame at a time and never slides between two. */
export function orbKeyframes(frames: number, size: number): Keyframe[] {
  const keys: Keyframe[] = [];
  for (let i = 0; i < frames; i++) {
    const transform = `translateX(${-i * size}px)`;
    keys.push({ offset: i / frames, transform });
    keys.push({ offset: Math.min(1, (i + 1) / frames - 1e-4), transform });
  }
  keys.push({ offset: 1, transform: `translateX(${-(frames - 1) * size}px)` });
  return keys;
}
