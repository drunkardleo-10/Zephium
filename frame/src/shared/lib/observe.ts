/** Bounds observation of a promise. Abort never cancels an admitted native effect. */
export type Observation<T> =
  { state: "received"; value: T } | { state: "failed" | "timeout" | "aborted" };

export function observe<T>(
  request: Promise<T>,
  milliseconds: number,
  signal?: AbortSignal,
): Promise<Observation<T>> {
  return new Promise((resolve) => {
    let finished = false;
    let timer: ReturnType<typeof setTimeout> | undefined = undefined;
    const complete = (result: Observation<T>) => {
      if (finished) return;
      finished = true;
      clearTimeout(timer);
      signal?.removeEventListener("abort", abort);
      resolve(result);
    };
    const abort = () => complete({ state: "aborted" });
    // Always consume late rejection, including already-aborted observations.
    void request.then(
      (value) => complete({ state: "received", value }),
      () => complete({ state: "failed" }),
    );
    if (signal?.aborted) {
      abort();
      return;
    }
    if (!Number.isFinite(milliseconds) || milliseconds <= 0) {
      complete({ state: "timeout" });
      return;
    }
    signal?.addEventListener("abort", abort, { once: true });
    timer = setTimeout(() => complete({ state: "timeout" }), milliseconds);
  });
}

export function pause(milliseconds: number, signal?: AbortSignal): Promise<void> {
  return new Promise((resolve) => {
    let timer: ReturnType<typeof setTimeout> | undefined = undefined;
    const finish = () => {
      clearTimeout(timer);
      signal?.removeEventListener("abort", finish);
      resolve();
    };
    if (signal?.aborted) {
      finish();
      return;
    }
    signal?.addEventListener("abort", finish, { once: true });
    timer = setTimeout(finish, milliseconds);
  });
}
