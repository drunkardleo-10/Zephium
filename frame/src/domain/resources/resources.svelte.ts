import { registerCloseTask } from "$shared/lib/close";
import { SvelteMap } from "svelte/reactivity";
import { mergePage, newerRevision, noteReferences } from "./resource-model";
import { commands } from "$shared/ipc/bindings";
import type {
  ResourceCall_Deserialize as ResourceCall,
  ResourceKind,
  ResourceDraft_Deserialize as ResourceDraft,
  ResourceRecord_Serialize as ResourceRecord,
  ResourceCommand_Deserialize as ResourceCommand,
  ResourceResponse_Serialize as ResourceResponse,
  ResourceSummary,
} from "$shared/ipc/bindings";
import { events } from "$shared/ipc/native-events";
import { observe } from "$shared/lib/observe";

const AUTOSAVE_DELAY = 1000;

export type SaveState = "saved" | "unsaved" | "saving" | "conflict" | "unknown" | "failed";
/** Profile-bound projections and transient edits. Native owns every durable fact. */
export class ResourceSession {
  readonly profile: string;
  readonly kind: ResourceKind;
  items = $state.raw<ResourceSummary[]>([]);
  record = $state.raw<ResourceRecord | null>(null);
  draft = $state.raw<ResourceDraft | null>(null);
  saveState = $state<SaveState>("saved");
  error = $state<string | null>(null);
  loading = $state(false);
  navigating = $state(false);
  query = $state("");
  trash = $state(false);
  filter = $state<"all" | "open" | "completed">("all");
  private resumeId: string | null = null;
  private pendingAdopts = true;
  next = $state<string | null>(null);
  editorKey = $state(0);
  referencesRevision = $state(0);
  pending: ResourceCommand | null = null;
  private changed = 0;
  private pendingVersion = 0;
  private epoch = 0;
  private listing = 0;
  private lifetime = new AbortController();
  private stop: (() => void) | null = null;
  private refreshTimer: ReturnType<typeof setTimeout> | undefined;
  private searchTimer: ReturnType<typeof setTimeout> | undefined;
  private timer: ReturnType<typeof setTimeout> | undefined;
  private writing: Promise<boolean> | null = null;
  private active = false;
  private requestedId: string | null = null;
  async requestOpen(id: string) {
    if (this.active) await this.open(id);
    else this.requestedId = id;
  }
  readonly removeCloseTask: () => void;
  constructor(profile: string, kind: ResourceKind) {
    this.profile = profile;
    this.kind = kind;
    this.removeCloseTask = registerCloseTask(() => this.flushForClose());
  }
  private async flushForClose(): Promise<boolean> {
    const inactive = !this.active;
    if (inactive) this.lifetime = new AbortController();
    try {
      if (this.pending && ["unknown", "failed"].includes(this.saveState)) await this.retry();
      return await this.flush();
    } finally {
      if (inactive) this.lifetime.abort();
    }
  }
  async start() {
    if (this.active) return;
    this.active = true;
    const epoch = ++this.epoch;
    this.lifetime = new AbortController();
    const stop = await events.resourceChanged.listen(({ payload }) => {
      if (epoch !== this.epoch || payload.profile !== this.profile) return;
      this.scheduleRefresh();
      if (
        this.draft?.content.kind === "note" &&
        noteReferences(this.draft.content.document).includes(payload.id)
      )
        this.referencesRevision++;
      if (
        payload.id === this.record?.id &&
        newerRevision(payload.revision, this.record.revision) &&
        !this.writing
      ) {
        if (this.saveState === "saved") void this.open(payload.id);
        else if (this.saveState !== "unknown") this.saveState = "conflict";
      }
    });
    if (!this.active || epoch !== this.epoch) {
      stop();
      return;
    }
    this.stop = stop;
    await this.reload();
    if (this.requestedId) {
      const id = this.requestedId;
      this.requestedId = null;
      await this.open(id);
      return;
    }
    const selected = this.record?.id ?? this.resumeId;
    if (selected && this.saveState === "saved") await this.open(selected);
    if (this.saveState === "unsaved") void this.flush();
  }
  stopObserving() {
    this.active = false;
    ++this.epoch;
    ++this.listing;
    clearTimeout(this.timer);
    clearTimeout(this.searchTimer);
    clearTimeout(this.refreshTimer);
    this.stop?.();
    this.stop = null;
    this.lifetime.abort();
    // Reload projections on activation; only unresolved drafts must retain bodies.
    this.items = [];
    this.next = null;
    this.loading = false;
    if (this.pending) this.saveState = "unknown";
    if (this.saveState === "saved") {
      this.resumeId = this.record?.id ?? this.resumeId;
      this.record = null;
      this.draft = null;
    }
  }
  private async call(call: ResourceCall): Promise<ResourceResponse> {
    const result = await observe(
      Promise.resolve().then(() => commands.resourceCall(this.profile, call)),
      9000,
      this.lifetime.signal,
    );
    if (result.state !== "received") return { kind: "error", error: "outcome_unknown" };
    const reply = result.value;
    if (reply.profile !== this.profile && reply.response.kind !== "error")
      return { kind: "error", error: "unavailable" };
    return reply.response;
  }
  async resolveNotes(ids: string[]): Promise<ResourceSummary[]> {
    const result = await this.call({ kind: "resolve_notes", ids: ids.slice(0, 64) });
    if (result.kind !== "page") throw new Error("unavailable");
    return result.items;
  }
  async findNotes(search: string): Promise<ResourceSummary[]> {
    const result = await this.call({
      kind: "list",
      query: {
        completed: null,
        kind: "note",
        search: search.slice(0, 512),
        trashed: false,
        after: null,
        limit: 50,
      },
    });
    if (result.kind !== "page") throw new Error("unavailable");
    return result.items;
  }
  private scheduleRefresh() {
    if (!this.active) return;
    clearTimeout(this.refreshTimer);
    this.refreshTimer = setTimeout(() => {
      void this.reload();
    }, 100);
  }
  async reload(more = false) {
    clearTimeout(this.refreshTimer);
    const generation = ++this.listing;
    this.loading = true;
    const result = await this.call({
      kind: "list",
      query: {
        completed:
          this.kind === "task" && this.filter !== "all" ? this.filter === "completed" : null,
        kind: this.kind,
        search: this.query,
        trashed: this.trash,
        after: more ? this.next : null,
        limit: 100,
      },
    });
    if (generation !== this.listing || !this.active) return;
    this.loading = false;
    if (result.kind === "page") {
      this.items = more ? mergePage(this.items, result.items) : result.items;
      this.next = this.items.length < 1000 ? result.next : null;
      this.error = null;
    } else this.error = result.kind === "error" ? result.error : "unavailable";
  }
  async open(id: string) {
    if (this.navigating) return;
    this.navigating = true;
    try {
      if (!(await this.flush())) return;
      const epoch = this.epoch;
      const result = await this.call({ kind: "get", id });
      if (epoch !== this.epoch) return;
      if (result.kind !== "record") {
        this.error = result.kind === "error" ? result.error : "unavailable";
        return;
      }
      if (result.record.draft.content.kind !== this.kind) {
        this.error = "invalid";
        return;
      }
      this.resumeId = null;
      this.record = result.record;
      this.draft = structuredClone(result.record.draft);
      this.saveState = "saved";
      this.pending = null;
      this.editorKey++;
      this.changed++;
    } finally {
      this.navigating = false;
    }
  }
  get canEdit(): boolean {
    return (
      !this.navigating &&
      !this.record?.trashed &&
      !(
        this.saveState === "unknown" &&
        (!this.pendingAdopts ||
          this.pending?.intent.kind === "trash" ||
          this.pending?.intent.kind === "restore")
      )
    );
  }
  edit(patch: Partial<ResourceDraft>) {
    if (!this.draft || !this.canEdit) return;
    if (
      Object.entries(patch).every(
        ([key, value]) => this.draft?.[key as keyof ResourceDraft] === value,
      )
    )
      return;
    this.draft = { ...this.draft, ...patch };
    this.changed++;
    if (["conflict", "unknown"].includes(this.saveState)) return;
    this.saveState = "unsaved";
    clearTimeout(this.timer);
    this.timer = setTimeout(() => {
      void this.flush(false);
    }, AUTOSAVE_DELAY);
  }
  async create(title: string) {
    if (this.navigating) return;
    this.navigating = true;
    try {
      if (!(await this.flush())) return;
      const draft: ResourceDraft = {
        title,
        pinned: false,
        related: [],
        content:
          this.kind === "note"
            ? {
                kind: "note",
                document: {
                  version: 1,
                  document: { type: "doc", content: [{ type: "paragraph" }] },
                },
              }
            : { kind: "task", description: "", completed: false, due_date: null },
      };
      this.pending = {
        version: 1,
        request_id: crypto.randomUUID(),
        intent: { kind: "create", draft },
      };
      this.draft = draft;
      this.record = null;
      this.changed++;
      this.editorKey++;
      this.pendingVersion = this.changed;
      await this.write();
    } finally {
      this.navigating = false;
    }
  }
  async flush(drain = true): Promise<boolean> {
    clearTimeout(this.timer);
    if (this.writing) {
      if (!drain) return false;
      if (!(await this.writing)) return false;
    }
    if (["conflict", "unknown", "failed"].includes(this.saveState)) return false;
    if (!this.draft || this.saveState === "saved") return true;
    if (!this.record) return false;
    this.pending = {
      version: 1,
      request_id: crypto.randomUUID(),
      intent: {
        kind: "replace",
        id: this.record.id,
        expected_revision: this.record.revision,
        draft: structuredClone(this.draft),
      },
    };
    this.pendingVersion = this.changed;
    if (!(await this.write())) return false;
    return drain && this.saveState === "unsaved" ? this.flush() : true;
  }
  private write(): Promise<boolean> {
    if (this.writing) return this.writing;
    const command = this.pending;
    if (!command) return Promise.resolve(false);
    const adopt = this.pendingAdopts;
    const version = this.pendingVersion;
    const epoch = this.epoch;
    this.saveState = "saving";
    const work = (async () => {
      const result = await this.call({ kind: "mutate", command });
      if (epoch !== this.epoch) return false;
      if (result.kind !== "applied" || result.request_id !== command.request_id) {
        this.saveState =
          result.kind === "error" && result.error === "conflict"
            ? "conflict"
            : result.kind === "error" && result.error !== "outcome_unknown"
              ? "failed"
              : "unknown";
        this.error = result.kind === "error" ? result.error : "outcome_unknown";
        return false;
      }
      if (adopt || this.record?.id === result.record.id) this.record = result.record;
      this.pending = null;
      this.pendingAdopts = true;
      if (!adopt) {
        if (this.record?.id === result.record.id) this.draft = structuredClone(result.record.draft);
        this.saveState = "saved";
        this.error = null;
        void this.call({ kind: "acknowledge", request_id: command.request_id });
        this.scheduleRefresh();
        return true;
      }
      if (result.applied_revision !== result.record.revision) {
        this.saveState = "conflict";
        return false;
      }
      if (version === this.changed) {
        this.draft = structuredClone(result.record.draft);
        this.saveState = "saved";
      } else this.saveState = "unsaved";
      this.error = null;
      void this.call({ kind: "acknowledge", request_id: command.request_id });
      this.scheduleRefresh();
      return true;
    })();
    this.writing = work;
    void work.finally(() => {
      if (this.writing === work) this.writing = null;
      if (this.active && this.saveState === "unsaved") {
        clearTimeout(this.timer);
        this.timer = setTimeout(() => {
          void this.flush(false);
        }, AUTOSAVE_DELAY);
      }
    });
    return work;
  }
  async retry() {
    if (this.pending) await this.write();
    else if (this.record && this.draft) {
      this.saveState = "unsaved";
      await this.flush();
    }
  }
  search(value: string) {
    this.query = value.slice(0, 512);
    clearTimeout(this.searchTimer);
    this.searchTimer = setTimeout(() => {
      this.scheduleRefresh();
    }, 180);
  }
  get disposable() {
    return !this.active && this.saveState === "saved";
  }
  async back() {
    if (await this.flush()) {
      this.record = null;
      this.draft = null;
      this.resumeId = null;
      this.editorKey++;
    }
  }
  async keepDraft() {
    if (!this.record || !this.draft || this.saveState === "unknown") return;
    const result = await this.call({ kind: "get", id: this.record.id });
    if (result.kind !== "record" || result.record.trashed) return;
    this.record = result.record;
    this.pending = null;
    this.saveState = "unsaved";
    await this.flush();
  }
  async discardDraft() {
    if (!this.record || this.saveState === "unknown") return;
    const result = await this.call({ kind: "get", id: this.record.id });
    if (result.kind === "record") {
      this.resumeId = null;
      this.record = result.record;
      this.draft = structuredClone(result.record.draft);
      this.pending = null;
      this.saveState = "saved";
      this.editorKey++;
    }
  }
  async setTaskCompleted(item: ResourceSummary, completed: boolean) {
    if (this.navigating) return;
    this.navigating = true;
    try {
      if (!(await this.flush())) return;
      const result = await this.call({ kind: "get", id: item.id });
      if (result.kind !== "record" || result.record.draft.content.kind !== "task") return;
      if (result.record.revision !== item.revision) {
        this.error = "conflict";
        this.scheduleRefresh();
        return;
      }
      const draft: ResourceDraft = {
        ...result.record.draft,
        content: { ...result.record.draft.content, completed },
      };
      this.pendingAdopts = false;
      this.pending = {
        version: 1,
        request_id: crypto.randomUUID(),
        intent: { kind: "replace", id: item.id, expected_revision: item.revision, draft },
      };
      this.pendingVersion = this.changed;
      await this.write();
    } finally {
      this.navigating = false;
    }
  }
  async setTrashed(trashed: boolean) {
    if (this.navigating) return;
    this.navigating = true;
    try {
      if (!(await this.flush()) || !this.record) return;
      this.pending = {
        version: 1,
        request_id: crypto.randomUUID(),
        intent: {
          kind: trashed ? "trash" : "restore",
          id: this.record.id,
          expected_revision: this.record.revision,
        },
      };
      this.pendingVersion = this.changed;
      await this.write();
    } finally {
      this.navigating = false;
    }
  }
}

// Retain unsaved drafts across utility hide/show. This is bounded transient state,
// not a second database; clean inactive sessions can always be reconstructed.
const sessions = new SvelteMap<string, ResourceSession>();
export function resourceSession(
  profile: string,
  kind: ResourceKind,
  host: string,
): ResourceSession | null {
  const key = `${profile}:${kind}:${host}`;
  const existing = sessions.get(key);
  if (existing) return existing;
  if (sessions.size >= 16) {
    const clean = [...sessions].find(([, session]) => session.disposable);
    if (!clean) return null;
    clean[1].stopObserving();
    clean[1].removeCloseTask();
    sessions.delete(clean[0]);
  }
  const session = new ResourceSession(profile, kind);
  sessions.set(key, session);
  return session;
}
