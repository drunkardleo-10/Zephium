import { commands } from "$shared/ipc/bindings";
import { SvelteMap, SvelteSet } from "svelte/reactivity";
import type {
  DownloadCall,
  DownloadError,
  DownloadPreferences,
  DownloadView,
} from "$shared/ipc/bindings";
import { events } from "$shared/ipc/native-events";

/** Profile-bound projection. Native owns transfers, paths and durable history. */
export class DownloadSession {
  readonly profile: string;
  entries = $state.raw<DownloadView[]>([]);
  preferences = $state.raw<DownloadPreferences | null>(null);
  error = $state<DownloadError | null>(null);
  supported = $state(true);
  loading = $state(false);
  busy = $state(false);
  next = $state<string | null>(null);
  private generation = 0;
  private active = false;
  private stopListening: (() => void) | null = null;
  private refreshAgain = false;
  private updating = false;
  private updateAgain = false;
  private removedDuringLoad = new SvelteSet<string>();
  private updatedDuringLoad = new SvelteSet<string>();

  constructor(profile: string) {
    this.profile = profile;
  }

  async start(initial = true) {
    if (this.active) return;
    this.active = true;
    const generation = ++this.generation;
    const stop = await events.downloadsChanged.listen(({ payload }) => {
      if (!this.active || generation !== this.generation || payload.profile !== this.profile)
        return;
      void this.refresh();
    });
    if (!this.active || generation !== this.generation) {
      stop();
      return;
    }
    this.stopListening = stop;
    if (initial) await this.reload();
  }

  stop() {
    this.active = false;
    this.generation++;
    this.stopListening?.();
    this.stopListening = null;
    this.entries = [];
    this.removedDuringLoad.clear();
    this.updatedDuringLoad.clear();
    this.next = null;
    this.loading = false;
    this.refreshAgain = false;
    this.updating = false;
    this.updateAgain = false;
    this.busy = false;
  }

  async refresh() {
    if (this.updating) {
      this.updateAgain = true;
      return;
    }
    const generation = this.generation;
    this.updating = true;
    try {
      const response = await commands.downloadCall(this.profile, { kind: "updates" });
      if (generation !== this.generation) return;
      if (response.kind === "updates") {
        const records = new SvelteMap(this.entries.map((entry) => [entry.id, entry]));
        for (const entry of response.entries) {
          if (this.loading) this.updatedDuringLoad.add(entry.id);
          const old = records.get(entry.id);
          if (!old || entry.revision > old.revision) records.set(entry.id, entry);
        }
        for (const id of response.removed) {
          records.delete(id);
          if (this.loading) this.removedDuringLoad.add(id);
        }
        this.entries = [...records.values()]
          .sort((a, b) => b.id.localeCompare(a.id))
          .slice(0, 2000);
      }
    } catch {
      if (generation === this.generation) this.error = "unavailable";
    } finally {
      if (generation === this.generation) {
        this.updating = false;
        if (this.updateAgain && this.active) {
          this.updateAgain = false;
          void this.refresh();
        }
      }
    }
  }

  async reload(more = false) {
    if (this.loading) {
      this.refreshAgain = true;
      return;
    }
    if (more && (!this.next || this.entries.length >= 2000)) return;
    const generation = this.generation;
    this.loading = true;
    try {
      const response = await commands.downloadCall(this.profile, {
        kind: "list",
        before: more ? this.next : null,
        limit: 50,
      });
      if (generation !== this.generation) return;
      if (response.kind === "page") {
        this.supported = response.supported;
        const entries = response.entries
          .filter((entry) => !this.removedDuringLoad.has(entry.id))
          .map((entry) => {
            const current = this.entries.find((old) => old.id === entry.id);
            return current && current.revision > entry.revision ? current : entry;
          });
        this.entries = more
          ? [
              ...this.entries,
              ...entries.filter((entry) => !this.entries.some((old) => old.id === entry.id)),
            ]
          : [
              ...entries,
              ...this.entries.filter(
                (entry) =>
                  this.updatedDuringLoad.has(entry.id) &&
                  !this.removedDuringLoad.has(entry.id) &&
                  !entries.some((old) => old.id === entry.id),
              ),
            ]
              .sort((a, b) => b.id.localeCompare(a.id))
              .slice(0, 2000);
        this.next = response.next;
        this.error = null;
      } else if (response.kind === "error") {
        this.error = response.error;
      }
    } catch {
      if (generation === this.generation) this.error = "unavailable";
    } finally {
      if (generation === this.generation) {
        this.loading = false;
        this.removedDuringLoad.clear();
        this.updatedDuringLoad.clear();
        if (this.refreshAgain && this.active) {
          this.refreshAgain = false;
          void this.reload();
        }
      }
    }
  }

  async perform(call: DownloadCall) {
    if (this.busy) return;
    const generation = this.generation;
    this.busy = true;
    this.error = null;
    try {
      const response = await commands.downloadCall(this.profile, call);
      if (generation !== this.generation) return;
      if (response.kind === "error") {
        if (response.error !== "cancelled") this.error = response.error;
      } else if (response.kind === "preferences") {
        this.preferences = response.preferences;
        this.supported = response.supported;
      } else if (call.kind === "forget" || call.kind === "cancel") {
        await this.reload();
      }
    } catch {
      if (generation === this.generation) this.error = "unavailable";
    } finally {
      if (generation === this.generation) this.busy = false;
    }
  }
}
