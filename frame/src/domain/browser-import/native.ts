import { commands, type ImportJobView } from "$shared/ipc/bindings";
import { events } from "$shared/ipc/native-events";
import type { ImportAdapter, ImportJob } from "./browser-import.svelte";

function job(view: ImportJobView): ImportJob {
  return {
    source: view.source,
    profile: view.profile,
    kinds: view.kinds.map((kind) => ({
      kind: kind.kind,
      state: kind.state,
      done: kind.done,
      total: kind.total,
      problem: kind.problem,
    })),
    finished: view.finished,
    cancelled: view.cancelled,
  };
}

/** The import port backed by native. A locked file is reported per kind as
 *  it happens, so no source is marked running up front. */
export function nativeImportAdapter(): ImportAdapter {
  return {
    async sources() {
      const sources = await commands.importSources();
      return sources.map((source) => ({
        id: source.id,
        browser: source.browser,
        name: source.name,
        profiles: source.profiles,
        kinds: source.kinds,
        needsPermission: source.needs_permission,
        running: false,
      }));
    },
    start: (source, profile, kinds) => commands.importStart(source, profile, kinds),
    async cancel() {
      await commands.importCancel();
    },
    onProgress(listener) {
      let stop: (() => void) | null = null;
      let disposed = false;
      void events.importProgress
        .listen(({ payload }) => listener(job(payload)))
        .then((off) => {
          if (disposed) off();
          else stop = off;
        });
      return () => {
        disposed = true;
        stop?.();
      };
    },
    async openPermissionSettings(source) {
      await commands.importOpenPermission(source);
    },
  };
}
