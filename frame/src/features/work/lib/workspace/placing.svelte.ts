import { commands, type WorkEnvironmentReference } from "$shared/ipc/bindings";
import type { WorkEnvironmentSession } from "$domain/work-environment";
import { defaultSize, type CanvasItem, type CanvasPosition } from "../canvas-model";
import { elementFor } from "../organize";

/** Only an explicit https or http address; anything else is not a link. */
function linkUrl(raw: string): string | null {
  const text = raw.trim();
  if (!text) return null;
  try {
    const url = new URL(/^[a-z][a-z0-9+.-]*:/iu.test(text) ? text : `https://${text}`);
    return url.protocol === "https:" || url.protocol === "http:" ? url.toString() : null;
  } catch {
    return null;
  }
}

const hostOf = (url: string) => {
  try {
    return new URL(url).host.replace(/^www\./u, "");
  } catch {
    return "";
  }
};

type Deps = {
  session: () => WorkEnvironmentSession;
  /** Something else is in flight: nothing new is placed meanwhile. */
  busy: () => boolean;
  /** The middle of the visible canvas, in canvas coordinates. */
  centre: () => CanvasPosition | null;
  /** Where a window point lands on the canvas. */
  at: (x: number, y: number) => CanvasPosition | null;
  /** A dropped file: its path joins the request, so the run asks for its folder in place. */
  file?: (path: string) => void;
};

/**
 * The person's own things put on the canvas: an element added and stood
 * where they dropped it, or in the middle; folders the application admits
 * first; pasted links.
 */
export class CanvasPlacing {
  folderPending = $state(false);
  folderRefused = $state(false);
  #notice: ReturnType<typeof setTimeout> | undefined;
  readonly #deps: Deps;

  constructor(deps: Deps) {
    this.#deps = deps;
  }

  /** Adds an element and stands it where the person put it, or in the middle. */
  async place(
    reference: WorkEnvironmentReference,
    type: CanvasItem["type"],
    at: CanvasPosition | null,
  ): Promise<boolean> {
    const session = this.#deps.session();
    if (session.snapshot && elementFor(session.snapshot, reference)) return true;
    if (!(await session.flushView())) return false;
    if (!(await session.edit({ kind: "add", reference, area: null }))) return false;
    const current = session.snapshot;
    const element = current ? elementFor(current, reference) : undefined;
    const point = at ?? this.#deps.centre();
    if (!current || !element || !point) return true;
    const size = defaultSize({ id: element.id, title: "", kind: "", detail: "", status: "", type });
    session.checkpoint({
      ...current.view,
      placements: [
        ...current.view.placements.filter((place) => place.element !== element.id),
        {
          element: element.id,
          x: Math.round(point.x - size.width / 2),
          y: Math.round(point.y - size.height / 2),
          ...size,
        },
      ],
    });
    return true;
  }

  /** One quiet line, and it goes away on its own. */
  #refuse() {
    this.folderRefused = true;
    clearTimeout(this.#notice);
    this.#notice = setTimeout(() => (this.folderRefused = false), 6000);
  }

  /**
   * A folder becomes a card only after the application admits its path. A
   * file among dropped paths is not a refusal a person needs to hear about.
   */
  async addFolder(path: string, at?: CanvasPosition | null, dropped = false): Promise<boolean> {
    if (this.folderPending || this.#deps.busy()) return false;
    this.folderPending = true;
    try {
      const profile = this.#deps.session().profile;
      const admitted = await commands.workAdmitFolder(profile, path).catch(() => null);
      if (admitted?.status !== "ok" || admitted.data.kind !== "admitted") {
        const file =
          admitted?.status === "ok" &&
          admitted.data.kind === "refused" &&
          admitted.data.not_a_folder;
        if (dropped && file) this.#deps.file?.(path);
        else this.#refuse();
        return false;
      }
      return await this.#placeFolder(admitted.data, at ?? null);
    } finally {
      this.folderPending = false;
    }
  }

  #placeFolder(folder: { path: string; name: string }, at: CanvasPosition | null) {
    return this.place({ kind: "folder", path: folder.path, name: folder.name }, "folder", at);
  }

  /** The native folder picker; a cancelled choice says nothing. Whether one was placed. */
  async chooseFolder(): Promise<boolean> {
    if (this.folderPending || this.#deps.busy()) return false;
    this.folderPending = true;
    try {
      const chosen = await commands.workPickFolder(this.#deps.session().profile).catch(() => null);
      if (chosen?.status !== "ok" || !chosen.data) return false;
      if (chosen.data.kind !== "admitted") {
        this.#refuse();
        return false;
      }
      return await this.#placeFolder(chosen.data, null);
    } finally {
      this.folderPending = false;
    }
  }

  /** Shows an admitted folder, or a file inside one, where it lives. */
  reveal(path: string) {
    void commands.workRevealPath(this.#deps.session().profile, path).catch(() => null);
  }

  /** A pasted link becomes a card of its own; it opens in the pane, like a source. */
  async addLink(raw: string): Promise<boolean> {
    const url = linkUrl(raw);
    if (!url || this.#deps.busy()) return false;
    return this.place({ kind: "link", url, title: hostOf(url) }, "link", null);
  }

  /**
   * Finder drops: the application hands the frame the dropped paths and the
   * drop point in CSS pixels. Granted folders land where they were dropped; a
   * file's path joins the request being written.
   */
  listen(): () => void {
    const dropped = (event: Event) => {
      const detail = (event as CustomEvent<{ paths?: unknown; x?: unknown; y?: unknown }>).detail;
      const paths = Array.isArray(detail?.paths)
        ? detail.paths.filter((path): path is string => typeof path === "string")
        : [];
      if (!paths.length) return;
      const at =
        typeof detail?.x === "number" && typeof detail?.y === "number"
          ? this.#deps.at(detail.x, detail.y)
          : null;
      void this.#drop(paths.slice(0, 8), at);
    };
    window.addEventListener("zephium:work-paths-dropped", dropped);
    return () => {
      window.removeEventListener("zephium:work-paths-dropped", dropped);
      clearTimeout(this.#notice);
    };
  }

  async #drop(paths: readonly string[], at: CanvasPosition | null) {
    let index = 0;
    for (const path of paths) {
      const placed = await this.addFolder(
        path,
        at ? { x: at.x + index * 24, y: at.y + index * 24 } : null,
        true,
      );
      if (placed) index += 1;
    }
  }
}
