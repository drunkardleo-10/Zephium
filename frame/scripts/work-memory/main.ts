/**
 * The Work screen built as the app ships it (minified, split, no dev server),
 * over a fake native side that serves the QA profile's real works. `run.mjs`
 * drives it through `window.harness` and reads each WebKit process's footprint.
 */
import "$styles/global.css";
import { mount, unmount } from "svelte";
import type {
  WorkEnvironmentSnapshot,
  WorkEnvironmentSummary,
  WorkRuntimeProjection,
} from "$shared/ipc/bindings";

type Raw = {
  snapshot: WorkEnvironmentSnapshot;
  objectives: Record<string, WorkRuntimeProjection>;
  pages: { execution: string; attempt: string; step: string }[];
  media?: Record<string, { digest: string; mime: string }>;
};

const frames = new Map<string, string>();
const pictures = new Map<string, string>();
(globalThis as { __harnessMedia?: unknown }).__harnessMedia = { frames, pictures };

const works = new Map<string, WorkEnvironmentSnapshot>();
const objectives = new Map<string, WorkRuntimeProjection>();
const pages = new Map<string, unknown[]>();
const media = new Map<string, unknown>();
const summaries: WorkEnvironmentSummary[] = [];
let minted = 0;

const reply = (profile: string, body: unknown) => ({ version: 1, profile, reply: body });
function workCall(profile: string, call: { kind: string; request: Record<string, unknown> }) {
  const request = call.request as {
    kind: string;
    id?: string;
    command?: string;
    query?: { kind: string; work?: string };
    view?: object;
    expected?: string;
    intent?: { kind: string; id: string; edit: Record<string, unknown> };
  };
  if (call.kind === "environment") {
    if (request.kind === "list")
      return reply(profile, {
        kind: "environment",
        reply: { kind: "page", works: summaries, selected: null, next: null },
      });
    if ((request.kind === "open" || request.kind === "read") && works.has(request.id!))
      return reply(profile, {
        kind: "environment",
        reply: { kind: "snapshot", snapshot: works.get(request.id!) },
      });
    if (request.kind === "checkpoint" && works.has(request.id!)) {
      const applied = String(BigInt(request.expected!) + 1n);
      const snapshot = { ...works.get(request.id!)!, view: { ...request.view, revision: applied } };
      works.set(request.id!, snapshot as WorkEnvironmentSnapshot);
      return reply(profile, {
        kind: "environment",
        reply: {
          kind: "checkpointed",
          expected: request.expected,
          applied_view_revision: applied,
          replayed: false,
          snapshot,
        },
      });
    }
    const intent = request.intent;
    const current = intent ? works.get(intent.id) : undefined;
    if (request.kind === "command" && intent?.kind === "edit" && current) {
      const edit = intent.edit;
      if (edit.kind === "add" || edit.kind === "relate") {
        minted += 1;
        const id = `01M3ZZZZZZZZZZZZZZZZ${String(minted).padStart(6, "0")}`;
        const revision = String(BigInt(current.revision) + 1n);
        const snapshot = {
          ...current,
          revision,
          ...(edit.kind === "add"
            ? { elements: [...current.elements, { id, area: null, reference: edit.reference }] }
            : {
                relations: [
                  ...(current.relations ?? []),
                  {
                    id,
                    from: edit.from,
                    to: edit.to,
                    kind: edit.relation,
                    origin: { kind: "user" },
                  },
                ],
              }),
        } as WorkEnvironmentSnapshot;
        works.set(intent.id, snapshot);
        return reply(profile, {
          kind: "environment",
          reply: {
            kind: "applied",
            command: request.command,
            applied_revision: revision,
            applied_view_revision: snapshot.view.revision,
            replayed: false,
            snapshot,
          },
        });
      }
    }
  }
  if (call.kind === "query" && request.query?.kind === "projection") {
    const projection = objectives.get(request.query.work!);
    if (projection) return reply(profile, { kind: "projection", projection });
  }
  return reply(profile, { kind: "error", error: "not_found" });
}

const unknown = new Set<string>();
const callbacks = new Map<number, (value: unknown) => void>();
let callbackId = 0;
async function invoke(cmd: string, args: Record<string, never>): Promise<unknown> {
  const profile = args.expectedProfile as string;
  switch (cmd) {
    case "work_call":
      return workCall(profile, args.call);
    case "work_activity":
      return {
        version: 1,
        profile,
        work: args.work,
        signals: [],
        pages: pages.get(args.work) ?? [],
        error: null,
      };
    case "work_human_pages":
      return { version: 1, profile, work: args.work, pages: [], error: null };
    case "resource_call": {
      const id = (args.call as { id?: string }).id;
      const asset = id ? media.get(id) : undefined;
      return {
        profile,
        response: asset
          ? {
              kind: "record",
              record: {
                id,
                revision: "1",
                created_at: "0",
                updated_at: "0",
                trashed: false,
                draft: { content: { kind: "media", asset } },
              },
            }
          : { kind: "error", error: "not_found" },
      };
    }
    case "note_call":
      return { profile, response: { kind: "error", error: "not_found" } };
    case "favicon_probe":
      return true;
    case "media_admit_remote":
      return { status: "error", error: "unavailable" };
    case "plugin:event|listen":
      return ++callbackId;
    case "plugin:event|unlisten":
      return null;
    default:
      unknown.add(cmd);
      throw new Error(`unmocked ${cmd}`);
  }
}
(window as unknown as { __TAURI_INTERNALS__: unknown }).__TAURI_INTERNALS__ = {
  invoke,
  transformCallback(callback: (value: unknown) => void) {
    const id = ++callbackId;
    callbacks.set(id, callback);
    return id;
  },
  unregisterCallback(id: number) {
    callbacks.delete(id);
  },
  runCallback(id: number, value: unknown) {
    callbacks.get(id)?.(value);
  },
  callbacks,
  convertFileSrc: (path: string) => path,
  metadata: { currentWindow: { label: "main" }, currentWebview: { label: "main" } },
  plugins: { path: { sep: "/", delimiter: ":" } },
};

const EXTENSION: Record<string, string> = {
  "image/png": "png",
  "image/jpeg": "jpg",
  "image/webp": "webp",
  "image/gif": "gif",
};
const CROCKFORD = "0123456789ABCDEFGHJKMNPQRSTVWXYZ";
const ULID = /\b[0-9A-HJKMNP-TV-Z]{26}\b/gu;
const PROFILE = "01M3CTWDG20GFZPACD7905RSRJ";
const SPACE = "01M3CTWDG2AVF2G6YPBVJHJK5X";
const DRAFT = "01M3CTWDG2AVF2G6YPBVJHJK60";
let touched = 1_700_000_000_000;
/** Fresh copies keep their originals' picture addresses: what grows then is not the image cache. */
const sameUrls = new URLSearchParams(location.search).has("same");

async function load(name: string): Promise<Raw | null> {
  const response = await fetch(`/${name}/scene.json`);
  return response.ok ? ((await response.json()) as Raw) : null;
}

/** A work's copy with every id its own (the last ULID character set by the copy), its files mapped. */
function copy(raw: Raw, mark: number | null, name: string, sub = 0): Raw {
  const renamed = (text: string) =>
    mark === null
      ? text
      : text.replace(ULID, (id) =>
          id === PROFILE || id === raw.snapshot.space
            ? id
            : `${id.slice(0, 24)}${CROCKFORD[sub]}${CROCKFORD[mark]}`,
        );
  const out = JSON.parse(renamed(JSON.stringify(raw))) as Raw;
  const version = mark === null || sameUrls ? "" : `?v=${mark}`;
  raw.pages.forEach((original, at) => {
    const one = out.pages[at]!;
    frames.set(
      `${one.attempt}-${one.step}`,
      `/${name}/frames/${original.attempt}-${original.step}.png${version}`,
    );
  });
  for (const asset of Object.values(raw.media ?? {}))
    pictures.set(
      asset.digest,
      `/${name}/media/${asset.digest}.${EXTENSION[asset.mime] ?? "png"}${version}`,
    );
  return out;
}

function admit(part: Raw, id: string, title: string) {
  const snapshot = {
    ...part.snapshot,
    id,
    profile: PROFILE,
    space: SPACE,
    title,
    lifecycle: "active",
  } as WorkEnvironmentSnapshot;
  works.set(id, snapshot);
  const executionOf = new Map<string, string>();
  for (const [objective, projection] of Object.entries(part.objectives)) {
    objectives.set(objective, projection);
    for (const execution of projection.executions) executionOf.set(execution.id, objective);
  }
  for (const one of part.pages) {
    const objective = executionOf.get(one.execution);
    if (objective) pages.set(objective, [...(pages.get(objective) ?? []), one]);
  }
  for (const element of snapshot.elements) {
    const reference = element.reference;
    if (reference.kind === "resource")
      media.set(reference.resource, Object.values(part.media ?? {})[0]);
  }
  summaries.push({
    id,
    space: SPACE,
    title,
    lifecycle: "active",
    revision: snapshot.revision,
    name: title,
    requests: [],
    touched_ms: String(touched++),
    empty: !snapshot.elements.length,
  });
}

async function build(names: string[], allDay: string[], mark: number | null): Promise<string[]> {
  const ids: string[] = [];
  const tag = mark === null ? "K" : CROCKFORD[mark]!;
  for (const [at, name] of names.entries()) {
    const raw = await load(name);
    if (!raw) continue;
    const id = `01M3CTWDG2AVF2G6YPBVJH${tag}${CROCKFORD[at]}${CROCKFORD[at]}${CROCKFORD[at]}`;
    admit(copy(raw, mark, name, at), id, mark === null ? name : `${name}-${mark}`);
    ids.push(id);
  }
  if (mark !== null || !allDay.length) return ids;
  const loaded: [string, Raw][] = [];
  for (const name of allDay) {
    const raw = await load(name);
    if (raw) loaded.push([name, raw]);
  }
  const parts: Raw[] = [];
  for (let index = 0; index < 30 && loaded.length; index++) {
    const [name, raw] = loaded[index % loaded.length]!;
    parts.push(copy(raw, 10 + Math.floor(index / loaded.length), name));
  }
  const first = parts[0];
  if (!first) return ids;
  const merged: Raw = {
    snapshot: {
      ...first.snapshot,
      elements: parts.flatMap((part) => part.snapshot.elements),
      relations: parts.flatMap((part) => part.snapshot.relations ?? []),
      view: { ...first.snapshot.view, placements: [] },
    },
    objectives: Object.fromEntries(parts.flatMap((part) => Object.entries(part.objectives))),
    pages: parts.flatMap((part) => part.pages),
    media: Object.fromEntries(parts.flatMap((part) => Object.entries(part.media ?? {}))),
  };
  const id = "01M3CTWDG2AVF2G6YPBVJHJKZZ";
  admit(merged, id, "all day");
  ids.push(id);
  return ids;
}

/** Listeners and observers still attached, by what they are attached to. */
const attached = new Map<string, number>();
{
  const keyOf = (target: EventTarget, type: string) =>
    `${target === window ? "window" : target === document ? "document" : ((target as Node).nodeName ?? target.constructor.name)}:${type}`;
  const live = new WeakMap<object, Set<string>>();
  const add = EventTarget.prototype.addEventListener;
  const remove = EventTarget.prototype.removeEventListener;
  EventTarget.prototype.addEventListener = function (type, listener, options) {
    if (listener && (this === window || this === document)) {
      const key = keyOf(this, type);
      const capture = typeof options === "boolean" ? options : !!options?.capture;
      const set = live.get(listener) ?? new Set();
      const id = `${key}|${capture}`;
      if (!set.has(id)) {
        set.add(id);
        live.set(listener, set);
        attached.set(key, (attached.get(key) ?? 0) + 1);
      }
    }
    return add.call(this, type, listener, options);
  };
  EventTarget.prototype.removeEventListener = function (type, listener, options) {
    if (listener && (this === window || this === document)) {
      const key = keyOf(this, type);
      const capture = typeof options === "boolean" ? options : !!options?.capture;
      const id = `${key}|${capture}`;
      if (live.get(listener)?.delete(id)) attached.set(key, (attached.get(key) ?? 1) - 1);
    }
    return remove.call(this, type, listener, options);
  };
  for (const Observer of ["ResizeObserver", "IntersectionObserver", "MutationObserver"] as const) {
    const Native = window[Observer] as unknown as new (...args: unknown[]) => {
      observe(...a: unknown[]): void;
      disconnect(): void;
    };
    (window as unknown as Record<string, unknown>)[Observer] = class extends Native {
      #on = false;
      #site: string;
      constructor(...args: unknown[]) {
        super(...args);
        const frames = (new Error().stack ?? "")
          .split("\n")
          .filter((line) => line.includes("/assets/"));
        this.#site = `${Observer}@${(frames.find((line) => !line.includes("/assets/index-")) ?? frames[1] ?? "?").replace(/^.*\/assets\//u, "").replace(/-[\w-]{8}\.js/u, "")}`;
      }
      override observe(...a: unknown[]) {
        if (!this.#on) attached.set(this.#site, (attached.get(this.#site) ?? 0) + 1);
        this.#on = true;
        super.observe(...a);
      }
      override disconnect() {
        if (this.#on) attached.set(this.#site, (attached.get(this.#site) ?? 1) - 1);
        this.#on = false;
        super.disconnect();
      }
    };
  }
}
/** Every element the page made, weakly: after a collection, those still alive and detached are retained. */
const made: WeakRef<Element>[] = [];
{
  const note = (node: Node) => {
    if (node instanceof Element) made.push(new WeakRef(node));
    if (node instanceof Element || node instanceof DocumentFragment)
      for (const element of (node as ParentNode).querySelectorAll("*"))
        made.push(new WeakRef(element));
  };
  const clone = Node.prototype.cloneNode;
  Node.prototype.cloneNode = function (deep?: boolean) {
    const copy = clone.call(this, deep);
    note(copy);
    return copy;
  };
  const importNode = Document.prototype.importNode;
  Document.prototype.importNode = function <T extends Node>(node: T, deep?: boolean): T {
    const copy = importNode.call(this, node, deep) as T;
    note(copy);
    return copy;
  };
  const create = Document.prototype.createElement;
  Document.prototype.createElement = function (
    this: Document,
    ...args: Parameters<Document["createElement"]>
  ) {
    const element = create.apply(this, args);
    made.push(new WeakRef(element));
    return element;
  } as Document["createElement"];
}
function detached() {
  const byClass = new Map<string, number>();
  let alive = 0;
  let count = 0;
  for (let at = made.length - 1; at >= 0; at--) {
    const element = made[at]!.deref();
    if (!element) {
      made.splice(at, 1);
      continue;
    }
    alive += 1;
    if (element.isConnected) continue;
    count += 1;
    const name = `${element.localName}.${[...element.classList].join(".")}`.slice(0, 80);
    byClass.set(name, (byClass.get(name) ?? 0) + 1);
  }
  return {
    alive,
    detached: count,
    top: [...byClass].sort((a, b) => b[1] - a[1]).slice(0, 25),
  };
}
const intervals = new Set<number>();
const workers = { made: 0, ended: 0 };
{
  const setI = window.setInterval.bind(window);
  const clearI = window.clearInterval.bind(window);
  window.setInterval = ((handler: TimerHandler, ms?: number, ...rest: unknown[]) => {
    const id = setI(handler, ms, ...rest);
    intervals.add(id);
    return id;
  }) as typeof window.setInterval;
  window.clearInterval = ((id?: number) => {
    if (id !== undefined) intervals.delete(id);
    clearI(id);
  }) as typeof window.clearInterval;
  const Native = window.Worker;
  window.Worker = class extends Native {
    constructor(url: string | URL, options?: WorkerOptions) {
      super(url, options);
      workers.made += 1;
    }
    override terminate() {
      workers.ended += 1;
      super.terminate();
    }
  };
}

const wait = (ms: number) => new Promise((done) => setTimeout(done, ms));
const frame = () => new Promise((done) => requestAnimationFrame(() => done(null)));

function inPage() {
  let canvasBytes = 0;
  const canvases = document.querySelectorAll("canvas");
  for (const canvas of canvases) canvasBytes += canvas.width * canvas.height * 4;
  let imageBytes = 0;
  const images = document.querySelectorAll("img");
  for (const image of images)
    if (image.complete) imageBytes += image.naturalWidth * image.naturalHeight * 4;
  return {
    attached: Object.fromEntries([...attached].filter(([, count]) => count > 0)),
    nodes: document.getElementsByTagName("*").length,
    flowNodes: document.querySelectorAll(".svelte-flow__node").length,
    canvases: canvases.length,
    canvasMB: +(canvasBytes / 1048576).toFixed(1),
    images: images.length,
    imageMB: +(imageBytes / 1048576).toFixed(1),
    workersLive: workers.made - workers.ended,
    intervals: intervals.size,
    animations: document.getAnimations().filter((a) => a.playState === "running").length,
  };
}

async function settle() {
  let last = -1;
  let still = 0;
  for (let tick = 0; tick < 600 && still < 20; tick++) {
    await frame();
    const count = document.querySelectorAll(".svelte-flow__node, canvas, img").length;
    still = count === last ? still + 1 : 0;
    last = count;
  }
  await wait(800);
}

/** Allocation pressure until the collector has run: WebKit gives a page no gc(). */
async function collect() {
  for (let round = 0; round < 6; round++) {
    let held: ArrayBuffer[] = [];
    for (let at = 0; at < 64; at++) held.push(new ArrayBuffer(4 << 20));
    held = [];
    void held;
    await wait(250);
  }
  await wait(3000);
}

const WORKS = [
  "trip",
  "jobs",
  "db",
  "learning",
  "compare",
  "compilers",
  "lego",
  "trip3",
  "yctrip",
  "r4browsers",
];
const ALL_DAY = [
  "trip3",
  "compare",
  "compilers",
  "lunios",
  "day",
  "yctrip",
  "aisaas",
  "lego",
  "learning",
  "jobs",
];

let session: {
  open(id: string): Promise<boolean>;
  start(title: string): Promise<void>;
  reload(): Promise<boolean>;
  snapshot: { id: string } | null;
} | null = null;
let screen: Record<string, unknown> | null = null;
let ids: string[] = [];
let previous: string[] = [];

const harness = {
  inPage,
  collect,
  detached,
  unknown: () => [...unknown],
  async start() {
    works.set(DRAFT, {
      version: 1,
      id: DRAFT,
      profile: PROFILE,
      space: SPACE,
      title: "New work",
      lifecycle: "active",
      revision: "1",
      elements: [],
      areas: [],
      view: { revision: "1", x: 0, y: 0, zoom_milli: 1000, placements: [] },
    } as WorkEnvironmentSnapshot);
    ids = await build(WORKS, ALL_DAY, null);
    summaries.unshift({
      id: DRAFT,
      space: SPACE,
      title: "New work",
      lifecycle: "active",
      revision: "1",
      name: null,
      requests: [],
      touched_ms: String(touched++),
      empty: true,
    });
    const [{ WorkEnvironmentSession }, { default: Workspace }] = await Promise.all([
      import("$domain/work-environment"),
      import("$features/work/components/WorkEnvironmentWorkspace.svelte"),
    ]);
    session = new WorkEnvironmentSession(PROFILE, SPACE) as unknown as typeof session;
    const target = document.getElementById("root")!;
    screen = mount(Workspace, {
      target,
      props: {
        session,
        tabs: [],
        spaceName: "Personal",
        profileLabel: "Reader",
        aiEnabled: true,
        onopen: () => {},
        onnewtab: () => {},
      },
    } as never);
    await session!.start("New work");
    for (let tries = 0; tries < 100 && session!.snapshot?.id !== DRAFT; tries++) await wait(50);
    await wait(1000);
    return ids.length;
  },
  ids: () => ids,
  async open(id: string) {
    const started = performance.now();
    await session!.open(id);
    await settle();
    return Math.round(performance.now() - started);
  },
  async round() {
    for (const id of ids) {
      await session!.open(id);
      await settle();
    }
    await session!.open(DRAFT);
    await settle();
  },
  async fresh(mark: number) {
    // The fake native side lets the last fresh set go: what it held is not the page's own.
    for (const id of previous) {
      const snapshot = works.get(id);
      works.delete(id);
      for (const element of snapshot?.elements ?? [])
        if (element.reference.kind === "objective") {
          objectives.delete(element.reference.objective);
          pages.delete(element.reference.objective);
        }
    }
    const only = new URLSearchParams(location.search).get("only");
    const fresh = await build(only ? Array.from({ length: 10 }, () => only) : WORKS, [], mark);
    previous = fresh;
    if (new URLSearchParams(location.search).has("build-only")) return;
    await session!.reload();
    for (const id of fresh) {
      await session!.open(id);
      await settle();
    }
    await session!.open(DRAFT);
    await settle();
  },
  async close() {
    if (screen) await unmount(screen);
    screen = null;
  },
};
(window as unknown as { harness: typeof harness }).harness = harness;
