import { commands } from "$shared/ipc/bindings";
import { events } from "$shared/ipc/native-events";
import { listenAll } from "./lifecycle";
import { observe } from "./observe";

const tasks = new Set<() => Promise<boolean>>();
export function registerCloseTask(task: () => Promise<boolean>): () => void {
  tasks.add(task);
  return () => {
    tasks.delete(task);
  };
}
/** Small document-lifetime host listener, retained even when feature views are hidden. */
export function installCloseService(): () => void {
  let live = true;
  let stops: (() => void)[] = [];
  let current: string | null = null;
  const flush = async () => {
    const deadline = performance.now() + 12_000;
    let okay = true;
    for (const task of [...tasks]) {
      const remaining = deadline - performance.now();
      if (remaining <= 0) return false;
      const result = await observe(Promise.resolve().then(task), remaining);
      okay = result.state === "received" && result.value && okay;
    }
    return okay;
  };
  void listenAll([
    events.resourceClose.listen(({ payload: token }) => {
      if (!live) return;
      current = token;
      document.body.inert = true;
      void flush()
        .then(async (success) => {
          const accepted = await commands.resourceCloseReady(token, success);
          if (live && current === token && (!accepted || !success)) {
            document.body.inert = false;
            current = null;
          }
        })
        .catch(() => {
          if (live && current === token) {
            document.body.inert = false;
            current = null;
          }
        });
    }),
    events.resourceCloseCancelled.listen(({ payload: token }) => {
      if (current === token) {
        document.body.inert = false;
        current = null;
      }
    }),
  ]).then(
    (listeners) => {
      if (live) stops = listeners;
      else for (const stop of listeners) stop();
    },
    () => {
      document.body.inert = false;
    },
  );
  return () => {
    live = false;
    for (const stop of stops) stop();
    document.body.inert = false;
  };
}
