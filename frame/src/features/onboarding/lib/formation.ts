export type Box = { x: number; y: number; w: number; h: number };

type Particle = {
  x: number;
  y: number;
  vx: number;
  vy: number;
  tx: number;
  ty: number;
  hx: Float32Array;
  hy: Float32Array;
  head: number;
  n: number;
  bright: boolean;
};

const COUNT = 1300;
const TRAIL = 8;
const BUCKETS = 4;
/** How long the air takes to settle into the letters. */
const CONVERGE = 2600;
/** How long the letters it formed take to clear once the name stands. */
const DISSOLVE = 1100;

/**
 * The name drawn out of the air: fine particles drift in from across the
 * stage and settle into the letters, then clear away as the solid name
 * takes their place. It runs once, a few seconds, and then nothing of it
 * is left running: the loop ends and the canvas is cleared.
 */
export function formName(
  canvas: HTMLCanvasElement,
  options: {
    stage: { width: number; height: number };
    /** Where the name stands, in stage pixels. */
    box: Box;
    /** The letters, as one path in the coordinates `place` maps from. */
    path: Path2D;
    place: (context: CanvasRenderingContext2D) => void;
    /** Device pixels per stage pixel. */
    density: number;
    color: string;
    onformed: () => void;
    ondone: () => void;
  },
): () => void {
  const { stage, density } = options;
  canvas.width = Math.round(stage.width * density);
  canvas.height = Math.round(stage.height * density);
  const context = canvas.getContext("2d");
  if (!context) {
    options.onformed();
    options.ondone();
    return () => {};
  }
  context.setTransform(density, 0, 0, density, 0, 0);

  const targets = sample(options);
  const particles: Particle[] = [];
  for (let index = 0; index < COUNT && targets.length > 0; index++) {
    const target = targets[(Math.random() * targets.length) | 0]!;
    const x = Math.random() * stage.width;
    const y = Math.random() * stage.height;
    particles.push({
      x,
      y,
      vx: 0,
      vy: 0,
      tx: target[0] + (Math.random() - 0.5) * 1.2,
      ty: target[1] + (Math.random() - 0.5) * 1.2,
      hx: new Float32Array(TRAIL).fill(x),
      hy: new Float32Array(TRAIL).fill(y),
      head: 0,
      n: 1,
      bright: Math.random() < 0.25,
    });
  }

  const [r, g, b] = rgb(options.color);
  let started = 0;
  let formedAt = 0;
  let frame = 0;
  let last = 0;
  let stopped = false;

  function step(now: number) {
    if (stopped) return;
    if (!started) started = last = now;
    const dt = Math.min(2, (now - last) / 16.67);
    last = now;
    const age = now - started;
    const settle = Math.min(1, age / CONVERGE);
    const fade = formedAt ? Math.max(0, 1 - (now - formedAt) / DISSOLVE) : 1;

    for (const particle of particles) {
      if (!formedAt) {
        // Pulled toward its place in the letters, harder as they settle,
        // with the air's own drift dying away as it arrives.
        const pull = 0.006 + settle * settle * 0.05;
        const damping = 0.86 - settle * 0.2;
        const angle = drift(particle.x, particle.y, age);
        particle.vx = (particle.vx + (particle.tx - particle.x) * pull) * damping;
        particle.vy = (particle.vy + (particle.ty - particle.y) * pull) * damping;
        particle.vx += Math.cos(angle) * 0.45 * (1 - settle);
        particle.vy += Math.sin(angle) * 0.45 * (1 - settle);
      } else {
        // Released, it lifts away on the air as it fades.
        const angle = drift(particle.x, particle.y, age);
        particle.vx += (Math.cos(angle) * 0.9 - particle.vx) * 0.04 * dt;
        particle.vy += (Math.sin(angle) * 0.9 - 0.25 - particle.vy) * 0.04 * dt;
      }
      particle.x += particle.vx * dt;
      particle.y += particle.vy * dt;
      particle.head = (particle.head + 1) % TRAIL;
      particle.hx[particle.head] = particle.x;
      particle.hy[particle.head] = particle.y;
      if (particle.n < TRAIL) particle.n++;
    }

    context!.clearRect(0, 0, stage.width, stage.height);
    context!.lineCap = "round";
    const base = (0.18 + settle * 0.5) * fade;
    for (const bright of [false, true]) {
      for (let bucket = 0; bucket < BUCKETS; bucket++) {
        const from = Math.floor((bucket * (TRAIL - 1)) / BUCKETS);
        const to = Math.floor(((bucket + 1) * (TRAIL - 1)) / BUCKETS);
        context!.beginPath();
        for (const particle of particles) {
          if (particle.bright !== bright) continue;
          for (let segment = from; segment < to && segment < particle.n - 1; segment++) {
            const a = (particle.head - segment + TRAIL) % TRAIL;
            const z = (particle.head - segment - 1 + TRAIL) % TRAIL;
            context!.moveTo(particle.hx[a]!, particle.hy[a]!);
            context!.lineTo(particle.hx[z]!, particle.hy[z]!);
          }
        }
        const tail = 1 - bucket / BUCKETS;
        const alpha = Math.min(0.95, base * tail * tail * (bright ? 1.5 : 0.8));
        context!.strokeStyle = `rgba(${r},${g},${b},${alpha.toFixed(3)})`;
        context!.lineWidth = bright ? 0.8 : 0.55;
        context!.stroke();
      }
    }

    if (!formedAt && settle >= 1) {
      formedAt = now;
      options.onformed();
    }
    if (formedAt && fade <= 0) {
      stop();
      options.ondone();
      return;
    }
    frame = requestAnimationFrame(step);
  }

  function stop() {
    stopped = true;
    cancelAnimationFrame(frame);
    context!.clearRect(0, 0, stage.width, stage.height);
  }

  frame = requestAnimationFrame(step);
  return stop;
}

/** A slow, curling field: enough that nothing travels in a straight line. */
function drift(x: number, y: number, t: number) {
  return (
    -0.15 +
    Math.sin(x * 0.0048 + t * 0.00011) * Math.cos(y * 0.0057 - t * 0.00008) * 0.9 +
    Math.sin((x - y) * 0.0031 + t * 0.00006) * 0.45
  );
}

/** Points inside the letters, every other pixel, in stage pixels. */
function sample(options: {
  box: Box;
  path: Path2D;
  place: (context: CanvasRenderingContext2D) => void;
}): [number, number][] {
  const { box } = options;
  const off = document.createElement("canvas");
  off.width = Math.ceil(box.w);
  off.height = Math.ceil(box.h);
  const context = off.getContext("2d");
  if (!context) return [];
  options.place(context);
  context.fill(options.path);
  const data = context.getImageData(0, 0, off.width, off.height).data;
  const points: [number, number][] = [];
  for (let y = 0; y < off.height; y += 2)
    for (let x = 0; x < off.width; x += 2)
      if (data[(y * off.width + x) * 4 + 3]! > 128) points.push([box.x + x, box.y + y]);
  return points;
}

/** A CSS colour as RGB, by letting the canvas normalise it. */
function rgb(color: string): [number, number, number] {
  const probe = document.createElement("canvas").getContext("2d");
  if (!probe) return [240, 240, 244];
  probe.fillStyle = color;
  const value = probe.fillStyle;
  if (value.startsWith("#") && value.length === 7)
    return [1, 3, 5].map((at) => parseInt(value.slice(at, at + 2), 16)) as [number, number, number];
  const parts = value.match(/\d+(\.\d+)?/gu)?.map(Number) ?? [240, 240, 244];
  return [parts[0] ?? 240, parts[1] ?? 240, parts[2] ?? 244];
}
