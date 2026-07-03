import { createEffect, createSignal, on, Show } from "solid-js";

const IS_WINDOWS = navigator.userAgent.includes("Windows");

export function iconUrl(key: string): string {
  const hash = key.lastIndexOf("#");
  const version = hash === -1 ? "" : `?v=${key.slice(hash + 1)}`;
  const base = hash === -1 ? key : key.slice(0, hash);
  const slash = base.indexOf("/");
  const profile = base.slice(0, slash);
  const origin = encodeURIComponent(base.slice(slash + 1));
  const root = IS_WINDOWS ? "http://zicon.localhost" : "zicon://localhost";
  return `${root}/${profile}/${origin}${version}`;
}

// The img stays mounted through loading cycles; swapping elements would
// re-request the icon on every navigation (custom schemes bypass http cache).
export function FavIcon(props: { favicon: string | null; loading?: boolean }) {
  const [failed, setFailed] = createSignal(false);
  createEffect(
    on(
      () => props.favicon,
      () => setFailed(false),
    ),
  );
  return (
    <Show
      when={props.favicon && !failed()}
      fallback={
        <span
          class="h-3.5 w-3.5 shrink-0 rounded-full bg-faint/40"
          classList={{ "animate-pulse bg-accent/70": props.loading }}
        />
      }
    >
      <img
        src={iconUrl(props.favicon!)}
        onError={() => setFailed(true)}
        class="h-3.5 w-3.5 shrink-0 rounded"
        classList={{ "animate-pulse opacity-60": props.loading }}
        alt=""
      />
    </Show>
  );
}
