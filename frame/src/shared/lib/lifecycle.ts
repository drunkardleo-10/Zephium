export type Unlisten = () => void;

/** Document-lifetime generation guard. Call begin only for a new initialization. */
export function createLifecycle() {
  let generation = 0;
  let active = false;
  return {
    begin() {
      active = true;
      return ++generation;
    },
    isCurrent(value: number) {
      return active && generation === value;
    },
    end() {
      active = false;
      generation++;
    },
  };
}

/** Registrations start at the call site, before await. Partial failures release all successes. */
export async function listenAll(promises: readonly Promise<Unlisten>[]): Promise<Unlisten[]> {
  const results = await Promise.allSettled(promises);
  const listeners: Unlisten[] = [];
  let failure: PromiseRejectedResult | undefined;
  for (const result of results) {
    if (result.status === "fulfilled") listeners.push(result.value);
    else failure ??= result;
  }
  if (failure) {
    for (const stop of listeners) {
      try {
        stop();
      } catch {
        /* Continue releasing siblings; report the original registration failure. */
      }
    }
    throw failure.reason;
  }
  return listeners;
}
