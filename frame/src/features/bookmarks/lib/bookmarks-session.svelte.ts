import type {
  BookmarkCall,
  BookmarkCrumb,
  BookmarkError,
  BookmarkResponse,
  BookmarkView,
} from "$shared/ipc/bindings";
import { commands } from "$shared/ipc/bindings";

const SEARCH_DEBOUNCE_MS = 160;

/** One profile's Bookmarks panel: the folder in view, or search results, and
 *  the writes made from it. Native owns the tree; every change is read back. */
export class BookmarksSession {
  readonly profile: string;

  folder = $state<string | null>(null);
  path = $state.raw<BookmarkCrumb[]>([]);
  items = $state.raw<BookmarkView[]>([]);
  query = $state("");
  loading = $state(false);
  error = $state<BookmarkError | null>(null);
  /** The bookmark just added or revealed, drawn apart until it is used. */
  highlighted = $state<string | null>(null);

  private reads = 0;
  private searchTimer: ReturnType<typeof setTimeout> | undefined;

  constructor(profile: string) {
    this.profile = profile;
  }

  get searching() {
    return this.query.trim() !== "";
  }

  get empty() {
    return !this.loading && this.error === null && this.items.length === 0;
  }

  private async call(call: BookmarkCall): Promise<BookmarkResponse> {
    try {
      return await commands.bookmarkCall(this.profile, call);
    } catch {
      return { kind: "error", error: "unavailable" };
    }
  }

  /** Applies a listing or results, unless a newer read has started since. */
  private async read(call: BookmarkCall) {
    const read = ++this.reads;
    this.loading = true;
    const response = await this.call(call);
    if (read !== this.reads) return;
    this.loading = false;
    if (response.kind === "listing") {
      this.error = null;
      this.folder = response.folder;
      this.path = response.path;
      this.items = response.items;
    } else if (response.kind === "results") {
      this.error = null;
      this.items = response.items;
    } else if (response.kind === "error") {
      // A folder removed elsewhere reads as missing; fall back to the top.
      if (response.error === "missing" && call.kind === "list" && call.folder !== null) {
        void this.open(null);
        return;
      }
      this.error = response.error;
    }
  }

  open(folder: string | null) {
    clearTimeout(this.searchTimer);
    this.query = "";
    return this.read({ kind: "list", folder });
  }

  /** Shows `id` where it lives and draws it apart. */
  async reveal(id: string) {
    clearTimeout(this.searchTimer);
    this.query = "";
    this.highlighted = id;
    await this.read({ kind: "reveal", id });
  }

  search(value: string) {
    this.query = value.slice(0, 512);
    clearTimeout(this.searchTimer);
    if (!this.searching) {
      void this.read({ kind: "list", folder: this.folder });
      return;
    }
    const query = this.query;
    this.searchTimer = setTimeout(
      () => void this.read({ kind: "search", query }),
      SEARCH_DEBOUNCE_MS,
    );
  }

  private refresh() {
    return this.searching
      ? this.read({ kind: "search", query: this.query })
      : this.read({ kind: "list", folder: this.folder });
  }

  /** A write either applies and the view is read again, or reports why not. */
  private async write(call: BookmarkCall): Promise<string | null | false> {
    const response = await this.call(call);
    if (response.kind === "error") {
      this.error = response.error;
      return false;
    }
    await this.refresh();
    return response.kind === "saved" ? response.id : null;
  }

  rename(id: string, title: string) {
    return this.write({ kind: "rename", id, title });
  }

  remove(id: string) {
    if (this.highlighted === id) this.highlighted = null;
    return this.write({ kind: "remove", id });
  }

  async addFolder(title: string) {
    const id = await this.write({ kind: "add_folder", parent: this.folder, title });
    if (typeof id === "string") this.highlighted = id;
    return id;
  }

  move(id: string, parent: string | null, index: number) {
    return this.write({ kind: "move", id, parent, index });
  }

  stop() {
    this.reads += 1;
    clearTimeout(this.searchTimer);
  }
}
