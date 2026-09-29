import { SvelteMap, SvelteSet } from "svelte/reactivity";
import {
  commands,
  type WorkEnvironmentSnapshot,
  type WorkRuntimeProjection,
} from "$shared/ipc/bindings";
import type { MediaAssetV1 } from "$domain/resources";
import type { WorkEnvironmentSession } from "$domain/work-environment";
import { youtubeThumbnail } from "../link-media";
import { subjectImageCandidates, subjectsOf } from "../subjects";

function canonical(url: string): string {
  try {
    return new URL(url).toString();
  } catch {
    return url;
  }
}

/** A subject shows a picture once any "uses" relation leaves it. */
const pictured = (current: WorkEnvironmentSnapshot, element: string) =>
  (current.relations ?? []).some(
    (relation) => relation.from === element && relation.kind === "uses",
  );

const RETRY_MS = 5000;

type Deps = {
  session: () => WorkEnvironmentSession;
  media: () => ReadonlyMap<string, MediaAssetV1>;
  objectives: () => ReadonlyMap<string, WorkRuntimeProjection>;
  /** Elements in or near the view: their pictures are admitted first. */
  near?: () => ReadonlySet<string>;
};

/** A picture still owed: the element that holds it, its own key, where to fetch it, when it is there. */
type Owed = {
  element: string;
  key: string;
  candidates: string[];
  done: (snapshot: WorkEnvironmentSnapshot) => boolean;
};
/** An object's items name at most this many candidates each worth trying. */
const ITEM_CANDIDATES = 2;

/**
 * Subjects and video links name public image candidates. Rust fetches,
 * bounds, decodes and stores an admitted copy; the canvas only ever draws
 * that copy. One queue per canvas: it runs until every subject that can have
 * a picture has one, falls back to the next candidate when Rust refuses one,
 * and retries a refused candidate once after a pause.
 */
export class PictureQueue {
  readonly #deps: Deps;
  /** One admission per candidate address per environment. */
  readonly #admissions = new SvelteSet<string>();
  readonly #refusals = new SvelteMap<string, { count: number; after: number }>();
  readonly #admittedFor = new SvelteSet<string>();
  readonly #gallery = new SvelteSet<string>();
  #running = false;
  #again = false;
  #timer: ReturnType<typeof setTimeout> | undefined;

  constructor(deps: Deps) {
    this.#deps = deps;
  }

  dispose() {
    clearTimeout(this.#timer);
  }

  /** The media element on this canvas that already holds one candidate. */
  #placed(snapshot: WorkEnvironmentSnapshot, address: string): string | undefined {
    const media = this.#deps.media();
    for (const element of snapshot.elements) {
      if (element.reference.kind !== "resource") continue;
      const origin = media.get(element.reference.resource)?.origin;
      if (origin?.kind === "fetched" && canonical(origin.url) === address) return element.id;
    }
    return undefined;
  }

  /** The fetched addresses of the pictures an element already uses. */
  #used(snapshot: WorkEnvironmentSnapshot, element: string): string[] {
    const media = this.#deps.media();
    return (snapshot.relations ?? []).flatMap((relation) => {
      if (relation.kind !== "uses" || relation.from !== element) return [];
      const target = snapshot.elements.find((entry) => entry.id === relation.to)?.reference;
      const origin = target?.kind === "resource" ? media.get(target.resource)?.origin : undefined;
      return origin?.kind === "fetched" ? [canonical(origin.url)] : [];
    });
  }

  /**
   * Pictures still owed, those in view first: each subject's and video
   * link's one picture, and one for each item of a set of picks.
   */
  #unpictured(current: WorkEnvironmentSnapshot): Owed[] {
    const owed = current.elements.flatMap((element): Owed[] => {
      const reference = element.reference;
      if (reference.kind === "artifact") {
        const artifact = this.#deps
          .objectives()
          .get(reference.objective)
          ?.executions.find((entry) => entry.id === reference.execution)
          ?.artifacts.find((entry) => entry.id === reference.artifact);
        if (artifact?.data.kind !== "picks") return [];
        return artifact.data.items.flatMap((item, index): Owed[] => {
          const key = `${element.id}#${index}`;
          const candidates = (item.image_candidates ?? []).slice(0, ITEM_CANDIDATES);
          if (!candidates.length || this.#admittedFor.has(`${current.id} ${key}`)) return [];
          const own = candidates.map(canonical);
          const done = (snapshot: WorkEnvironmentSnapshot) => {
            const used = this.#used(snapshot, element.id);
            return own.some((address) => used.includes(address));
          };
          return done(current) ? [] : [{ element: element.id, key, candidates, done }];
        });
      }
      if (this.#admittedFor.has(`${current.id} ${element.id}`)) return [];
      const done = (snapshot: WorkEnvironmentSnapshot) => pictured(snapshot, element.id);
      if (reference.kind === "link") {
        // A video link's picture is its thumbnail, admitted like a subject's.
        const thumbnail = youtubeThumbnail(reference.url);
        if (!thumbnail || done(current)) return [];
        return [{ element: element.id, key: element.id, candidates: [thumbnail], done }];
      }
      if (reference.kind !== "subject" || done(current)) return [];
      const run = this.#deps
        .objectives()
        .get(reference.objective)
        ?.executions.find((entry) => entry.id === reference.execution);
      const artifact = run?.artifacts.find((entry) => entry.id === reference.artifact);
      const subject = artifact ? subjectsOf(artifact)[reference.index] : undefined;
      const candidates = subject && run ? subjectImageCandidates(run, subject) : [];
      return candidates.length ? [{ element: element.id, key: element.id, candidates, done }] : [];
    });
    const near = this.#deps.near?.();
    return near?.size
      ? [...owed].sort((a, b) => Number(near.has(b.element)) - Number(near.has(a.element)))
      : owed;
  }

  async #admit(environment: string, element: string, candidate: string) {
    try {
      const result = await commands.mediaAdmitRemote(
        this.#deps.session().profile,
        environment,
        element,
        candidate,
      );
      return result.status === "ok" && result.data.kind === "admitted";
    } catch {
      return false;
    }
  }

  async run() {
    if (this.#running) {
      this.#again = true;
      return;
    }
    this.#running = true;
    try {
      do {
        this.#again = false;
        const session = this.#deps.session();
        const current = session.snapshot;
        if (!current) break;
        let retry = Infinity;
        for (const { element, key: owed, candidates, done } of this.#unpictured(current)) {
          for (const candidate of candidates) {
            const latest = session.snapshot ?? current;
            if (latest.id !== current.id || done(latest)) break;
            const address = canonical(candidate);
            const known = this.#placed(latest, address);
            if (known) {
              // The picture is already here: point at it instead of fetching it twice.
              if (
                await session.edit({ kind: "relate", from: element, to: known, relation: "uses" })
              )
                break;
              continue;
            }
            const key = `${current.id} ${address}`;
            const refused = this.#refusals.get(key) ?? { count: 0, after: 0 };
            if (refused.count >= 2) continue;
            if (Date.now() < refused.after) {
              retry = Math.min(retry, refused.after);
              continue;
            }
            this.#admissions.add(key);
            if (await this.#admit(current.id, element, candidate)) {
              this.#admittedFor.add(`${current.id} ${owed}`);
              break;
            }
            const after = Date.now() + RETRY_MS;
            this.#refusals.set(key, { count: refused.count + 1, after });
            if (!refused.count) retry = Math.min(retry, after);
          }
        }
        if (retry !== Infinity) {
          clearTimeout(this.#timer);
          this.#timer = setTimeout(() => void this.run(), Math.max(0, retry - Date.now()));
        }
      } while (this.#again);
    } finally {
      this.#running = false;
    }
  }

  /**
   * Opening a product view is an explicit act: it is worth admitting the
   * other pictures the run observed for that subject, and only then.
   */
  async gallery(element: string, have: readonly { digest: string }[]) {
    const session = this.#deps.session();
    const current = session.snapshot;
    const reference = current?.elements.find((entry) => entry.id === element)?.reference;
    if (!current || reference?.kind !== "subject" || this.#gallery.has(element)) return;
    this.#gallery.add(element);
    const run = this.#deps
      .objectives()
      .get(reference.objective)
      ?.executions.find((execution) => execution.id === reference.execution);
    const artifact = run?.artifacts.find((entry) => entry.id === reference.artifact);
    const subject = artifact ? subjectsOf(artifact)[reference.index] : undefined;
    if (!run || !subject) return;
    const media = [...this.#deps.media().values()];
    const known = have.flatMap((picture) => {
      const origin = media.find((asset) => asset.digest === picture.digest)?.origin;
      return origin?.kind === "fetched" ? [canonical(origin.url)] : [];
    });
    for (const candidate of subjectImageCandidates(run, subject).slice(0, 3)) {
      if (known.length >= 3) break;
      const address = canonical(candidate);
      if (known.includes(address)) continue;
      known.push(address);
      const key = `${current.id} ${address}`;
      if (this.#admissions.has(key)) continue;
      this.#admissions.add(key);
      try {
        await commands.mediaAdmitRemote(session.profile, current.id, element, candidate);
      } catch {
        /* The other pictures are a nicety; the first one already stands. */
      }
    }
  }
}
